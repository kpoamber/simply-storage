# Innovare Storage

Distributed multi-storage file management system with content-addressable deduplication, container-per-project isolation, and hot/cold tiering. Built with Rust (Actix-Web) and PostgreSQL + Citus. Includes an admin web UI and multi-node clustering.

## Architecture

```
                     ┌──────────────┐
                     │    Nginx     │ :80/:443
                     │  least_conn  │
                     └──────┬───────┘
                            │
                 ┌──────────┼──────────┐
                 ▼                     ▼
           ┌──────────┐          ┌──────────┐
           │  App (1) │          │  App (2) │   Actix-Web 4 / Tokio
           │          │          │          │   --scale app=N
           └────┬─────┘          └────┬─────┘
                │   Background workers:    │
                │   • 4x SyncWorker        │
                │   • 1x TierWorker        │
                │   • 1x Heartbeat         │
                └───────────┬──────────────┘
                            ▼
              ┌──────────────────────────┐
              │  PostgreSQL + Citus 12.1 │
              │  Coordinator + 2 Workers │
              └──────────────────────────┘
```

### Layers

```
API (Actix routes, JWT extractors)
    ↓
Services (FileService, TierService, BulkService, SharedLinkService, AuthService)
    ↓
Backend Resolver (container-per-project: slug-suffix)
    ↓
DB (sqlx, models.rs)          Storage Trait (8 backends)
    ↓                              ↓
PostgreSQL              S3 / Azure / GCS / Local / Hetzner / FTP / SFTP / Samba
```

Multiple stateless app instances run behind nginx and share state through distributed PostgreSQL (Citus). Each instance runs background sync workers coordinated via PostgreSQL advisory locks, registers itself in a `nodes` table, and sends heartbeats every 30 seconds.

### Key Features

- **Container-per-project isolation** - Each project gets its own bucket/container/folder on shared storages, derived automatically from project slug
- **Content-addressable deduplication** - Files stored by SHA-256 hash with `ab/cd/hash` directory sharding, duplicates detected automatically
- **Multi-backend storage** - S3, Azure Blob, GCS, DigitalOcean Spaces, Hetzner StorageBox, FTP, SFTP, Samba, local disk
- **Automatic file sync** - Background workers distribute files across configured storage backends with project-aware container resolution
- **Hot/cold tiering** - Configurable per-project archival policy based on last access time
- **Multi-storage temp links** - Generate presigned URLs from all storages that support direct links (S3, Azure, GCS)
- **Horizontal scaling** - Add app instances behind nginx; new nodes join via config sync from existing nodes
- **Authentication & authorization** - JWT-based auth with access/refresh tokens, role-based access control (admin/user), project ownership, and user-to-project/storage membership assignments
- **File metadata** - Attach custom JSON key/value metadata on upload, returned with all file listing/detail endpoints
- **Metadata search** - Search files within a project using recursive AND/OR/NOT filter DSL on metadata key/value pairs, with summary statistics and timeline charts
- **Bulk deletion** - Delete files matching metadata filters, date ranges, and size ranges with preview mode and automatic orphan cleanup
- **Proxy-based shared links** - Share files via unique token URLs with optional password protection, expiration, and download limits. Downloads are proxied through the server (clients never see storage URLs). Tracks view/download statistics
- **Sync details UI** - Per-file sync status with storage-level details (path, sync time), force-sync and copy-public-link buttons
- **Sensitive field protection** - Storage credentials preserved during edits; never exposed to frontend
- **Admin dashboard** - React frontend for managing projects, storages, files, and monitoring sync tasks
- **Bulk operations** - Sync-all to a storage, export storage as tar.gz archive

### Container-per-Project

Each project gets an isolated bucket/container/folder on shared storages:

```
Storage "amazon-kpo" (S3, credentials only)
│
├─ Project "katmandu" (assignment id: a3f2b1...)
│  └─ bucket: katmandu-a3f2b1
│
├─ Project "berlin" (assignment id: 7c8e4d...)
│  └─ bucket: berlin-7c8e4d
│
└─ Project "moscow" (container_override: "custom-bucket")
   └─ bucket: custom-bucket
```

Resolution: `container_override` > `{project.slug}-{6 hex from assignment UUID}`

| Storage Type | Container maps to |
|-------------|-------------------|
| S3 / GCS | `bucket` |
| Azure | `container` |
| Local | `path/slug-suffix` |
| Hetzner / FTP / SFTP | `base_path/slug-suffix` |

Containers are auto-created on first upload.

### Data Flow

1. **Upload** - Computes SHA-256, deduplicates, resolves project container via backend_resolver, stores to primary (hot) storage with content-addressed path (`ab/cd/hash`), creates sync tasks for secondary storages (including for duplicate files missing on project storages)
2. **Sync** - SyncWorker picks pending tasks with distributed advisory locks, uses `task.project_id` to resolve correct containers, downloads from source, uploads to target
3. **Tiering** - TierWorker scans files older than `hot_to_cold_days`, creates sync tasks to cold storage
4. **Download** - FileService resolves project backends, finds first available location (prefers hot), streams content, updates `last_accessed_at`

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

All API endpoints require authentication via a JWT access token passed in the `Authorization: Bearer <token>` header, except `/health`, `/api/auth/login`, `/api/auth/refresh`, and `/s/{token}/*` (public shared link access). Storage, bulk, system management, and user management endpoints require `admin` role. Project and file endpoints enforce owner/member/admin access.

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
| POST | `/api/projects/{project_id}/files` | Upload file (multipart/form-data, optional JSON metadata field) |
| GET | `/api/projects/{project_id}/files` | List project files (paginated) |
| GET | `/api/files/{id}` | Get file metadata with locations |
| GET | `/api/files/{id}/download` | Download file |
| GET | `/api/files/{id}/link` | Generate temporary signed download link (`?storage_id=` for specific storage) |
| POST | `/api/files/{id}/sync` | Force sync file to a specific storage (ignores retry limits) |
| DELETE | `/api/files/{id}` | Delete file reference |
| POST | `/api/files/{id}/restore` | Restore file from cold tier |
| POST | `/api/projects/{project_id}/files/search` | Search files by metadata filters (AND/OR/NOT DSL) |
| POST | `/api/projects/{project_id}/files/search/summary` | Get summary stats and timeline for search results |
| POST | `/api/projects/{project_id}/files/bulk-delete/preview` | Preview count/size of files matching bulk delete filters |
| POST | `/api/projects/{project_id}/files/bulk-delete` | Delete file references matching filters with orphan cleanup |

### Shared Links (Authenticated Management)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/projects/{project_id}/shared-links` | Create shared link (optional password, expiration, max downloads) |
| GET | `/api/projects/{project_id}/shared-links` | List shared links for project with stats |
| GET | `/api/shared-links/{id}` | Get shared link details |
| DELETE | `/api/shared-links/{id}` | Deactivate shared link |

### Shared Links (Public Proxy - No Auth)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/s/{token}` | Get shared link info (file name, size, type, password required) |
| POST | `/s/{token}/verify` | Verify password for protected link, returns short-lived download token |
| GET | `/s/{token}/download` | Download file via proxy (dl_token query param required for protected links) |

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

### Storage Backends

| Backend | Protocol | Direct Links | Auto-create Container | Config Fields |
|---------|----------|-------------|----------------------|---------------|
| `s3` | AWS SDK | presigned URL | CreateBucket | `region`, `endpoint_url`*, `access_key_id`, `secret_access_key` |
| `azure` | REST + SharedKey | SAS URL | PUT ?restype=container | `account_name`, `account_key`, `endpoint`* |
| `gcs` | REST + JWT/OAuth | V4 signed URL | POST /storage/v1/b | `client_email`, `private_key_pem`, `token_uri`* |
| `local` | filesystem | HMAC signed URL | mkdir -p | `path` |
| `hetzner` | WebDAV (HTTPS) | no (proxy) | MKCOL | `host`, `username`, `password`, `port`*, `base_path`* |
| `ftp` | FTP | no (proxy) | MKD | `host`, `port`*, `username`, `password`, `base_path`* |
| `sftp` | SSH | no (proxy) | mkdir | `host`, `port`*, `username`, `password`, `base_path`* |
| `samba` | SMB (pavao) | no (proxy) | no | `host`, `share`, `username`, `password` (requires `samba` feature) |

\* = optional field

Bucket/container fields are **not configured at storage level** — they are derived from the project slug when the storage is assigned to a project. Sensitive fields (credentials) are protected during storage edits.

## Database (Citus)

| Table | Distributed by | Purpose |
|-------|---------------|---------|
| `files` | `id` | Deduplicated files (SHA-256) |
| `file_locations` | `file_id` | Storage locations per file (status, synced_at) |
| `file_references` | `project_id` | Project-file links with JSONB metadata |
| `projects` | local | Projects with slug (used as container name) |
| `storages` | local | Backend configurations (credentials in JSONB) |
| `project_storages` | local | Project-storage assignments + container_override |
| `sync_tasks` | local | Sync queue with project_id for container resolution |
| `users` | local | Authentication (argon2 hashed passwords) |
| `shared_links` | local | Public file sharing tokens with limits |
| `nodes` | local | Active app instances (heartbeat) |

## Deployment & Scaling

### Option 1: Single Server (default)

Everything runs on one server via `docker-compose up`. Suitable for up to ~100 users and ~1M files.

```
┌─── Single Server (e.g. Hetzner CX32 / 4 vCPU, 8 GB) ───┐
│                                                           │
│  Nginx :80 → App(1) + App(2) → PostgreSQL + Citus        │
│               ↓                                           │
│         4x SyncWorker + 1x TierWorker + 1x Heartbeat     │
│                                                           │
│  Volumes: postgres_data, app_data                         │
└───────────────────────────────────────────────────────────┘
          ↕              ↕              ↕           ↕
     Amazon S3      Azure Blob      Google CS    Hetzner Box
```

```bash
docker-compose up --build
```

All app instances are stateless — state is stored in PostgreSQL. Sync workers across replicas are coordinated via advisory locks (no double-processing).

### Option 2: Separate Database (medium load)

Move PostgreSQL to a dedicated server with fast SSD and more RAM for caching. App servers remain stateless and can be scaled independently.

```
┌─── Server 1 (App) ────────────┐    ┌─── Server 2 (Database) ─────────┐
│                                │    │                                  │
│  Nginx                         │    │  PostgreSQL Coordinator          │
│  App(1) + App(2) + App(3)     │───→│  Citus Worker 1                  │
│  12x SyncWorker               │    │  Citus Worker 2                  │
│  3x TierWorker                │    │                                  │
│                                │    │  Dedicated SSD, 16+ GB RAM      │
└────────────────────────────────┘    └──────────────────────────────────┘
```

Change `APP_DATABASE__URL` to point to the external DB server. Recommended when:
- Database size exceeds available RAM on a single server
- You need independent backup/maintenance windows for DB
- Query latency becomes a bottleneck

### Option 3: Multi-Node (high load)

Fully distributed setup with dedicated load balancer, multiple app servers, and a Citus cluster. Each app server runs its own pool of sync workers.

```
┌───── Load Balancer ──────┐
│  Nginx / HAProxy         │
│  (or cloud LB)           │
└──────┬──────┬──────┬─────┘
       ↓      ↓      ↓
┌────────┐ ┌────────┐ ┌────────┐
│ App(1) │ │ App(2) │ │ App(3) │   Separate servers
│ 4 sync │ │ 4 sync │ │ 4 sync │   or Kubernetes pods
└────────┘ └────────┘ └────────┘
       ↓      ↓      ↓
┌──────────────────────────────────┐
│  Citus Coordinator               │
│  ├─ Worker 1                     │
│  ├─ Worker 2                     │
│  ├─ Worker 3                     │
│  └─ Worker N (add as needed)     │
└──────────────────────────────────┘
```

```bash
# Scale app instances on a single host
docker-compose up --build --scale app=5

# Or deploy on multiple hosts with shared DB
APP_DATABASE__URL=postgres://user:pass@db-host:5432/innovare_storage \
APP_SYNC__NUM_WORKERS=4 \
docker-compose up --build
```

Advisory locks in PostgreSQL ensure that sync tasks are never processed by multiple workers simultaneously, regardless of how many app instances are running.

### Option 4: Kubernetes

For cloud-native environments, each component maps to K8s resources:

| Component | K8s Resource | Scaling |
|-----------|-------------|---------|
| App | Deployment + HPA | Auto-scale by CPU/request rate |
| Nginx | Ingress Controller | Managed by cloud provider |
| PostgreSQL | StatefulSet or managed DB (CloudSQL, RDS) | Vertical / read replicas |
| Citus Workers | StatefulSet | Add shards for horizontal scale |

App pods are stateless (no persistent volumes needed). Database should use managed services (Cloud SQL, Amazon RDS, Azure Database) for production reliability.

### Scaling Guidelines

| Metric | Option 1 | Option 2 | Option 3+ |
|--------|----------|----------|-----------|
| Files | up to 1M | up to 10M | 10M+ |
| Users | up to 100 | up to 1,000 | 1,000+ |
| Upload throughput | ~100 MB/s | ~500 MB/s | ~N * 500 MB/s |
| Sync workers | 4-8 | 12-24 | N * 4 per node |
| Database size | up to 50 GB | up to 500 GB | 500 GB+ (sharded) |
| Server spec | 4 vCPU, 8 GB | 8 vCPU, 16 GB (each) | Per workload |

**Bottleneck progression:**
1. First bottleneck is usually **database I/O** — move DB to dedicated server (Option 2)
2. Next is **sync throughput** — add app instances with more workers (Option 3)
3. Then **database queries** — add Citus workers for horizontal sharding
4. Finally **storage bandwidth** — add more storage backends or upgrade plans

### Docker Compose

```bash
# Start everything
docker-compose up --build

# Scale app instances
docker-compose up --build --scale app=3
```

Access at `http://localhost`.

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
├── api/            # HTTP route handlers
│   ├── auth.rs     # JWT auth middleware (AuthenticatedUser, AdminUser extractors)
│   ├── auth_routes.rs  # Auth endpoints (login, refresh, logout, user CRUD)
│   ├── files.rs    # Upload, download, temp links, sync details, force sync
│   ├── projects.rs # Project CRUD, storage assignments, members
│   ├── storages.rs # Storage CRUD, container management
│   ├── shared_links.rs  # Shared link management + public proxy endpoints
│   └── mod.rs      # Route registration, system endpoints, pagination
├── db/
│   ├── mod.rs      # Connection pool, migrations, Citus setup
│   └── models.rs   # All models, CRUD, metadata filter DSL compiler
├── services/
│   ├── backend_resolver.rs  # Container-per-project resolution (shared logic)
│   ├── file_service.rs      # Upload/download with dedup, content-addressed paths
│   ├── bulk_service.rs      # Bulk operations, sync-all
│   ├── tier_service.rs      # Hot/cold tiering
│   ├── auth_service.rs      # JWT generation/validation, argon2 hashing
│   └── shared_link_service.rs  # Proxy-based file sharing
├── storage/        # StorageBackend trait + implementations
│   ├── traits.rs   # Trait definition (upload, download, delete, temp_url, containers)
│   ├── registry.rs # Backend factory + runtime registry
│   ├── s3.rs, azure.rs, gcs.rs, local.rs, hetzner.rs, ftp.rs, sftp.rs
│   └── samba.rs    # Optional (--features samba)
├── workers/
│   ├── sync_worker.rs  # Background sync with project-aware container resolution
│   └── tier_worker.rs  # Automatic hot→cold archiving
├── config.rs       # Configuration loading (TOML + env vars)
├── error.rs        # AppError type with HTTP status mapping
├── lib.rs          # AppState, health check, Actix-Web configuration
└── main.rs         # Server startup, worker spawning, graceful shutdown

frontend/src/
├── api/            # API client (axios), TypeScript types
├── components/     # StorageForm (sensitive field protection), Layout
├── contexts/       # AuthContext (token storage, auto-refresh)
└── pages/          # Dashboard, Projects, ProjectDetail (sync details dialog),
                    # ProjectSearch, ProjectBulkDelete, SharedLinks,
                    # SharedLinkAccess, Storages, StorageDetail,
                    # Users, UserDetail, Nodes, SyncTasks, Login

migrations/         # SQL schema migrations (015 files)
docker/             # nginx.conf
deploy/             # deploy.sh, cloud-init.yml
```

## License

All rights reserved.
