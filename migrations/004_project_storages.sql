-- Many-to-many junction table for project <-> storage assignments
-- Replaces the one-to-many storages.project_id relationship
CREATE TABLE project_storages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    storage_id UUID NOT NULL REFERENCES storages(id) ON DELETE CASCADE,
    container_override VARCHAR(255),
    prefix_override VARCHAR(1024),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, storage_id)
);

CREATE INDEX idx_project_storages_project_id ON project_storages(project_id);
CREATE INDEX idx_project_storages_storage_id ON project_storages(storage_id);

CREATE TRIGGER update_project_storages_updated_at
    BEFORE UPDATE ON project_storages
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Migrate existing data: storages with project_id become explicit assignments
INSERT INTO project_storages (project_id, storage_id, is_active)
SELECT project_id, id, enabled
FROM storages
WHERE project_id IS NOT NULL
ON CONFLICT (project_id, storage_id) DO NOTHING;

-- Global storages (project_id IS NULL) get assigned to all existing active projects
INSERT INTO project_storages (project_id, storage_id, is_active)
SELECT p.id, s.id, s.enabled
FROM projects p
CROSS JOIN storages s
WHERE s.project_id IS NULL AND s.enabled = TRUE
  AND p.deleted_at IS NULL
ON CONFLICT (project_id, storage_id) DO NOTHING;
