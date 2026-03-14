-- Shared links: proxy-based file sharing with optional password protection and expiration
CREATE TABLE shared_links (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    token VARCHAR(32) NOT NULL UNIQUE,
    file_id UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    original_name VARCHAR(1024) NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id),
    password_hash VARCHAR(255),
    expires_at TIMESTAMPTZ,
    max_downloads INTEGER,
    download_count BIGINT NOT NULL DEFAULT 0,
    last_accessed_at TIMESTAMPTZ,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for common queries
CREATE UNIQUE INDEX idx_shared_links_token ON shared_links(token);
CREATE INDEX idx_shared_links_file_id ON shared_links(file_id);
CREATE INDEX idx_shared_links_project_id ON shared_links(project_id);
CREATE INDEX idx_shared_links_created_by ON shared_links(created_by);
