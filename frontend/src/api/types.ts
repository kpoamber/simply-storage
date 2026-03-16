export interface Project {
  id: string;
  name: string;
  slug: string;
  hot_to_cold_days: number | null;
  owner_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface ProjectWithStats {
  project: Project;
  stats: {
    file_count: number;
    total_size: number;
  };
}

export interface SystemStats {
  total_files: number;
  total_storage_used: number;
  pending_sync_tasks: number;
}

export interface StorageBase {
  id: string;
  name: string;
  storage_type: string;
  config: Record<string, unknown>;
  is_hot: boolean;
  project_id: string | null;
  enabled: boolean;
  supports_direct_links: boolean;
  created_at: string;
  updated_at: string;
}

export interface StorageBackend extends StorageBase {
  file_count: number;
  used_space: number;
}

export interface StorageSyncDetail {
  storage_id: string;
  storage_name: string;
  storage_type: string;
  status: string;
  storage_path: string | null;
  supports_direct_links: boolean;
  synced_at: string | null;
}

export interface FileReference {
  id: string;
  file_id: string;
  project_id: string;
  original_name: string;
  metadata: Record<string, string | number | boolean>;
  created_at: string;
  file_size?: number;
  sync_status?: string;
  synced_storages?: number;
  total_storages?: number;
  sync_details?: StorageSyncDetail[];
}

export interface FileRecord {
  id: string;
  hash_sha256: string;
  size: number;
  content_type: string;
  created_at: string;
}

export interface FileLocation {
  id: string;
  file_id: string;
  storage_id: string;
  storage_path: string;
  status: string;
  synced_at: string | null;
  last_accessed_at: string | null;
  created_at: string;
}

export interface FileMetadata {
  file: FileRecord;
  locations: FileLocation[];
  references: FileReference[];
}

export interface TempLinkEntry {
  storage_name: string;
  storage_type: string;
  url: string;
}

export interface TempLinkResponse {
  links: TempLinkEntry[];
  expires_in_seconds: number;
}

export interface ProjectStorageAssignment {
  id: string;
  project_id: string;
  storage_id: string;
  container_override: string | null;
  prefix_override: string | null;
  is_active: boolean;
  created_at: string;
  updated_at: string;
  storage_name: string;
  storage_type: string;
  is_hot: boolean;
  enabled: boolean;
}

export interface SyncTask {
  id: string;
  file_id: string;
  source_storage_id: string;
  target_storage_id: string;
  status: string;
  retries: number;
  error_msg: string | null;
  project_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface ExportStatus {
  job_id: string;
  storage_id: string;
  status: string;
  total_files: number;
  processed_files: number;
  total_bytes: number;
  error: string | null;
}

export interface AuthUser {
  id: string;
  username: string;
  role: 'admin' | 'user';
  created_at: string;
  updated_at: string;
}

export interface MemberInfo extends AuthUser {
  assigned_at: string;
  assignment_role?: string;
}

export interface ProjectAssignment extends Project {
  assignment_role: string;
}

export interface UserWithAssignments {
  user: AuthUser;
  projects: ProjectAssignment[];
  storages: StorageBase[];
}

export interface CreateUserInput {
  username: string;
  password: string;
  role: string;
}

export interface UpdateUserInput {
  role?: string;
  password?: string;
}

export interface LoginRequest {
  username: string;
  password: string;
}

export interface AuthTokenResponse {
  access_token: string;
  refresh_token: string;
}

// Search types
export interface MetadataFilterLeaf {
  key: string;
  value: string | number | boolean | null;
}

export interface MetadataFilterAnd {
  and: MetadataFilterNode[];
}

export interface MetadataFilterOr {
  or: MetadataFilterNode[];
}

export interface MetadataFilterNot {
  not: MetadataFilterNode;
}

export type MetadataFilterNode =
  | MetadataFilterLeaf
  | MetadataFilterAnd
  | MetadataFilterOr
  | MetadataFilterNot;

export interface SearchRequest {
  filters?: MetadataFilterNode;
  page?: number;
  per_page?: number;
}

export interface SearchResult {
  results: FileReference[];
  total: number;
  page: number;
  per_page: number;
}

export interface TimelineEntry {
  date: string;
  count: number;
  size: number;
}

export interface SearchSummary {
  total_files: number;
  total_size: number;
  earliest_upload: string | null;
  latest_upload: string | null;
  timeline: TimelineEntry[];
}

export interface BulkDeleteRequest {
  metadata_filters?: MetadataFilterNode;
  created_before?: string;
  created_after?: string;
  size_min?: number;
  size_max?: number;
  last_accessed_before?: string;
}

export interface BulkDeletePreview {
  matching_references: number;
  total_size: number;
}

export interface BulkDeleteResult {
  deleted_references: number;
  orphaned_files_cleaned: number;
  freed_bytes: number;
}

// Shared links
export interface SharedLink {
  id: string;
  token: string;
  file_id: string;
  project_id: string;
  original_name: string;
  created_by: string;
  password_protected: boolean;
  expires_at: string | null;
  max_downloads: number | null;
  download_count: number;
  last_accessed_at: string | null;
  is_active: boolean;
  created_at: string;
}

export interface SharedLinkInfo {
  file_name: string;
  file_size: number;
  content_type: string;
  password_required: boolean;
  expires_at: string | null;
}

export interface CreateSharedLinkRequest {
  file_id: string;
  password?: string;
  expires_in_seconds?: number;
  max_downloads?: number;
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}
