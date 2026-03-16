-- Clean slate: purge all file data so we can start fresh with container-per-project design.
-- The new behavior: container/bucket defaults to project slug when no container_override is set.

DELETE FROM sync_tasks;
DELETE FROM shared_links;
DELETE FROM file_locations;
DELETE FROM file_references;
DELETE FROM files;
