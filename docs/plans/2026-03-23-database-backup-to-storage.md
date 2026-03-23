# Database Backup to Storage Integration

## Overview

Добавить в приложение функциональность автоматического создания бэкапов базы данных (pg_dump) по расписанию с загрузкой на выбранный storage backend. Включает UI для настройки расписания, выбора storage, просмотра истории бэкапов и ручного запуска.

## Context

- Files involved: migrations/, src/db/models.rs, src/services/, src/workers/, src/api/, src/config.rs, src/main.rs, src/lib.rs, frontend/src/pages/, frontend/src/App.tsx, frontend/src/components/Sidebar.tsx
- Related patterns: TierWorker (periodic background worker), StorageBackend trait (upload), SharedLinkService (service pattern), storages.rs API (admin CRUD pattern)
- Dependencies: `cron` crate (cron expression parsing), `tokio::process::Command` (pg_dump execution)

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- Follow existing project patterns (sqlx query_as, AppError, AdminUser extractor, React Query)
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Database migrations for backup_configs and backup_history

**Files:**
- Create: `migrations/016_backup_config.sql`

- [ ] Create `backup_configs` table: id (UUID PK), name (VARCHAR), storage_id (UUID FK -> storages), storage_path (VARCHAR, prefix path in storage), schedule_cron (VARCHAR, cron expression), retention_count (INT, how many backups to keep), enabled (BOOLEAN), created_at/updated_at (TIMESTAMPTZ)
- [ ] Create `backup_history` table: id (UUID PK), config_id (UUID FK -> backup_configs, nullable for manual backups), storage_id (UUID FK -> storages), file_path (VARCHAR, path in storage), file_size_bytes (BIGINT), status (VARCHAR: pending/running/completed/failed), error_message (TEXT nullable), started_at/completed_at (TIMESTAMPTZ nullable), created_at (TIMESTAMPTZ)
- [ ] Add indexes: backup_configs(storage_id), backup_configs(enabled), backup_history(config_id), backup_history(status), backup_history(created_at DESC)
- [ ] Verify migration runs successfully with `cargo build`

### Task 2: Database models and CRUD

**Files:**
- Modify: `src/db/models.rs`

- [ ] Add `BackupConfig` struct with FromRow, Serialize, Deserialize: id, name, storage_id, storage_path, schedule_cron, retention_count, enabled, created_at, updated_at
- [ ] Add `CreateBackupConfig` and `UpdateBackupConfig` input structs
- [ ] Implement CRUD methods: create, find_by_id, list, update, delete
- [ ] Add `list_enabled` method returning only enabled configs
- [ ] Add `BackupRecord` struct with FromRow: id, config_id, storage_id, file_path, file_size_bytes, status, error_message, started_at, completed_at, created_at
- [ ] Add `CreateBackupRecord` input struct
- [ ] Implement BackupRecord methods: create, find_by_id, list (with optional config_id filter, ordered by created_at DESC), update_status (status + error_message + completed_at), delete, count_by_config_id
- [ ] Add `list_oldest_completed_by_config` method for retention cleanup (returns completed backups beyond retention_count)
- [ ] Write unit tests for model validation logic
- [ ] Run `cargo test`

### Task 3: BackupService - business logic

**Files:**
- Create: `src/services/backup_service.rs`
- Modify: `src/services/mod.rs`

- [ ] Create `BackupService` struct with pool (PgPool), registry (Arc<StorageRegistry>), database_url (String)
- [ ] Implement `create_backup(config_id: Option<Uuid>, storage_id: Uuid, storage_path: &str)` method:
  - Create BackupRecord with status "running"
  - Execute pg_dump via `tokio::process::Command` using database URL from config, output to temp file
  - Compress with gzip (via `flate2` or shell command)
  - Generate filename: `backup_YYYYMMDD_HHMMSS.sql.gz`
  - Upload to storage backend via registry using `storage_path/filename`
  - Update BackupRecord with completed status, file_size, file_path
  - On failure: update BackupRecord with failed status and error_message
- [ ] Implement `cleanup_old_backups(config_id: Uuid, retention_count: i32)` method:
  - Query backups beyond retention count
  - Delete from storage backend
  - Delete BackupRecord entries
- [ ] Implement `delete_backup(backup_id: Uuid)` method: delete file from storage, then delete record
- [ ] Implement `get_next_run_time(cron_expr: &str) -> Option<DateTime>` helper using cron crate
- [ ] Add BackupService to services module
- [ ] Write tests for service logic (mock storage operations)
- [ ] Run `cargo test`

### Task 4: BackupWorker - scheduled background worker

**Files:**
- Create: `src/workers/backup_worker.rs`
- Modify: `src/workers/mod.rs`
- Modify: `src/main.rs`

- [ ] Create `BackupWorker` struct with backup_service (BackupService), pool (PgPool), cancel_token (CancellationToken), check_interval (Duration, default 60s)
- [ ] Implement `spawn()` method following TierWorker pattern
- [ ] Implement `run()` loop: every check_interval, query enabled BackupConfigs, check if each config's cron schedule indicates it's time to run (compare with last backup's started_at), trigger backup if due
- [ ] After each successful backup, call cleanup_old_backups for retention
- [ ] Add BackupWorker to workers module
- [ ] Spawn BackupWorker in main.rs with cancel_token, add handle to shutdown sequence
- [ ] Register BackupService in AppState (web::Data) for API access
- [ ] Write tests for schedule checking logic
- [ ] Run `cargo test`

### Task 5: API routes for backup management

**Files:**
- Create: `src/api/backups.rs`
- Modify: `src/api/mod.rs`

- [ ] Create backup config endpoints (all require AdminUser):
  - `GET /api/backup-configs` - list all backup configs (join with storage name for display)
  - `POST /api/backup-configs` - create config (validate cron expression, validate storage_id exists)
  - `PUT /api/backup-configs/{id}` - update config
  - `DELETE /api/backup-configs/{id}` - delete config (and optionally associated backups)
- [ ] Create backup history endpoints (all require AdminUser):
  - `GET /api/backups` - list backup history (with optional config_id query filter, paginated)
  - `POST /api/backups/trigger` - trigger manual backup (accepts storage_id, storage_path in body; or config_id to use config settings)
  - `DELETE /api/backups/{id}` - delete specific backup (from storage and DB)
- [ ] Register routes in api/mod.rs configure_api_routes
- [ ] Write integration tests for API endpoints
- [ ] Run `cargo test`

### Task 6: Frontend - Backup management page

**Files:**
- Create: `frontend/src/pages/Backups.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/Sidebar.tsx`

- [ ] Create Backups.tsx page with two sections/tabs: Configuration and History
- [ ] Configuration section: table of backup configs (name, storage, schedule, retention, enabled toggle), create/edit modal with form fields (name, storage selector dropdown, storage_path, cron expression input with presets like "daily at 2am", retention_count, enabled), delete button with confirmation
- [ ] History section: table of backup records (date, config name, storage, file size, status with color coding, duration), manual backup trigger button, delete button for individual backups
- [ ] Add "Backups" link to Sidebar.tsx (admin only, with database/archive icon)
- [ ] Add route in App.tsx wrapped in AdminRoute
- [ ] Use React Query for data fetching (useQuery/useMutation pattern matching existing pages)
- [ ] Run `cd frontend && npm run lint && npm run build`

### Task 7: Configuration and documentation

**Files:**
- Modify: `src/config.rs`
- Modify: `CLAUDE.md`

- [ ] Add `BackupConfig` section to AppConfig with fields: enabled (bool, default true), check_interval_secs (u64, default 60), temp_dir (String, default "/tmp")
- [ ] Add env var support: APP_BACKUP__ENABLED, APP_BACKUP__CHECK_INTERVAL_SECS, APP_BACKUP__TEMP_DIR
- [ ] Update CLAUDE.md with new files, patterns, env vars, and table descriptions
- [ ] Run `cargo test`

### Task 8: Verify acceptance criteria

- [ ] Run full backend test suite: `cargo test`
- [ ] Run clippy: `cargo clippy -- -D warnings`
- [ ] Run frontend lint: `cd frontend && npm run lint`
- [ ] Run frontend build: `cd frontend && npm run build`

### Task 9: Update documentation

- [ ] Update CLAUDE.md with backup-related entries (files, tables, env vars, patterns)
- [ ] Move this plan to `docs/plans/completed/`
