export interface Project {
  id: string;
  name: string;
  slug: string;
  hot_to_cold_days: number | null;
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

export interface StorageBackend {
  id: string;
  name: string;
  storage_type: string;
  config: Record<string, unknown>;
  is_hot: boolean;
  project_id: string | null;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  file_count: number;
  used_space: number;
}

export interface FileReference {
  id: string;
  file_id: string;
  project_id: string;
  original_name: string;
  created_at: string;
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

export interface TempLinkResponse {
  url: string;
  expires_in_seconds: number;
}

export interface SyncTask {
  id: string;
  file_id: string;
  source_storage_id: string;
  target_storage_id: string;
  status: string;
  retries: number;
  error_msg: string | null;
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

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
}
