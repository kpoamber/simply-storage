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
- `src/api/auth_routes.rs` - Auth API endpoints (register, login, refresh, me, logout)
- `src/db/models.rs` - Database models with sqlx FromRow, all CRUD functions
- `src/services/` - Business logic layer (FileService, BulkService, TierService, AuthService)
- `src/services/auth_service.rs` - AuthService (JWT token generation/validation, argon2 password hashing)
- `src/storage/` - StorageBackend trait implementations, one file per backend type
- `src/storage/registry.rs` - Factory that instantiates backends from storage_type + JSON config
- `src/workers/` - Background tokio tasks (SyncWorker, TierWorker, heartbeat)
- `src/config.rs` - AppConfig loaded from config/default.toml + APP_ env vars
- `src/error.rs` - AppError enum with thiserror, implements actix-web ResponseError
- `src/lib.rs` - AppState struct, app configuration, health check endpoint
- `src/main.rs` - Server startup, migration, worker spawning, graceful shutdown
- `frontend/src/` - React admin dashboard
- `frontend/src/contexts/AuthContext.tsx` - Auth context (token storage, login/logout, auto-refresh)
- `frontend/src/pages/Login.tsx` - Login and registration page
- `migrations/` - SQL migrations (run automatically on startup)

## Code Patterns

- Storage backends implement `#[async_trait] StorageBackend` trait
- Content-addressable storage paths: `ab/cd/abcdef1234...` (first 2 + next 2 chars of SHA-256)
- Distributed locking via PostgreSQL advisory locks (`pg_try_advisory_xact_lock`)
- Graceful shutdown via `tokio_util::sync::CancellationToken`
- Configuration: serde defaults for all fields, TOML file optional, env vars override
- Error handling: `AppError` maps to HTTP status codes via `ResponseError` trait
- API responses: JSON with serde Serialize
- File uploads: `actix-multipart` with streaming
- Authentication: JWT access tokens (Bearer header) + refresh tokens, argon2 password hashing
- Authorization: `AuthenticatedUser` extractor from request, role-based (admin/user) with owner checks

## Database

- Tables: projects, storages, files, file_references, file_locations, sync_tasks, nodes, users, refresh_tokens
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

## Optional Features

- `samba` - Enables Samba/SMB storage backend (requires pavao crate)
  Build with: `cargo build --features samba`
