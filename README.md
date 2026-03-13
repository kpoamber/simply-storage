# Innovare Storage

High-performance distributed file storage system built with Rust (Actix-Web) and PostgreSQL + Citus. Receives files via HTTP API, deduplicates by SHA-256 hash, distributes across multiple storage backends, supports hot/cold tiering, and provides temporary download/preview links. Includes an admin web UI and multi-node clustering.

## Architecture

```
                    ┌─────────┐
                    │  nginx   │ :80/:443
                    └────┬─────┘
                         │ least_conn
              ┌──────────┼──────────┐
              ▼          ▼          ▼
         ┌────────┐ ┌────────┐ ┌────────┐
         │ app-1  │ │ app-2  │ │ app-N  │  :8080
         └───┬────┘ └───┬────┘ └───┬────┘
             │          │          │
             └──────────┼──────────┘
                        ▼
              ┌──────────────────┐
              │   PostgreSQL     │
              │  (Citus coord)  │
              └───┬─────────┬───┘
                  ▼         ▼
            ┌─────────┐ ┌─────────┐
            │ worker-1│ │ worker-2│
            └─────────┘ └─────────┘
```

Multiple stateless app instances run behind nginx and share state through distributed PostgreSQL (Citus). Each instance runs background sync workers coordinated via PostgreSQL advisory locks, registers itself in a `nodes` table, and sends heartbeats every 30 seconds.

### Key Features

- **Content-addressable deduplication** - Files stored by SHA-256 hash, duplicates detected automatically
- **Multi-backend storage** - S3, Azure Blob, GCS, DigitalOcean Spaces, Hetzner StorageBox, FTP, SFTP, Samba, local disk
- **Automatic file sync** - Background workers distribute files across configured storage backends
- **Hot/cold tiering** - Configurable per-project archival policy based on last access time
- **Temporary signed links** - HMAC-signed download/preview URLs with configurable expiry
- **Horizontal scaling** - Add app instances behind nginx; new nodes join via config sync from existing nodes
- **Authentication & authorization** - JWT-based auth with access/refresh tokens, role-based access control (admin/user), project ownership, and user-to-project/storage membership assignments
- **Admin dashboard** - React frontend for managing projects, storages, files, and monitoring sync tasks
- **Bulk operations** - Sync-all to a storage, export storage as tar.gz archive

### Data Flow

1. **Upload** - FileService computes SHA-256, deduplicates, stores to primary (hot) storage, creates sync tasks for other backends
2. **Sync** - SyncWorker picks pending tasks with distributed advisory locks, downloads from source, uploads to target
3. **Tiering** - TierWorker scans files older than `hot_to_cold_days`, creates sync tasks to cold storage
4. **Download** - FileService finds first available location (prefers hot), streams content, updates `last_accessed_at`

## Technology Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust, Actix-Web 4, Tokio |
| Database | PostgreSQL + Citus |
| Frontend | React 18, TypeScript, Vite, Tailwind CSS |
| Authentication | jsonwebtoken (JWT), argon2 (password hashing) |
| Storage SDKs | aws-sdk-s3, reqwest (WebDAV), suppaftp, russh-sftp, pavao (Samba) |
| Containerization | Docker (multi-stage build) |
| Load Balancer | Nginx |
| CI/CD | GitHub Actions, GHCR |

## Setup & Build

### Prerequisites

- Rust 1.82+ (with cargo)
- Node.js 20+ (with npm)
- PostgreSQL 15+ (with Citus extension for distributed mode)
- Docker & Docker Compose (for containerized deployment)

### Local Development

```bash
# Clone the repository
git clone https://github.com/yourorg/innovare-storage.git
cd innovare-storage

# Build the backend
cargo build

# Build the frontend
cd frontend && npm install && npm run build && cd ..

# Run with default config (requires PostgreSQL)
cargo run
```

The server starts on `http://0.0.0.0:8080` by default. The admin frontend is served at the root path `/`.

### Docker Compose (Full Stack)

```bash
# Start everything (nginx, app x2, postgres coordinator, 2 citus workers)
docker-compose up --build

# Scale app instances
docker-compose up --build --scale app=3
```

Access at `http://localhost`. See [deploy/README-deploy.md](deploy/README-deploy.md) for cloud deployment.

### Running Tests

```bash
# Backend tests
cargo test

# Lint
cargo clippy -- -D warnings

# Frontend tests
cd frontend && npm test

# Frontend lint
cd frontend && npm run lint
```

## Configuration

Configuration is loaded from `config/default.toml` with environment variable overrides. Environment variables use the `APP_` prefix with `__` as separator (e.g., `APP_SERVER__PORT=9090`).

### Configuration Reference

```toml
[server]
host = "0.0.0.0"       # Bind address
port = 8080             # HTTP port

[database]
url = "postgres://localhost:5432/innovare_storage"  # PostgreSQL connection string
max_connections = 10    # Connection pool size

[storage]
local_temp_path = "./data/temp"             # Local temp file storage path
hmac_secret = "change-me-in-production"     # HMAC secret for signed URLs

[sync]
num_workers = 4              # Number of background sync workers
max_retries = 5              # Max retries for failed sync tasks
poll_interval_secs = 5       # How often workers poll for pending tasks
tier_scan_interval_secs = 300  # How often to scan for files to archive

[auth]
jwt_secret = "change-me-jwt-secret-in-production"  # JWT signing secret (MUST change in production)
access_token_ttl_secs = 900        # Access token lifetime (15 minutes)
refresh_token_ttl_secs = 604800    # Refresh token lifetime (7 days)
default_admin_username = "admin"            # Default admin user created on first startup
default_admin_password = "Innovare2026!"    # Default admin password (MUST change in production)
```

### Environment Variable Examples

```bash
APP_SERVER__PORT=9090
APP_DATABASE__URL=postgres://user:pass@db:5432/mydb
APP_DATABASE__MAX_CONNECTIONS=20
APP_STORAGE__HMAC_SECRET=my-secret-key
APP_SYNC__NUM_WORKERS=8
APP_AUTH__JWT_SECRET=my-jwt-secret
APP_AUTH__ACCESS_TOKEN_TTL_SECS=900
APP_AUTH__REFRESH_TOKEN_TTL_SECS=604800
APP_AUTH__DEFAULT_ADMIN_USERNAME=admin
APP_AUTH__DEFAULT_ADMIN_PASSWORD=Innovare2026!
RUST_LOG=info  # Log level: trace, debug, info, warn, error
```

## API Endpoints

All API endpoints require authentication via a JWT access token passed in the `Authorization: Bearer <token>` header, except `/health`, `/api/auth/login`, and `/api/auth/refresh`. Storage, bulk, system management, and user management endpoints require `admin` role. Project and file endpoints enforce owner/member/admin access.

### Authentication

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/auth/login` | Login with username/password, returns access + refresh tokens |
| POST | `/api/auth/refresh` | Refresh access token using refresh token (rotates refresh token) |
| GET | `/api/auth/me` | Get current user info (requires auth) |
| POST | `/api/auth/logout` | Revoke refresh token |
| POST | `/api/auth/users` | Create new user (admin-only) |
| GET | `/api/auth/users` | List all users (admin-only) |
| GET | `/api/auth/users/{user_id}` | Get user detail with project/storage assignments (admin-only) |
| PUT | `/api/auth/users/{user_id}` | Update user role or password (admin-only) |
| DELETE | `/api/auth/users/{user_id}` | Delete user (admin-only) |

### Projects

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/projects` | Create a project |
| GET | `/api/projects` | List all projects (admin) or accessible projects (user) |
| GET | `/api/projects/{id}` | Get project with file stats |
| PUT | `/api/projects/{id}` | Update project settings |
| DELETE | `/api/projects/{id}` | Delete project |
| GET | `/api/projects/{id}/members` | List project members (admin-only) |
| POST | `/api/projects/{id}/members` | Add member to project (admin-only) |
| DELETE | `/api/projects/{id}/members/{user_id}` | Remove member from project (admin-only) |

### Files

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/projects/{project_id}/files` | Upload file (multipart/form-data) |
| GET | `/api/projects/{project_id}/files` | List project files (paginated) |
| GET | `/api/files/{id}` | Get file metadata with locations |
| GET | `/api/files/{id}/download` | Download file |
| GET | `/api/files/{id}/link` | Generate temporary signed download link |
| DELETE | `/api/files/{id}` | Delete file reference |
| POST | `/api/files/{id}/restore` | Restore file from cold tier |

### Storages

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/storages` | Register storage backend (admin-only) |
| GET | `/api/storages` | List storages with usage stats (admin: all, user: assigned only) |
| GET | `/api/storages/{id}` | Get storage details |
| PUT | `/api/storages/{id}` | Update storage config |
| DELETE | `/api/storages/{id}` | Disable storage |
| GET | `/api/storages/{id}/files` | List files in storage (paginated) |
| GET | `/api/storages/{id}/containers` | List storage containers/buckets |
| POST | `/api/storages/{id}/containers` | Create a new container/bucket |
| GET | `/api/storages/{id}/members` | List storage members (admin-only) |
| POST | `/api/storages/{id}/members` | Add member to storage (admin-only) |
| DELETE | `/api/storages/{id}/members/{user_id}` | Remove member from storage (admin-only) |

### Bulk Operations

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/storages/{id}/sync-all` | Sync all missing files to this storage |
| POST | `/api/storages/{id}/export` | Start tar.gz export of storage |
| GET | `/api/storages/{id}/export/status` | Poll export progress |
| GET | `/api/storages/{id}/export/download` | Download completed export archive |

### System

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/api/system/stats` | Aggregate system statistics |
| GET | `/api/system/nodes` | List active cluster nodes |
| GET | `/api/system/config-export` | Export config for node bootstrapping |
| GET | `/api/sync-tasks` | List sync tasks (filterable by status, storage_id) |

### Supported Storage Types

| Type | Config Fields |
|------|--------------|
| `local` | `path` |
| `s3` | `region`, `bucket`, `endpoint_url` (optional, for S3-compatible e.g. DigitalOcean Spaces, MinIO), `access_key_id`, `secret_access_key`, `prefix`, `force_path_style` (optional), `multipart_threshold` (optional), `part_size` (optional) |
| `azure` | `account_name`, `account_key`, `container`, `prefix`, `endpoint` (optional, for Azurite or sovereign clouds) |
| `gcs` | `bucket`, `prefix`, `client_email`, `private_key_pem`, `token_uri` (optional) |
| `hetzner` | `host`, `username`, `password`, `port`, `base_path`, `sub_account` (optional) |
| `ftp` | `host`, `port`, `username`, `password`, `base_path` |
| `sftp` | `host`, `port`, `username`, `password`, `base_path` |
| `samba` | `host`, `share`, `username`, `password`, `workgroup`, `base_path` (requires `samba` feature) |

## Deployment

### Docker Compose (Local/Dev)

```bash
docker-compose up --build
```

### Cloud Deployment (Hetzner / DigitalOcean)

**Option A: Cloud-Init (automated)**

1. Edit `deploy/cloud-init.yml` with your GHCR credentials and image repo
2. Create a server via Hetzner Cloud Console or DigitalOcean, paste cloud-init YAML as user-data
3. Server automatically installs Docker, pulls image, and starts the service

**Option B: Deploy script**

```bash
# Standalone deploy
IMAGE_REPO=ghcr.io/yourorg/innovare-storage:latest ./deploy/deploy.sh

# Join existing cluster
IMAGE_REPO=ghcr.io/yourorg/innovare-storage:latest ./deploy/deploy.sh --join 10.0.0.1

# Rolling update
./deploy/deploy.sh --update
```

### CI/CD

GitHub Actions workflow (`.github/workflows/build-push.yml`) triggers on push to `main`:
- Builds Docker image (multi-stage: frontend + backend)
- Pushes to GHCR with commit SHA and `latest` tags

See [deploy/README-deploy.md](deploy/README-deploy.md) for the full deployment guide including TLS configuration and monitoring.

## Project Structure

```
src/
├── api/            # HTTP route handlers (auth, files, projects, storages, bulk)
│   ├── auth.rs     # JWT auth middleware (AuthenticatedUser extractor)
│   └── auth_routes.rs  # Auth endpoints (register, login, refresh, logout)
├── db/             # Database models and CRUD operations
├── services/       # Business logic (file_service, bulk_service, tier_service, auth_service)
│   └── auth_service.rs  # JWT/password hashing service
├── storage/        # Storage backend implementations (s3, azure, gcs, ftp, sftp, samba, hetzner, local)
├── workers/        # Background workers (sync_worker, tier_worker)
├── config.rs       # Configuration loading (TOML + env vars)
├── error.rs        # AppError type with HTTP status mapping
├── lib.rs          # App state, health check, Actix-Web configuration
└── main.rs         # Server startup, worker spawning, graceful shutdown

frontend/
├── src/
│   ├── api/        # API client (axios) and TypeScript types
│   ├── components/ # Reusable components (Layout, Sidebar, StorageForm)
│   ├── contexts/   # React contexts (AuthContext)
│   └── pages/      # Page components (Dashboard, Projects, ProjectDetail, Storages, StorageDetail, Users, UserDetail, Nodes, SyncTasks, Login)
├── package.json
├── vite.config.ts
└── tailwind.config.js

migrations/         # SQL schema migrations
docker/             # nginx.conf
deploy/             # deploy.sh, cloud-init.yml, README-deploy.md
```

## License

All rights reserved.
