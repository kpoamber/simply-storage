# User Administration UI with Project/Storage Assignments

## Overview

Add full user management UI to the admin dashboard with many-to-many user-to-project and user-to-storage bindings. This requires new database tables, backend API endpoints, authorization updates, and frontend pages for managing users and their assignments.

## Context

- Files involved:
  - Backend: `migrations/`, `src/db/models.rs`, `src/api/auth_routes.rs`, `src/api/projects.rs`, `src/api/storages.rs`, `src/api/auth.rs`, `src/lib.rs`
  - Frontend: `frontend/src/pages/`, `frontend/src/components/`, `frontend/src/api/types.ts`, `frontend/src/App.tsx`, `frontend/src/components/Sidebar.tsx`
- Related patterns: `project_storages` junction table pattern, `AuthenticatedUser` extractor, React Query mutations with cache invalidation, Tailwind table/form/modal patterns
- Dependencies: No new external dependencies

## Development Approach

- **Testing approach**: Regular (code first, then tests)
- Complete each task fully before moving to the next
- Backend tasks first, then frontend
- **CRITICAL: every task MUST include new/updated tests**
- **CRITICAL: all tests must pass before starting next task**

## Implementation Steps

### Task 1: Database migration and backend models

**Files:**
- Create: `migrations/007_user_assignments.sql`
- Modify: `src/db/models.rs`

- [x] Create migration 007_user_assignments.sql:
  - `user_projects` table: id (UUID PK), user_id (FK users CASCADE), project_id (FK projects CASCADE), created_at (timestamptz DEFAULT now()), UNIQUE(user_id, project_id)
  - `user_storages` table: id (UUID PK), user_id (FK users CASCADE), storage_id (FK storages CASCADE), created_at (timestamptz DEFAULT now()), UNIQUE(user_id, storage_id)
  - Indexes on user_id for both tables
- [x] Add UserProject model to models.rs:
  - Struct with sqlx::FromRow, Serialize
  - `create(pool, user_id, project_id)` - insert, return UserProject
  - `list_for_project(pool, project_id) -> Vec<User>` - join with users table
  - `list_for_user(pool, user_id) -> Vec<Project>` - join with projects table
  - `delete(pool, user_id, project_id)` - remove assignment
  - `is_member(pool, user_id, project_id) -> bool` - check membership
- [x] Add UserStorage model to models.rs:
  - Same pattern as UserProject
  - `create(pool, user_id, storage_id)`
  - `list_for_storage(pool, storage_id) -> Vec<User>` - join with users table
  - `list_for_user(pool, user_id) -> Vec<StorageBackend>` - join with storages table
  - `delete(pool, user_id, storage_id)`
  - `is_member(pool, user_id, storage_id) -> bool`
- [x] Write tests for model CRUD operations
- [x] Run `cargo test` - must pass before task 2

### Task 2: Backend API - user management enhancements

**Files:**
- Modify: `src/api/auth_routes.rs`

- [ ] Add GET /api/auth/users/{user_id} endpoint (admin-only):
  - Returns user detail with assigned project IDs and storage IDs
  - Response: `{ user: User, projects: Vec<Project>, storages: Vec<StorageBackend> }`
- [ ] Add PUT /api/auth/users/{user_id} endpoint (admin-only):
  - Allow updating role and/or resetting password
  - Input: `{ role?: String, password?: String }`
  - Validate role is "admin" or "user", validate password length
  - Prevent admin from demoting themselves
- [ ] Write tests for new endpoints
- [ ] Run `cargo test` - must pass before task 3

### Task 3: Backend API - project and storage members

**Files:**
- Modify: `src/api/projects.rs`
- Modify: `src/api/storages.rs`

- [ ] Add project member endpoints (admin-only):
  - GET /api/projects/{id}/members -> list users assigned to project
  - POST /api/projects/{id}/members -> assign user `{ user_id }`, return 409 if already assigned
  - DELETE /api/projects/{id}/members/{user_id} -> remove assignment
- [ ] Add storage member endpoints (admin-only):
  - GET /api/storages/{id}/members -> list users assigned to storage
  - POST /api/storages/{id}/members -> assign user `{ user_id }`, return 409 if already assigned
  - DELETE /api/storages/{id}/members/{user_id} -> remove assignment
- [ ] Write tests for member endpoints
- [ ] Run `cargo test` - must pass before task 4

### Task 4: Update authorization logic

**Files:**
- Modify: `src/api/auth.rs`
- Modify: `src/api/projects.rs`
- Modify: `src/api/storages.rs`

- [ ] Add `is_member_of_project(pool, user_id, project_id) -> bool` check to auth helper or use UserProject::is_member
- [ ] Update project access: allow access if user is owner OR admin OR member (via user_projects)
  - Update GET /api/projects to also return projects where user is a member (not just owned)
  - Update GET/PUT/DELETE /api/projects/{id} to allow members (read access for members, write for owner/admin)
  - Members get read access (view project, list files, download). Owner/admin get write access (update, delete, upload, manage storages)
- [ ] Update storage access: allow read access if user is assigned via user_storages (admin retains full access)
  - GET /api/storages - for non-admin users, return only assigned storages
  - GET /api/storages/{id} - allow if member
- [ ] Write tests for authorization changes
- [ ] Run `cargo test` - must pass before task 5

### Task 5: Frontend - Users management page

**Files:**
- Create: `frontend/src/pages/Users.tsx`
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/Sidebar.tsx`

- [ ] Add TypeScript types to types.ts:
  - `UserWithAssignments { user: AuthUser, projects: Project[], storages: StorageBackend[] }`
  - `CreateUserInput { username, password, role }`
  - `UpdateUserInput { role?, password? }`
- [ ] Create Users.tsx page:
  - Users table: username, role (badge), created_at, actions (edit, delete)
  - Create user form (inline or modal): username, password, role dropdown
  - Delete user with confirmation (prevent deleting self)
  - Role badge: admin (purple), user (gray)
  - Admin-only page
- [ ] Add /users route to App.tsx (protected, admin-only)
- [ ] Add Users nav item to Sidebar.tsx (admin-only, with Users icon from lucide-react)
- [ ] Write tests for Users page (render, create, delete)
- [ ] Run `cd frontend && npm test` - must pass before task 6

### Task 6: Frontend - User detail with assignments

**Files:**
- Create: `frontend/src/pages/UserDetail.tsx`
- Modify: `frontend/src/App.tsx`

- [ ] Create UserDetail.tsx page:
  - User info header: username, role, created_at
  - Edit user section: change role dropdown, reset password button/form
  - Projects section: table of assigned projects with remove button, "Add project" button opening a dropdown/modal of unassigned projects
  - Storages section: table of assigned storages with remove button, "Add storage" button opening a dropdown/modal of unassigned storages
  - Use React Query for fetching user detail, projects list, storages list
  - Mutations for add/remove assignments with cache invalidation
- [ ] Add /users/:id route to App.tsx (protected, admin-only)
- [ ] Make username in Users.tsx table a link to /users/:id
- [ ] Write tests for UserDetail page
- [ ] Run `cd frontend && npm test` - must pass before task 7

### Task 7: Frontend - Members sections on Project and Storage detail pages

**Files:**
- Modify: `frontend/src/pages/ProjectDetail.tsx`
- Modify: `frontend/src/pages/StorageDetail.tsx`

- [ ] Add Members section to ProjectDetail.tsx (visible to admins):
  - Table: username, role, assigned date, remove button
  - "Add member" button: modal/dropdown with available users (not yet assigned)
  - Shows project owner separately (with "Owner" badge, not removable)
- [ ] Add Members section to StorageDetail.tsx (visible to admins):
  - Table: username, assigned date, remove button
  - "Add member" button: modal/dropdown with available users
- [ ] Write tests for members sections
- [ ] Run `cd frontend && npm test` - must pass before task 8

### Task 8: Verify acceptance criteria

- [ ] Manual test: create user, assign to project and storage, verify user can see assigned resources
- [ ] Manual test: remove assignment, verify user loses access
- [ ] Manual test: admin can see all users/projects/storages, manage assignments from both sides
- [ ] Run full backend test suite: `cargo test`
- [ ] Run full frontend test suite: `cd frontend && npm test`
- [ ] Run backend linter: `cargo clippy -- -D warnings`
- [ ] Run frontend linter: `cd frontend && npm run lint`

### Task 9: Update documentation

- [ ] Update CLAUDE.md with new routes, tables, and patterns
- [ ] Move this plan to `docs/plans/completed/`
