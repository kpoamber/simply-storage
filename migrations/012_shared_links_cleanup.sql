-- Fix: remove duplicate unique index on shared_links.token
-- (the UNIQUE constraint on the column already creates an equivalent index)
DROP INDEX IF EXISTS idx_shared_links_token;

-- Fix: add ON DELETE CASCADE to created_by FK for consistency
-- with all other user-referencing foreign keys in the schema
ALTER TABLE shared_links DROP CONSTRAINT IF EXISTS shared_links_created_by_fkey;
ALTER TABLE shared_links ADD CONSTRAINT shared_links_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE CASCADE;
