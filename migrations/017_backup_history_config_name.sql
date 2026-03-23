-- Preserve the config name in backup history so it remains visible
-- after the backup_config is deleted (FK ON DELETE SET NULL clears config_id).
ALTER TABLE backup_history ADD COLUMN config_name VARCHAR(255);
