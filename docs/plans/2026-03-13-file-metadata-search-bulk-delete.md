# File Metadata Storage, Indexed Search, and Bulk Deletion

## Overview

Add support for user-defined metadata (JSON key/value) on file uploads, with fast indexed search (AND/OR/NOT filters) within projects, search UI with summary and time-series charts, and bulk deletion by configurable parameters.

## Context

- Files involved:
  - `migrations/` - new migration for metadata column + GIN index
  - `src/db/models.rs` - FileReference model, metadata CRUD, search queries
  - `src/api/files.rs` - upload endpoint (accept metadata), search endpoint, bulk delete endpoint
  - `src/api/mod.rs` - register new routes
  - `src/services/file_service.rs` - upload logic (pass metadata through)
  - `src/error.rs` - new error variants if needed
  - `frontend/src/api/types.ts` - TypeScript types for metadata, search, bulk delete
  - `frontend/src/api/client.ts` - API client functions
  - `frontend/src/pages/ProjectDetail.tsx` - metadata display, upload form changes
  - `frontend/src/pages/ProjectSearch.tsx` (new) - search page with query builder, results, charts
  - `frontend/src/pages/ProjectBulkDelete.tsx` (new) - bulk deletion UI
  - `frontend/src/App.tsx` - new routes
  - `frontend/src/components/Sidebar.tsx` - navigation updates
- Related patterns: JSONB + GIN index on PostgreSQL/Citus for flexible key/value search; file_references distributed by project_id (search stays shard-local); existing pagination pattern via PaginationParams; actix-multipart for uploads
- Dependencies: `serde_json` (already present), `recharts` (new, frontend charting library)

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- Backend tasks first, then frontend
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Database migration and model updates

**Files:**
- Create: `migrations/NNN_file_metadata.sql`
- Modify: `src/db/models.rs`

- [x] Create migration: add `metadata JSONB DEFAULT '{}'::jsonb` column to `file_references` table
- [x] Create GIN index: `CREATE INDEX idx_file_references_metadata ON file_references USING GIN (metadata jsonb_path_ops)`
- [x] Update `FileReference` struct in models.rs: add `metadata: serde_json::Value` field
- [x] Update all `FileReference` queries (insert, select, list) to include the metadata column
- [x] Add `FileReference::update_metadata()` method for updating metadata on an existing reference
- [x] Write tests: verify migration applies cleanly, metadata column defaults to empty object, GIN index exists
- [x] Run project test suite - must pass before task 2

### Task 2: Upload and retrieval API changes

**Files:**
- Modify: `src/api/files.rs`
- Modify: `src/services/file_service.rs`

- [x] Modify upload endpoint to accept a `metadata` field in the multipart form (JSON string, parsed to serde_json::Value)
- [x] Validate metadata is a flat JSON object (keys are strings, values are strings/numbers/booleans); reject nested objects/arrays with 400 error
- [x] Pass metadata through FileService::upload_file() to FileReference creation
- [x] Ensure file list endpoint (`GET /projects/{project_id}/files`) returns metadata in each FileReference response
- [x] Ensure file detail endpoint (`GET /files/{id}`) returns metadata
- [x] Write tests: upload with metadata, upload without metadata (defaults to {}), upload with invalid metadata (nested objects rejected), retrieve file with metadata
- [x] Run project test suite - must pass before task 3

### Task 3: Metadata search API

**Files:**
- Modify: `src/api/files.rs`
- Modify: `src/db/models.rs`

- [x] Design search query DSL as JSON request body:
  ```
  POST /api/projects/{project_id}/files/search
  {
    "filters": { "and": [ {"key": "env", "value": "prod"}, {"not": {"key": "status", "value": "deprecated"}} ] },
    "page": 1,
    "per_page": 50
  }
  ```
  Filter node types: `{"key": "k", "value": "v"}` (leaf), `{"and": [...]}`, `{"or": [...]}`, `{"not": <node>}`
- [x] Implement filter-to-SQL compiler in models.rs: recursively build WHERE clause using `metadata @> '{"key":"value"}'::jsonb` for leaf nodes, AND/OR/NOT for logical operators; use parameterized queries to prevent SQL injection
- [x] Add `FileReference::search_by_metadata()` in models.rs that executes the compiled query with pagination, scoped to project_id
- [x] Register `POST /projects/{project_id}/files/search` route; require read access to project
- [x] Return paginated results with total count (same FileReference shape with metadata included)
- [x] Write tests: search with single key match, AND filter, OR filter, NOT filter, nested AND/OR/NOT, empty filters (return all), no results
- [x] Run project test suite - must pass before task 4

### Task 4: Search summary and aggregation API

**Files:**
- Modify: `src/api/files.rs`
- Modify: `src/db/models.rs`

- [x] Add `POST /api/projects/{project_id}/files/search/summary` endpoint accepting same filter DSL
- [x] Return summary JSON: `{ total_files, total_size, earliest_upload, latest_upload, timeline: [{date, count, size}] }`
- [x] Timeline: aggregate matched files by date (day granularity), returning count and cumulative size per day
- [x] Reuse filter-to-SQL compiler from Task 3 for the WHERE clause
- [x] Implement aggregation query: JOIN file_references with files (for size), GROUP BY date
- [x] Write tests: summary with filters returns correct counts/sizes, empty result summary, timeline ordering
- [x] Run project test suite - must pass before task 5

### Task 5: Bulk deletion API

**Files:**
- Modify: `src/api/files.rs`
- Modify: `src/db/models.rs`
- Modify: `src/services/file_service.rs`

- [ ] Add `POST /api/projects/{project_id}/files/bulk-delete` endpoint (require owner or admin)
- [ ] Accept filter parameters as JSON body:
  ```
  {
    "metadata_filters": { "and": [...] },    // optional, same DSL as search
    "created_before": "2026-01-01T00:00:00", // optional, upload time
    "created_after": "2025-01-01T00:00:00",  // optional
    "size_min": 1048576,                      // optional, bytes
    "size_max": 10485760,                     // optional
    "last_accessed_before": "2025-06-01T00:00:00" // optional
  }
  ```
- [ ] Require at least one filter (reject empty filter request with 400)
- [ ] Add preview mode: `POST /api/projects/{project_id}/files/bulk-delete/preview` returns count and total size of matching files without deleting
- [ ] Implement deletion: delete matching file_references, then clean up orphaned files (files with zero remaining references) and their physical storage files + file_locations
- [ ] Return result: `{ deleted_references: N, orphaned_files_cleaned: M, freed_bytes: X }`
- [ ] Write tests: preview returns correct count, bulk delete removes matching references, orphan cleanup works, empty filter rejected, authorization checked
- [ ] Run project test suite - must pass before task 6

### Task 6: Frontend - metadata on upload and display

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/client.ts`
- Modify: `frontend/src/pages/ProjectDetail.tsx`

- [ ] Add `metadata` field to FileReference type in types.ts
- [ ] Update upload function in client.ts to include metadata as a form field (JSON string)
- [ ] Add metadata key/value input to upload area in ProjectDetail: dynamic rows of key + value inputs with add/remove buttons
- [ ] Display metadata as tags/badges in the file table (collapsed by default, expandable)
- [ ] Add metadata column or expandable row in file list table showing key=value pairs
- [ ] Write tests: metadata input renders, add/remove key-value rows, upload sends metadata, metadata displays in file list
- [ ] Run frontend test suite - must pass before task 7

### Task 7: Frontend - search page with query builder and charts

**Files:**
- Create: `frontend/src/pages/ProjectSearch.tsx`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/client.ts`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/Sidebar.tsx`

- [ ] Install recharts: `npm install recharts`
- [ ] Add search and summary API functions to client.ts
- [ ] Add TypeScript types for search request/response, summary response
- [ ] Create ProjectSearch page with:
  - Query builder UI: rows of (key, value) filter conditions, each with AND/OR/NOT toggle; support adding/removing conditions and grouping
  - Search button that calls search API
  - Results table: file name, size, metadata tags, created_at, sync status (reuse existing file table pattern)
  - Pagination controls
- [ ] Add summary section above results:
  - Total files found, total size (human-readable)
  - Line chart (recharts): file count over time (x=date, y=count)
  - Area chart (recharts): cumulative size over time (x=date, y=bytes)
- [ ] Add route `/projects/:id/search` in App.tsx
- [ ] Add "Search" link in project context navigation (ProjectDetail page tabs or sidebar sub-items)
- [ ] Write tests: query builder renders, add/remove filters works, search triggers API call, results render, charts render with mock data
- [ ] Run frontend test suite - must pass before task 8

### Task 8: Frontend - bulk deletion UI

**Files:**
- Create: `frontend/src/pages/ProjectBulkDelete.tsx`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/client.ts`
- Modify: `frontend/src/App.tsx`

- [ ] Add bulk delete API functions to client.ts (preview + execute)
- [ ] Add TypeScript types for bulk delete request/response
- [ ] Create ProjectBulkDelete page with:
  - Filter form: date range pickers (created_before/after, last_accessed_before), size range inputs (min/max), metadata filter builder (reuse from search page)
  - "Preview" button: shows count and total size of files matching filters
  - "Delete" button: requires confirmation dialog, shows preview count, then executes deletion
  - Result display: shows deleted count, orphans cleaned, freed space
- [ ] Add route `/projects/:id/bulk-delete` in App.tsx (admin/owner only)
- [ ] Add "Bulk Delete" link in project context navigation
- [ ] Write tests: filter form renders, preview triggers API call, confirmation dialog appears, delete executes after confirmation
- [ ] Run frontend test suite - must pass before task 9

### Task 9: Verify acceptance criteria

- [ ] Manual test: upload file with metadata via UI, verify metadata stored and displayed
- [ ] Manual test: search files by metadata key/value with AND/OR/NOT filters, verify correct results
- [ ] Manual test: verify summary shows correct totals, charts render with real data
- [ ] Manual test: bulk delete preview shows correct count, actual delete removes files
- [ ] Run full test suite: `cargo test` (backend) and `cd frontend && npm test` (frontend)
- [ ] Run linter: `cargo clippy -- -D warnings` and `cd frontend && npm run lint`
- [ ] Verify test coverage meets 80%+

### Task 10: Update documentation

- [ ] Update CLAUDE.md: add file_metadata table/column, search endpoint, bulk delete endpoint to project structure
- [ ] Move this plan to `docs/plans/completed/`
