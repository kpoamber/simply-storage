---

# Innovare Storage - Distributed File Storage System

## Overview

High-performance distributed file storage web service in Rust (Actix-Web) with PostgreSQL + Citus. The system receives files via HTTP API, deduplicates by SHA-256 hash, distributes across multiple storage backends (S3, Azure Blob, GCS, DigitalOcean Spaces, Hetzner StorageBox, FTP, SFTP, Samba, local disk), supports hot/cold tiering, and provides temporary download/preview links. Includes an admin web UI for managing projects, storages, and system settings. Multiple service instances run behind a load balancer and share state through distributed PostgreSQL. Deployed to Hetzner/DigitalOcean cloud servers via GitHub Actions with one-click provisioning and automatic config sync from existing nodes.

## Context

- Greenfield project, only README.md exists
- Language: Rust (backend), TypeScript + React (admin frontend)
- Web framework: Actix-Web
- Database: PostgreSQL + Citus (via sqlx with compile-time checked queries)
- Frontend: React + TypeScript + Vite, served as static files from Actix-Web
- Load balancer: Nginx (reverse proxy + load balancing across service instances)
- Deployment: Docker containers on Hetzner/DigitalOcean, CI/CD via GitHub Actions, images via GitHub Container Registry (GHCR)
- Async runtime: Tokio
- Priorities: speed and reliability

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- Use trait-based abstraction for storage backends
- Async-first architecture with Tokio runtime
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Project scaffolding and configuration

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/config.rs`
- Create: `src/error.rs`

- [ ] Initialize Cargo project with dependencies: actix-web, actix-files, tokio, sqlx (postgres), serde, sha2, uuid, chrono, tracing, config, thiserror
- [ ] Set up configuration loading (TOML file + env var overrides) for DB connection, server port, local temp storage path, HMAC secret, number of sync workers
- [ ] Set up tracing/logging with tracing-subscriber
- [ ] Create AppError type with proper HTTP status mapping via actix-web ResponseError
- [ ] Create health check endpoint (`GET /health`) and basic Actix-Web server startup
- [ ] Write tests: config loading, error conversion, health check endpoint

### Task 2: Database schema and migrations

**Files:**
- Create: `migrations/001_initial_schema.sql`
- Create: `src/db/mod.rs`
- Create: `src/db/models.rs`

- [ ] Design and write migration for core tables:
  - `projects` (id UUID PK, name VARCHAR, slug VARCHAR UNIQUE, hot_to_cold_days INT nullable, created_at, updated_at)
  - `storages` (id UUID PK, name VARCHAR, storage_type VARCHAR, config JSONB, is_hot BOOL DEFAULT true, project_id UUID nullable FK, enabled BOOL DEFAULT true, created_at, updated_at)
  - `files` (id UUID PK, hash_sha256 CHAR(64) UNIQUE, size BIGINT, content_type VARCHAR, created_at)
  - `file_references` (id UUID PK, file_id UUID FK, project_id UUID FK, original_name VARCHAR, created_at) UNIQUE(file_id, project_id, original_name)
  - `file_locations` (id UUID PK, file_id UUID FK, storage_id UUID FK, storage_path VARCHAR, status VARCHAR, synced_at TIMESTAMP, last_accessed_at TIMESTAMP, created_at) UNIQUE(file_id, storage_id)
  - `sync_tasks` (id UUID PK, file_id UUID FK, source_storage_id UUID FK, target_storage_id UUID FK, status VARCHAR DEFAULT 'pending', retries INT DEFAULT 0, error_msg TEXT, created_at, updated_at)
- [ ] Configure Citus distribution: shard `files` and `file_locations` by file_id, `file_references` by project_id
- [ ] Create sqlx model structs with FromRow and basic CRUD query functions
- [ ] Set up PgPool connection pool in application state
- [ ] Write tests: migration applies, CRUD operations, unique constraints enforced

### Task 3: Storage abstraction trait and local disk backend

**Files:**
- Create: `src/storage/mod.rs`
- Create: `src/storage/traits.rs`
- Create: `src/storage/local.rs`
- Create: `src/storage/registry.rs`

- [ ] Define `StorageBackend` async trait:
  - `upload(path: &str, data: Bytes) -> Result<()>`
  - `download(path: &str) -> Result<Bytes>`
  - `delete(path: &str) -> Result<()>`
  - `exists(path: &str) -> Result<bool>`
  - `generate_temp_url(path: &str, expires_in: Duration) -> Result<Option<String>>`
  - `list(prefix: &str) -> Result<Vec<String>>`
- [ ] Implement `LocalDiskBackend` with content-addressable storage using hash-based directory structure (`ab/cd/abcdef...`)
- [ ] For local backend temp URLs: generate HMAC-signed tokens with expiry, verified by dedicated download endpoint
- [ ] Create `StorageRegistry` to hold and look up backends by storage ID, with dynamic registration/reload
- [ ] Write tests: upload/download round-trip, delete, exists check, list, temp URL signing and verification

### Task 4: Core file service (upload with deduplication)

**Files:**
- Create: `src/services/mod.rs`
- Create: `src/services/file_service.rs`

- [ ] Implement file upload flow:
  1. Accept bytes, compute SHA-256 hash and determine content_type
  2. Check if hash exists in `files` table (deduplication)
  3. If new file: store to primary storage via backend, insert `files` record
  4. Create `file_references` row linking file to project with original filename
  5. Determine target storages (project-specific + shared), create `file_locations` for primary, create `sync_tasks` for remaining
  6. Return success immediately after primary storage write
- [ ] Implement file download: look up file -> find best available `file_location` (prefer hot) -> stream from storage backend -> update `last_accessed_at`
- [ ] Handle concurrent uploads of same file via DB unique constraint + retry/upsert logic
- [ ] Write tests: upload new file, upload duplicate (same hash returns existing), download, concurrent upload handling

### Task 5: REST API endpoints

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/files.rs`
- Create: `src/api/projects.rs`
- Create: `src/api/storages.rs`

- [ ] Project endpoints:
  - `POST /api/projects` - create project
  - `GET /api/projects` - list projects
  - `GET /api/projects/{id}` - get project with file stats
  - `PUT /api/projects/{id}` - update project settings (including hot_to_cold_days)
  - `DELETE /api/projects/{id}` - soft-delete project
- [ ] File endpoints:
  - `POST /api/projects/{project_id}/files` - upload file (multipart/form-data)
  - `GET /api/projects/{project_id}/files` - list project files (with pagination)
  - `GET /api/files/{id}` - get file metadata with all locations and references
  - `GET /api/files/{id}/download` - download file (stream from backend or redirect to temp URL)
  - `GET /api/files/{id}/link` - get temporary preview/download link with configurable expiry
  - `DELETE /api/files/{id}` - delete file reference from project
- [ ] Storage endpoints:
  - `POST /api/storages` - register new storage backend
  - `GET /api/storages` - list storages with usage stats
  - `GET /api/storages/{id}` - get storage details and stats
  - `PUT /api/storages/{id}` - update storage config
  - `DELETE /api/storages/{id}` - soft-disable storage
- [ ] System endpoints:
  - `GET /api/system/stats` - aggregate stats (total files, storage usage, sync task queue)
  - `GET /api/sync-tasks` - list sync tasks with filtering (status, storage_id)
- [ ] Node config sync endpoint:
  - `GET /api/system/config-export` - export current node config (storages, projects, settings) as JSON for bootstrapping new nodes
- [ ] Write integration tests for all endpoints using actix-web::test

### Task 6: S3-compatible storage backend (AWS S3 + DigitalOcean Spaces)

**Files:**
- Create: `src/storage/s3.rs`

- [ ] Implement `StorageBackend` trait for S3 using `aws-sdk-s3` crate
- [ ] Support configurable: endpoint_url (custom for DO Spaces and other S3-compatible services), region, bucket, prefix, access_key_id/secret_access_key (or IAM role)
- [ ] DigitalOcean Spaces is S3-compatible - same backend with custom endpoint (e.g. `https://ams3.digitaloceanspaces.com`)
- [ ] Implement presigned URL generation via S3 SDK for temp download links
- [ ] Handle multipart upload for files larger than configurable threshold (default 100MB)
- [ ] Write tests with LocalStack or mock (feature-gated integration tests)

### Task 7: Azure Blob and Google Cloud Storage backends

**Files:**
- Create: `src/storage/azure.rs`
- Create: `src/storage/gcs.rs`

- [ ] Implement Azure Blob Storage backend using `azure_storage_blobs` crate with SAS URL generation
- [ ] Implement GCS backend using `google-cloud-storage` crate with signed URL generation
- [ ] Both: configurable container/bucket, prefix, credentials
- [ ] Write tests for both backends (feature-gated integration tests)

### Task 8: FTP, SFTP, and Samba backends

**Files:**
- Create: `src/storage/ftp.rs`
- Create: `src/storage/sftp.rs`
- Create: `src/storage/samba.rs`

- [ ] Implement FTP backend using `suppaftp` crate (async mode)
- [ ] Implement SFTP backend using `russh-sftp` crate
- [ ] Implement Samba/SMB backend using `pavao` crate
- [ ] These backends return `None` from `generate_temp_url()` - temp access is proxied through the web service
- [ ] Write tests for each backend (feature-gated integration tests)

### Task 9: Hetzner StorageBox backend

**Files:**
- Create: `src/storage/hetzner.rs`

- [ ] Implement Hetzner StorageBox backend via WebDAV protocol using `reqwest` with DAV methods (PUT, GET, DELETE, PROPFIND, MKCOL)
- [ ] Support configurable: host (e.g. `uXXXXXX.your-storagebox.de`), username, password, sub-account, port, base_path
- [ ] Auto-create directory structure for content-addressable paths using MKCOL
- [ ] Returns `None` from `generate_temp_url()` - access is proxied through the web service
- [ ] Write tests (feature-gated integration tests)

### Task 10: Background sync worker

**Files:**
- Create: `src/workers/mod.rs`
- Create: `src/workers/sync_worker.rs`

- [ ] Implement background task processor as spawned tokio tasks running alongside the web server
- [ ] Poll `sync_tasks` table for pending tasks using PostgreSQL advisory locks (`pg_try_advisory_xact_lock`) for distributed locking across service instances
- [ ] Sync flow: download from source storage -> upload to target storage -> update `file_locations` status to 'synced'
- [ ] Retry logic with exponential backoff, max retries from config, update error_msg on failure
- [ ] Graceful shutdown via tokio CancellationToken
- [ ] Write tests: task pickup, sync execution, retry on failure, advisory lock prevents double-processing

### Task 11: Hot/cold tier management

**Files:**
- Create: `src/workers/tier_worker.rs`
- Create: `src/services/tier_service.rs`

- [ ] Background worker: periodically scan for files where `last_accessed_at + project.hot_to_cold_days < now()` and file is only on hot storages
- [ ] Archive flow: create sync_task to cold storage -> on completion, optionally delete from hot storage and update file_location status to 'archived'
- [ ] API endpoint: `POST /api/files/{id}/restore` - create sync_task from cold to hot storage, return immediately
- [ ] Ensure `last_accessed_at` is updated on every download and link generation
- [ ] Write tests: auto-archiving detection logic, restore flow, access timestamp updates

### Task 12: Bulk operations

**Files:**
- Create: `src/services/bulk_service.rs`
- Create: `src/api/bulk.rs`

- [ ] `POST /api/storages/{id}/sync-all` - enumerate all files not yet on this storage, create sync_tasks for each
- [ ] `POST /api/storages/{id}/export` - start background job to produce tar.gz archive of all files on the storage
- [ ] `GET /api/storages/{id}/export/status` - poll export job progress (percentage, file count)
- [ ] `GET /api/storages/{id}/export/download` - stream completed archive
- [ ] Write tests for bulk sync task creation and export lifecycle

### Task 13: Admin frontend - project scaffolding and layout

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/tsconfig.json`
- Create: `frontend/vite.config.ts`
- Create: `frontend/index.html`
- Create: `frontend/src/main.tsx`
- Create: `frontend/src/App.tsx`
- Create: `frontend/src/api/client.ts`
- Create: `frontend/src/components/Layout.tsx`
- Create: `frontend/src/components/Sidebar.tsx`

- [ ] Initialize React + TypeScript project with Vite
- [ ] Add dependencies: react-router-dom, @tanstack/react-query, tailwindcss, lucide-react (icons), axios
- [ ] Create API client wrapper (axios instance with base URL configuration)
- [ ] Create app shell layout: sidebar navigation (Dashboard, Projects, Storages, Sync Tasks, Nodes) + main content area
- [ ] Set up routing for all admin pages
- [ ] Configure Vite to build to `frontend/dist/`, configure Actix-Web to serve `frontend/dist/` as static files at root path
- [ ] Write tests: component renders, routing works

### Task 14: Admin frontend - dashboard and project management

**Files:**
- Create: `frontend/src/pages/Dashboard.tsx`
- Create: `frontend/src/pages/Projects.tsx`
- Create: `frontend/src/pages/ProjectDetail.tsx`

- [ ] Dashboard page: total files, total storage used, active sync tasks count, storage health overview, active nodes count (cards/widgets)
- [ ] Projects list page: table with name, file count, storage usage, hot_to_cold_days setting, actions (edit, view)
- [ ] Project detail page: file browser with search/pagination, project settings form (name, slug, hot_to_cold_days), assigned storages
- [ ] File upload UI: drag-and-drop zone or file picker on project detail page
- [ ] File actions: download, get temp link (with copy-to-clipboard), restore from cold, delete
- [ ] Write tests for key interactions

### Task 15: Admin frontend - storage management and sync monitoring

**Files:**
- Create: `frontend/src/pages/Storages.tsx`
- Create: `frontend/src/pages/StorageDetail.tsx`
- Create: `frontend/src/pages/SyncTasks.tsx`
- Create: `frontend/src/components/StorageForm.tsx`

- [ ] Storages list page: table with name, type, hot/cold, enabled status, file count, used space, actions
- [ ] Add/edit storage form: dynamic fields based on storage_type selection (S3: region, bucket, endpoint, keys; Azure: container, connection string; GCS: bucket, service account; FTP/SFTP: host, port, user, password, path; Samba: share, host, user, password; Hetzner StorageBox: host, user, password, path; DigitalOcean Spaces: region, bucket, keys; Local: path)
- [ ] Storage detail page: file list on this storage, usage stats, sync-all button, export button with progress
- [ ] Sync tasks page: table with file info, source/target storage, status, retries, error message, timestamps, filtering by status
- [ ] Write tests for form validation and storage type switching

### Task 16: Docker, CI/CD, and deployment infrastructure

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`
- Create: `docker/nginx.conf`
- Create: `.github/workflows/build-push.yml`
- Create: `deploy/cloud-init.yml`
- Create: `deploy/deploy.sh`
- Create: `deploy/README-deploy.md`

- [ ] Create Dockerfile: multi-stage build (Rust backend compile + frontend build -> minimal runtime image with both)
- [ ] Create nginx.conf: upstream block with multiple app instances, load balancing (least_conn), proxy_pass to app instances, health check via `/health` endpoint, client_max_body_size for large uploads, proxy_read_timeout for long uploads, TLS termination support (mounted certs or Let's Encrypt via certbot)
- [ ] Create docker-compose.yml with services:
  - `nginx` - load balancer (ports 80/443 exposed)
  - `app` (configurable replicas via `deploy.replicas` or `--scale`) - Innovare Storage instances
  - `postgres` - PostgreSQL + Citus coordinator
  - `postgres-worker-1`, `postgres-worker-2` - Citus workers
- [ ] GitHub Actions workflow (`.github/workflows/build-push.yml`):
  - Trigger on push to `main` branch
  - Build Docker image, push to GHCR (GitHub Container Registry) with commit SHA and `latest` tags
  - Use GitHub Secrets for GHCR auth (automatic via `GITHUB_TOKEN`)
- [ ] Cloud-init template (`deploy/cloud-init.yml`): bootstrap script for new Hetzner/DigitalOcean droplets that installs Docker, authenticates to GHCR (using a deploy token stored as user-data or injected via cloud provider metadata), pulls the latest image, and starts the service
- [ ] Deploy script (`deploy/deploy.sh`):
  - Accepts `--join <existing-node-ip>` flag to bootstrap from an existing node
  - When `--join` is used: calls `GET /api/system/config-export` on the existing node to fetch current configuration (DB connection, storage configs, HMAC secret), writes it to local config file, then starts the service
  - When run standalone: starts with local config file
  - Handles pulling latest image from GHCR, stopping old container, starting new one
  - Can be used for both initial deploy and updates (rolling restart)
- [ ] Add `GET /api/system/nodes` endpoint: each node registers itself in DB on startup (node_id, address, started_at, last_heartbeat), background task sends heartbeat every 30s, endpoint returns active nodes list
- [ ] Document one-click deploy workflow in `deploy/README-deploy.md`: create Hetzner/DO server with cloud-init -> server auto-pulls image from GHCR and joins cluster via `--join` flag
- [ ] Write tests: docker build succeeds, config-export endpoint returns valid config, node registration and heartbeat

### Task 17: Verify acceptance criteria

- [ ] Manual test: upload file via API, verify deduplication with second identical upload
- [ ] Manual test: download file via temp link, verify link expiry
- [ ] Manual test: register new S3 storage, run sync-all, verify files appear
- [ ] Manual test: configure hot_to_cold_days on project, verify archival after expiry
- [ ] Manual test: restore archived file
- [ ] Manual test: add DigitalOcean Spaces storage via admin UI, upload file, verify sync
- [ ] Manual test: admin dashboard shows correct stats
- [ ] Manual test: docker-compose up with 2 app instances, verify requests balanced across both
- [ ] Manual test: deploy new node using `deploy.sh --join <existing-ip>`, verify it picks up config and starts serving
- [ ] Run full test suite (`cargo test`)
- [ ] Run linter (`cargo clippy -- -D warnings`)
- [ ] Run frontend tests (`cd frontend && npm test`)
- [ ] Run frontend lint (`cd frontend && npm run lint`)
- [ ] Verify test coverage meets 80%+ (`cargo tarpaulin`)

### Task 18: Update documentation

- [ ] Update README.md with: architecture overview, setup/build instructions, configuration reference, API endpoint documentation, deployment guide (Docker Compose local + Hetzner/DO cloud deploy)
- [ ] Create CLAUDE.md with project conventions and patterns
- [ ] Move this plan to `docs/plans/completed/`
