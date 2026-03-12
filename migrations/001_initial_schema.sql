-- Initial schema for Innovare Storage distributed file storage system
-- Requires PostgreSQL 14+ and optionally Citus for distributed tables

-- Enable UUID generation
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Projects: logical groupings for files
CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(255) NOT NULL UNIQUE,
    hot_to_cold_days INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Storages: backend storage configurations
CREATE TABLE storages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    storage_type VARCHAR(50) NOT NULL,
    config JSONB NOT NULL DEFAULT '{}',
    is_hot BOOLEAN NOT NULL DEFAULT TRUE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Files: deduplicated file records keyed by SHA-256 hash
CREATE TABLE files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    hash_sha256 CHAR(64) NOT NULL UNIQUE,
    size BIGINT NOT NULL,
    content_type VARCHAR(255) NOT NULL DEFAULT 'application/octet-stream',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- File references: links files to projects with original filenames
CREATE TABLE file_references (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    original_name VARCHAR(1024) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(file_id, project_id, original_name)
);

-- File locations: tracks which storages hold a copy of each file
CREATE TABLE file_locations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    storage_id UUID NOT NULL REFERENCES storages(id) ON DELETE CASCADE,
    storage_path VARCHAR(2048) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    synced_at TIMESTAMPTZ,
    last_accessed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(file_id, storage_id)
);

-- Sync tasks: queue for background file synchronization between storages
CREATE TABLE sync_tasks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    source_storage_id UUID NOT NULL REFERENCES storages(id) ON DELETE CASCADE,
    target_storage_id UUID NOT NULL REFERENCES storages(id) ON DELETE CASCADE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    retries INT NOT NULL DEFAULT 0,
    error_msg TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE INDEX idx_storages_project_id ON storages(project_id);
CREATE INDEX idx_storages_enabled ON storages(enabled) WHERE enabled = TRUE;
CREATE INDEX idx_file_references_file_id ON file_references(file_id);
CREATE INDEX idx_file_references_project_id ON file_references(project_id);
CREATE INDEX idx_file_locations_file_id ON file_locations(file_id);
CREATE INDEX idx_file_locations_storage_id ON file_locations(storage_id);
CREATE INDEX idx_file_locations_status ON file_locations(status);
CREATE INDEX idx_sync_tasks_status ON sync_tasks(status);
CREATE INDEX idx_sync_tasks_file_id ON sync_tasks(file_id);

-- Updated_at trigger function
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_projects_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_storages_updated_at
    BEFORE UPDATE ON storages
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_sync_tasks_updated_at
    BEFORE UPDATE ON sync_tasks
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Citus distribution (only runs if Citus extension is available)
-- These statements will be executed separately and errors are non-fatal
-- when running on plain PostgreSQL.
--
-- To apply on a Citus cluster, run these manually:
--   SELECT create_distributed_table('files', 'id');
--   SELECT create_distributed_table('file_locations', 'file_id');
--   SELECT create_distributed_table('file_references', 'project_id');
