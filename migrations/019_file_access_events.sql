-- Append-only log of file access events used by the admin dashboard for
-- "accesses over time" charts and top-accessed listings. Insertions happen
-- best-effort from the download handlers (not on the latency-critical path).

CREATE TABLE file_access_events (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id      UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id   UUID,                          -- nullable: anonymous/shared-link access
    storage_id   UUID REFERENCES storages(id),  -- nullable: storage may not be known
    access_kind  VARCHAR(32) NOT NULL,          -- 'download' | 'shared_link_download' | 'bulk_download'
    user_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    bytes        BIGINT,                        -- denormalised file size at access time
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_file_access_events_created_at ON file_access_events(created_at DESC);
CREATE INDEX idx_file_access_events_file_id_created_at ON file_access_events(file_id, created_at DESC);
CREATE INDEX idx_file_access_events_project_id_created_at
    ON file_access_events(project_id, created_at DESC) WHERE project_id IS NOT NULL;
CREATE INDEX idx_file_access_events_storage_id_created_at
    ON file_access_events(storage_id, created_at DESC) WHERE storage_id IS NOT NULL;

-- On a Citus cluster, distribute by file_id to co-locate with files / file_locations:
--   SELECT create_distributed_table('file_access_events', 'file_id');
