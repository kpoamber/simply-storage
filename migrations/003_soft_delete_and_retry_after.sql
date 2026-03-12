-- Add retry_after column to sync_tasks for exponential backoff
ALTER TABLE sync_tasks ADD COLUMN retry_after TIMESTAMPTZ;

-- Index to efficiently find tasks ready for retry
CREATE INDEX idx_sync_tasks_retry_after ON sync_tasks(retry_after) WHERE status = 'pending';

-- Add deleted_at column to projects for soft-delete support
ALTER TABLE projects ADD COLUMN deleted_at TIMESTAMPTZ;
CREATE INDEX idx_projects_deleted_at ON projects(deleted_at) WHERE deleted_at IS NULL;
