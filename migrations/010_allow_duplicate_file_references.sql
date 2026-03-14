-- Allow multiple file_references for the same file+project+name (different metadata/uploads)
ALTER TABLE file_references DROP CONSTRAINT IF EXISTS file_references_file_id_project_id_original_name_key;
