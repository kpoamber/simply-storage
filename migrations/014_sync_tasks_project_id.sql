ALTER TABLE sync_tasks ADD COLUMN project_id UUID REFERENCES projects(id) ON DELETE SET NULL;

CREATE INDEX idx_sync_tasks_project_id ON sync_tasks(project_id);

-- Backfill existing pending/in_progress tasks from file_references
UPDATE sync_tasks st
SET project_id = (
    SELECT fr.project_id FROM file_references fr
    WHERE fr.file_id = st.file_id
    LIMIT 1
)
WHERE st.project_id IS NULL
  AND st.status IN ('pending', 'in_progress');
