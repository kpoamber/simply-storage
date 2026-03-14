# Proxy-Based Shared Links with View Statistics

## Overview

Replace direct-to-storage link generation with a proxy-based shared link system. The service generates unique tokens for file access, acts as a proxy between clients and storage backends, tracks view statistics, and supports both public and password-protected links with optional expiration. Password-protected links authenticate through a UI form, not through URL parameters.

## Context

- Files involved:
  - `migrations/` - new migration for shared_links table
  - `src/db/models.rs` - SharedLink model and CRUD queries
  - `src/services/` - new SharedLinkService
  - `src/api/` - new shared_links API module + public proxy endpoint
  - `src/api/mod.rs` - route registration
  - `src/lib.rs` - AppState updates
  - `frontend/src/pages/` - shared link management UI + public access page
- Related patterns: existing FileService download flow, HMAC-signed local temp links, AuthenticatedUser extractor, argon2 password hashing from AuthService
- Dependencies: no new external crates needed (argon2, rand already in use)

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Database Migration

**Files:**
- Create: `migrations/011_shared_links.sql`

- [x] Create `shared_links` table:
  - `id` UUID PRIMARY KEY
  - `token` VARCHAR(32) NOT NULL UNIQUE - short random URL-safe token
  - `file_id` UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE
  - `project_id` UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE
  - `original_name` VARCHAR(1024) NOT NULL - cached filename for display
  - `created_by` UUID NOT NULL REFERENCES users(id)
  - `password_hash` VARCHAR(255) - NULL means public link
  - `expires_at` TIMESTAMPTZ - NULL means no expiration
  - `max_downloads` INTEGER - NULL means unlimited
  - `download_count` BIGINT NOT NULL DEFAULT 0
  - `last_accessed_at` TIMESTAMPTZ
  - `is_active` BOOLEAN NOT NULL DEFAULT TRUE
  - `created_at` TIMESTAMPTZ NOT NULL DEFAULT NOW()
- [x] Add indexes: token (unique), file_id, project_id, created_by
- [x] Write tests: verify migration applies cleanly

### Task 2: SharedLink Model and Database Queries

**Files:**
- Modify: `src/db/models.rs`

- [x] Add `SharedLink` struct with sqlx::FromRow derive
- [x] Add `CreateSharedLink` input struct (file_id, project_id, original_name, created_by, password (plain), expires_at, max_downloads)
- [x] Implement `SharedLink::create()` - insert new shared link with generated token and optional argon2 password hash
- [x] Implement `SharedLink::find_by_token()` - lookup by token
- [x] Implement `SharedLink::find_by_id()` - lookup by UUID
- [x] Implement `SharedLink::list_by_project()` - list all links for a project
- [x] Implement `SharedLink::list_by_user()` - list all links created by a user
- [x] Implement `SharedLink::increment_download_count()` - atomic increment + update last_accessed_at
- [x] Implement `SharedLink::deactivate()` - set is_active = false
- [x] Implement `SharedLink::delete()` - hard delete
- [x] Implement token generation: 22-character URL-safe random string (base62 or base64url)
- [x] Write tests for all CRUD operations

### Task 3: SharedLinkService

**Files:**
- Create: `src/services/shared_link_service.rs`
- Modify: `src/services/mod.rs`

- [x] Create SharedLinkService struct holding PgPool and storage backends reference
- [x] `create_link()` - validate file exists and user has access to the project, create SharedLink with optional password hash and expiration
- [x] `get_link_info()` - fetch link by token, check expiration and active status, return public info (file_name, file_size, content_type, password_required flag, expires_at). Returns 404 if expired/inactive
- [x] `verify_password()` - validate password for a password-protected link by token, return success/failure
- [x] `download_via_link()` - validate token, check expiration, check max_downloads, find file locations, download from first available backend, increment stats, return file data with content headers. For password-protected links, requires a separate prior password verification step (session-based or short-lived download token)
- [x] `list_links()` - list links for a project with download stats
- [x] `deactivate_link()` - deactivate a link (only creator or admin)
- [x] `delete_link()` - delete a link (only creator or admin)
- [x] Write tests for service logic (validation, expiration, password verification, stats tracking)

### Task 4: API Endpoints

**Files:**
- Create: `src/api/shared_links.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/lib.rs`

- [x] Add SharedLinkService to AppState
- [x] Authenticated management endpoints (under /api):
  - `POST /api/projects/{project_id}/shared-links` - create shared link (body: file_id, password?, expires_in_seconds?, max_downloads?)
  - `GET /api/projects/{project_id}/shared-links` - list links for project with stats
  - `GET /api/shared-links/{id}` - get link details + stats
  - `DELETE /api/shared-links/{id}` - deactivate link
- [x] Public proxy endpoints (no auth required):
  - `GET /s/{token}` - returns JSON with link info (file_name, file_size, content_type, password_required, expires_at). Returns 404 if expired/inactive
  - `POST /s/{token}/verify` - accepts JSON body `{"password": "..."}`, verifies password for protected link. On success returns a short-lived download token (e.g. JWT with 5-minute TTL tied to this shared link). Returns 403 on wrong password
  - `GET /s/{token}/download?dl_token={token}` - proxy downloads file from storage. For public links dl_token is not required. For password-protected links, requires valid dl_token from prior /verify call. Validates expiration, max_downloads. Streams file with correct Content-Type and Content-Disposition headers. Increments download_count
- [x] Register routes in configure_api_routes and at app root level for /s/ prefix
- [x] Write tests for all endpoints (public access, password verification flow, expiration, stats)

### Task 5: Frontend - Shared Link Management

**Files:**
- Create: `frontend/src/pages/SharedLinks.tsx` - project shared links management page
- Create: `frontend/src/pages/SharedLinkAccess.tsx` - public page for accessing shared links
- Modify: `frontend/src/App.tsx` - add routes

- [x] SharedLinks page: table of shared links for a project with columns (file name, token/URL, created, expires, downloads, status, actions)
- [x] "Create Shared Link" dialog: select file from project, optional password, optional expiration duration, optional max downloads
- [x] Copy-to-clipboard button for generated link URL
- [x] Deactivate/delete actions per link
- [x] SharedLinkAccess page (public route /share/{token}):
  - Shows file info (name, size, type)
  - If link is expired or inactive: shows "link expired/unavailable" message, no download
  - If link is public: shows download button directly
  - If link is password-protected: shows password input form with submit button. On submit, calls POST /s/{token}/verify with the entered password. On success, receives download token and initiates download via /s/{token}/download?dl_token=... On wrong password, shows error message and lets user retry
- [x] Add navigation entry for shared links in project context
- [x] Write tests for components

### Task 6: Verify Acceptance Criteria

- [x] Manual test: create public shared link, access and download file via proxy
- [x] Manual test: create password-protected link, verify UI shows password form, enter password, download file
- [x] Manual test: create password-protected link, verify wrong password is rejected with error message
- [x] Manual test: create link with expiration, verify access denied after expiry
- [x] Manual test: verify download counter increments on each access
- [x] Manual test: verify client never receives direct storage URLs
- [x] Run full test suite: `cargo test` and `cd frontend && npm test`
- [x] Run linters: `cargo clippy -- -D warnings` and `cd frontend && npm run lint`

### Task 7: Update Documentation

- [x] Update CLAUDE.md with shared links patterns and new file descriptions
- [x] Move this plan to `docs/plans/completed/`
