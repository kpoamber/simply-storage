-- Backup system: scheduled database backups uploaded to storage backends

-- Configuration for automatic backups
CREATE TABLE backup_configs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL,
    storage_id UUID NOT NULL REFERENCES storages(id),
    storage_path VARCHAR(1024) NOT NULL DEFAULT 'backups',
    schedule_cron VARCHAR(100) NOT NULL,
    retention_count INTEGER NOT NULL DEFAULT 7,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- History of backup executions
CREATE TABLE backup_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    config_id UUID REFERENCES backup_configs(id) ON DELETE SET NULL,
    storage_id UUID NOT NULL REFERENCES storages(id),
    file_path VARCHAR(1024) NOT NULL,
    file_size_bytes BIGINT NOT NULL DEFAULT 0,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for backup_configs
CREATE INDEX idx_backup_configs_storage_id ON backup_configs(storage_id);
CREATE INDEX idx_backup_configs_enabled ON backup_configs(enabled);

-- Indexes for backup_history
CREATE INDEX idx_backup_history_config_id ON backup_history(config_id);
CREATE INDEX idx_backup_history_status ON backup_history(status);
CREATE INDEX idx_backup_history_created_at ON backup_history(created_at DESC);
