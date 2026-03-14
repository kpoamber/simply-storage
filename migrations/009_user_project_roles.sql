-- Add role column to user_projects for write access control
ALTER TABLE user_projects ADD COLUMN role VARCHAR(16) NOT NULL DEFAULT 'member';
