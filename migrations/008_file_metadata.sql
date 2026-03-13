-- Add metadata JSONB column to file_references with empty object default
ALTER TABLE file_references
    ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Create GIN index for fast metadata searches (jsonb_path_ops for @> operator)
CREATE INDEX idx_file_references_metadata ON file_references USING GIN (metadata jsonb_path_ops);
