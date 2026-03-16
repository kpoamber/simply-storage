# Innovare Storage - Project Conventions

## Language & Framework

- Rust 2021 edition, Actix-Web 4, Tokio async runtime
- Frontend: React 18 + TypeScript + Vite + Tailwind CSS
- Database: PostgreSQL + Citus (distributed), accessed via sqlx with compile-time checked queries

## Build & Test Commands

```bash
# Backend
cargo build                        # Build
cargo test                         # Run tests
cargo clippy -- -D warnings        # Lint (treat warnings as errors)

# Frontend
cd frontend && npm install         # Install deps
cd frontend && npm run build       # Build to frontend/dist/
cd frontend && npm test            # Run tests (vitest)
cd frontend && npm run lint        # Lint (eslint)

# Docker
docker-compose up --build          # Full stack
docker-compose up --build --scale app=3  # Scale app instances
```

## Project Structure

- `src/api/` - HTTP route handlers, each module registers routes via `web::scope`
- `src/api/auth.rs` - AuthenticatedUser extractor (JWT auth middleware via FromRequest)
- `src/api/auth_routes.rs` - Auth API endpoints (login, refresh, me, logout, admin user CRUD, user detail with assignments)
- `src/api/shared_links.rs` - Shared link management API + public proxy endpoints for file access via token
- `src/db/models.rs` - Database models with sqlx FromRow, all CRUD functions; includes MetadataFilter DSL compiler, search_by_metadata/search_summary queries, bulk delete queries, SharedLink model and CRUD
- `src/services/` - Business logic layer (FileService, BulkService, TierService, AuthService, SharedLinkService)
- `src/services/auth_service.rs` - AuthService (JWT token generation/validation, argon2 password hashing)
- `src/services/shared_link_service.rs` - SharedLinkService (proxy-based file sharing with optional password protection, expiration, download limits, view stats)
- `src/storage/` - StorageBackend trait implementations, one file per backend type
- `src/storage/registry.rs` - Factory that instantiates backends from storage_type + JSON config
- `src/workers/` - Background tokio tasks (SyncWorker, TierWorker, heartbeat)
- `src/config.rs` - AppConfig loaded from config/default.toml + APP_ env vars
- `src/error.rs` - AppError enum with thiserror, implements actix-web ResponseError
- `src/lib.rs` - AppState struct, app configuration, health check endpoint
- `src/main.rs` - Server startup, migration, admin seeding, worker spawning, graceful shutdown
- `frontend/src/` - React admin dashboard
- `frontend/src/contexts/AuthContext.tsx` - Auth context (token storage, login/logout, auto-refresh)
- `frontend/src/pages/Login.tsx` - Login page (no public registration)
- `frontend/src/pages/Users.tsx` - Admin user management page (list, create, delete users)
- `frontend/src/pages/UserDetail.tsx` - User detail with role/password editing and project/storage assignment management
- `frontend/src/pages/ProjectSearch.tsx` - Search page with metadata query builder (AND/OR/NOT filters), results table, and recharts summary charts
- `frontend/src/pages/ProjectBulkDelete.tsx` - Bulk deletion UI with filter form, preview, confirmation dialog, and result display
- `frontend/src/pages/SharedLinks.tsx` - Project shared links management (create, list, copy URL, deactivate/delete)
- `frontend/src/pages/SharedLinkAccess.tsx` - Public page for accessing shared links (file info, password form, download)
- `migrations/` - SQL migrations (run automatically on startup)

## Code Patterns

- Storage backends implement `#[async_trait] StorageBackend` trait
- Content-addressable storage paths: `ab/cd/abcdef1234...` (first 2 + next 2 chars of SHA-256)
- Distributed locking via PostgreSQL advisory locks (`pg_try_advisory_xact_lock`)
- Graceful shutdown via `tokio_util::sync::CancellationToken`
- Configuration: serde defaults for all fields, TOML file optional, env vars override
- Error handling: `AppError` maps to HTTP status codes via `ResponseError` trait
- API responses: JSON with serde Serialize
- File uploads: `actix-multipart` with streaming; metadata accepted as JSON string field, validated as flat key/value object
- Metadata search: POST /projects/{project_id}/files/search with recursive AND/OR/NOT filter DSL compiled to parameterized SQL using JSONB `@>` operator
- Bulk deletion: POST /projects/{project_id}/files/bulk-delete with metadata filters, date ranges, size ranges; includes preview endpoint and orphan file cleanup
- Authentication: JWT access tokens (Bearer header) + refresh tokens, argon2 password hashing
- Authorization: `AuthenticatedUser` extractor from request, role-based (admin/user) with owner and membership checks
- User-resource assignments: many-to-many via `user_projects` (with role: member/writer) and `user_storages` junction tables; members get read access, writers/owners/admins get write access
- Shared links: proxy-based file sharing via unique tokens. Public endpoints at `/s/{token}` (info, verify password, download). Password-protected links use argon2 hashing and short-lived download tokens (JWT, 5-min TTL). Downloads are proxied through the server - clients never receive direct storage URLs. Supports optional expiration, max download limits, and view statistics

## Database

- Tables: projects, storages, files, file_references, file_locations, sync_tasks, nodes, users, refresh_tokens, user_projects, user_storages, shared_links
- file_references.metadata: JSONB column (default `{}`) with GIN index (jsonb_path_ops) for fast key/value search
- Citus distribution: files and file_locations by file_id, file_references by project_id
- UUIDs as primary keys (uuid v4)
- Timestamps: chrono NaiveDateTime

## Configuration Env Vars

Prefix: `APP_`, separator: `__`
Example: `APP_SERVER__PORT=9090`, `APP_DATABASE__URL=postgres://...`

Auth-related:
- `APP_AUTH__JWT_SECRET` - JWT signing secret (default: `change-me-jwt-secret-in-production`)
- `APP_AUTH__ACCESS_TOKEN_TTL_SECS` - Access token TTL in seconds (default: 900 = 15 min)
- `APP_AUTH__REFRESH_TOKEN_TTL_SECS` - Refresh token TTL in seconds (default: 604800 = 7 days)
- `APP_AUTH__DEFAULT_ADMIN_USERNAME` - Default admin username (default: `admin`)
- `APP_AUTH__DEFAULT_ADMIN_PASSWORD` - Default admin password (default: `Innovare2026!`)

## CI/CD & Deployment

- `.github/workflows/ci.yml` - CI pipeline: backend-checks (clippy, test), frontend-checks (lint, build), docker-build-test; triggered on push/PR
- `.github/workflows/build-push.yml` - Docker image build & push to GHCR; depends on CI passing; semver tagging for `v*` tags; optional auto-deploy trigger
- `.github/workflows/deploy-hetzner.yml` - Deploy to Hetzner Cloud via SSH; inputs: environment (staging/production), profile (small/medium/large)
- `.github/workflows/deploy-windows.yml` - Deploy to Windows Server via SSH; inputs: profile, image_tag
- `.github/workflows/backup.yml` - Database backup (daily 2:00 UTC schedule + manual); supports Hetzner and Windows servers
- `.github/workflows/restore.yml` - Database restore from backup (manual trigger with date or file input)
- `deploy/docker-compose.prod.yml` - Base production compose using GHCR image, with nginx, certbot, named volumes
- `deploy/docker-compose.{small,medium,large}.yml` - Scale profile overrides (1/2/4 app replicas, standalone postgres / Citus 2 workers / Citus 4 workers)
- `deploy/.env.example` - Template for all production environment variables
- `deploy/docker/nginx-prod.conf.template` - Production nginx config template with TLS (uses envsubst for DOMAIN variable)
- `deploy/scripts/deploy.sh` - Hetzner deploy script: GHCR pull, pre-deploy backup, docker compose up with profile, health check, rollback on failure
- `deploy/scripts/deploy-windows.sh` - Windows Server deploy script via SSH with same backup/rollback pattern
- `deploy/scripts/backup.sh` - PostgreSQL/Citus backup: pg_dump for standalone, coordinator+workers for Citus; gzip compression
- `deploy/scripts/backup-cron.sh` - Cron wrapper: logging, retention-based rotation, optional webhook notification
- `deploy/scripts/restore.sh` - Database restore: accepts --file or --date, stops app containers, restores DB, restarts, health check
- `terraform/` - Hetzner Cloud infrastructure as code (hcloud provider): server, firewall, network, backup volume
- `terraform/cloud-init.yml` - Server provisioning: Docker, deploy user, SSH, backup cron, volume mount
- `terraform/tfvars/{small,medium,large}.tfvars` - Server size profiles: cx22/cx32/cx42

## Optional Features

- `samba` - Enables Samba/SMB storage backend (requires pavao crate)
  Build with: `cargo build --features samba`
