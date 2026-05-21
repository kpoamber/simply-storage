-- Resumable (tus-protocol) chunked uploads: track in-progress upload sessions.
-- Chunks are assembled on the shared local temp volume; this table records
-- progress so any app replica can serve subsequent chunks and finalize.
-- Coordinator-local (not Citus-distributed): rows are transient and low-volume.

CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    original_name VARCHAR(1024) NOT NULL,
    content_type VARCHAR(255) NOT NULL DEFAULT 'application/octet-stream',
    total_size BIGINT NOT NULL,
    offset_bytes BIGINT NOT NULL DEFAULT 0,
    temp_path TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}',
    status VARCHAR(20) NOT NULL DEFAULT 'in_progress', -- in_progress | completed | aborted
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_upload_sessions_status ON upload_sessions(status);
CREATE INDEX idx_upload_sessions_expires_at ON upload_sessions(expires_at);
CREATE INDEX idx_upload_sessions_project_id ON upload_sessions(project_id);
