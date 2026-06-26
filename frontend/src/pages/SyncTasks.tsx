import { useState, useEffect } from 'react';
import { useSearchParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ChevronRight, X } from 'lucide-react';
import apiClient from '../api/client';
import { SyncTask, StorageBackend, Project, FileMetadata, formatBytes } from '../api/types';

const STATUS_OPTIONS = ['all', 'pending', 'in_progress', 'completed', 'failed'];

export default function SyncTasks() {
  // Allow other pages (e.g. the dashboard Sync queue cards) to deep-link here
  // with `?status=&project_id=` prefilled. We mirror state back to the URL so
  // the link is shareable and Back/Forward navigation works as expected.
  const [searchParams, setSearchParams] = useSearchParams();
  const urlStatus = searchParams.get('status');
  const urlProject = searchParams.get('project_id');
  const initialStatus = urlStatus && STATUS_OPTIONS.includes(urlStatus) ? urlStatus : 'all';
  const initialProject = urlProject ?? 'all';

  const [statusFilter, setStatusFilter] = useState(initialStatus);
  const [projectFilter, setProjectFilter] = useState(initialProject);
  const [page, setPage] = useState(1);
  const [inspectFileId, setInspectFileId] = useState<string | null>(null);
  const perPage = 20;

  useEffect(() => {
    const next = new URLSearchParams();
    if (statusFilter !== 'all') next.set('status', statusFilter);
    if (projectFilter !== 'all') next.set('project_id', projectFilter);
    setSearchParams(next, { replace: true });
  }, [statusFilter, projectFilter, setSearchParams]);

  const { data: syncTasks, isLoading } = useQuery<SyncTask[]>({
    queryKey: ['sync-tasks', statusFilter, projectFilter, page],
    queryFn: () => {
      const params: Record<string, string | number> = { page, per_page: perPage };
      if (statusFilter !== 'all') params.status = statusFilter;
      if (projectFilter !== 'all') params.project_id = projectFilter;
      return apiClient.get('/sync-tasks', { params }).then(r => r.data);
    },
  });

  const { data: storages } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  const { data: projects } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: () => apiClient.get('/projects').then(r => r.data),
    staleTime: 60_000,
  });

  const storageMap = new Map(storages?.map(s => [s.id, s.name]) ?? []);
  const getStorageName = (id: string) => storageMap.get(id) ?? id.slice(0, 8) + '...';

  return (
    <div>
      <h2 className="text-2xl font-semibold text-gray-800">Sync Tasks</h2>
      <p className="mt-1 text-gray-500">Monitor file synchronization tasks.</p>

      <div className="mt-4 flex flex-wrap items-center gap-4">
        <div className="flex items-center gap-2">
          <label className="text-sm text-gray-600">Filter by status:</label>
          <select
            value={statusFilter}
            onChange={e => { setStatusFilter(e.target.value); setPage(1); }}
            className="rounded border border-gray-300 px-3 py-1.5 text-sm"
            aria-label="Filter by status"
          >
            {STATUS_OPTIONS.map(s => (
              <option key={s} value={s}>{s === 'all' ? 'All' : s.replace('_', ' ')}</option>
            ))}
          </select>
        </div>
        <div className="flex items-center gap-2">
          <label className="text-sm text-gray-600">Filter by project:</label>
          <select
            value={projectFilter}
            onChange={e => { setProjectFilter(e.target.value); setPage(1); }}
            className="rounded border border-gray-300 px-3 py-1.5 text-sm"
            aria-label="Filter by project"
            data-testid="project-filter"
          >
            <option value="all">All projects</option>
            {projects?.map(p => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </div>
      </div>

      {isLoading ? (
        <p className="mt-6 text-gray-400">Loading sync tasks...</p>
      ) : !syncTasks?.length ? (
        <p className="mt-6 text-gray-400">No sync tasks found.</p>
      ) : (
        <>
          <div className="mt-4 overflow-hidden rounded-lg border border-gray-200">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">File</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Project</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Source</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Target</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                  <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Retries</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Error</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Updated</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white">
                {syncTasks.map(task => (
                  <tr key={task.id}>
                    <td className="whitespace-nowrap px-4 py-2 text-sm font-mono">
                      <button
                        type="button"
                        onClick={() => setInspectFileId(task.file_id)}
                        className="text-blue-600 hover:underline"
                        title="View file details"
                      >
                        {task.file_id.slice(0, 8)}...
                      </button>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-700">
                      {task.project_name ?? <span className="text-gray-400">—</span>}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">{getStorageName(task.source_storage_id)}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">{getStorageName(task.target_storage_id)}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm">
                      <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                        task.status === 'completed' ? 'bg-green-100 text-green-700' :
                        task.status === 'pending' ? 'bg-yellow-100 text-yellow-700' :
                        task.status === 'in_progress' ? 'bg-blue-100 text-blue-700' :
                        task.status === 'failed' ? 'bg-red-100 text-red-700' :
                        'bg-gray-100 text-gray-500'
                      }`}>
                        {task.status}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-500">{task.retries}</td>
                    <td className="max-w-xs truncate px-4 py-2 text-sm text-red-500" title={task.error_msg ?? undefined}>
                      {task.error_msg ?? '—'}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                      {new Date(task.created_at).toLocaleString()}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                      {new Date(task.updated_at).toLocaleString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <div className="mt-3 flex items-center justify-between">
            <button
              onClick={() => setPage(p => Math.max(1, p - 1))}
              disabled={page === 1}
              className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
            >
              <ChevronLeft className="h-4 w-4" /> Previous
            </button>
            <span className="text-sm text-gray-500">Page {page}</span>
            <button
              onClick={() => setPage(p => p + 1)}
              disabled={(syncTasks?.length ?? 0) < perPage}
              className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
            >
              Next <ChevronRight className="h-4 w-4" />
            </button>
          </div>
        </>
      )}

      {inspectFileId && (
        <FileInspectorModal
          fileId={inspectFileId}
          storageMap={storageMap}
          projects={projects ?? []}
          onClose={() => setInspectFileId(null)}
        />
      )}
    </div>
  );
}

/// Side-modal that fetches `/api/files/{id}` and surfaces enough info to
/// answer "what is this file and which storages does it live on?" without
/// leaving the Sync Tasks page. Status is read straight from
/// file_locations (one row per storage), so a row may show 'synced',
/// 'archived' (in cold tier), 'restoring', etc. — same statuses the file
/// detail view in ProjectDetail uses.
function FileInspectorModal({
  fileId,
  storageMap,
  projects,
  onClose,
}: {
  fileId: string;
  storageMap: Map<string, string>;
  projects: Project[];
  onClose: () => void;
}) {
  const { data, isLoading, error } = useQuery<FileMetadata>({
    queryKey: ['file-metadata', fileId],
    queryFn: () => apiClient.get(`/files/${fileId}`).then(r => r.data),
  });
  const projectMap = new Map(projects.map(p => [p.id, p.name]));
  return (
    <div
      className="fixed inset-0 z-40 flex items-start justify-center bg-black/40 px-4 py-10"
      onClick={onClose}
    >
      <div
        className="w-full max-w-2xl rounded-lg bg-white p-5 shadow-lg"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-4">
          <div>
            <h3 className="text-lg font-semibold text-gray-800">File details</h3>
            <p className="mt-0.5 break-all font-mono text-xs text-gray-500">{fileId}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-700"
            aria-label="Close"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {isLoading && <p className="mt-4 text-sm text-gray-400">Loading...</p>}
        {error && <p className="mt-4 text-sm text-red-500">Failed to load file metadata.</p>}

        {data && (
          <div className="mt-4 space-y-4">
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm">
              <dt className="text-gray-500">Name</dt>
              <dd className="break-all text-gray-900">
                {data.references[0]?.original_name ?? <span className="text-gray-400">—</span>}
              </dd>
              <dt className="text-gray-500">Size</dt>
              <dd className="text-gray-900">{formatBytes(data.file.size)}</dd>
              <dt className="text-gray-500">Content type</dt>
              <dd className="font-mono text-xs text-gray-700">{data.file.content_type}</dd>
              <dt className="text-gray-500">Created</dt>
              <dd className="text-gray-700">{new Date(data.file.created_at).toLocaleString()}</dd>
              <dt className="text-gray-500">Projects</dt>
              <dd className="text-gray-700">
                {data.references.length === 0 ? (
                  <span className="text-gray-400">—</span>
                ) : (
                  Array.from(new Set(data.references.map(r => r.project_id)))
                    .map(pid => projectMap.get(pid) ?? pid.slice(0, 8) + '...')
                    .join(', ')
                )}
              </dd>
            </dl>

            <div>
              <h4 className="text-xs font-medium uppercase text-gray-500">Storage locations</h4>
              {data.locations.length === 0 ? (
                <p className="mt-2 text-sm text-gray-400">No locations recorded for this file.</p>
              ) : (
                <table className="mt-2 min-w-full text-sm">
                  <thead className="text-left text-xs uppercase text-gray-500">
                    <tr>
                      <th className="py-1 pr-3 font-medium">Storage</th>
                      <th className="py-1 pr-3 font-medium">Status</th>
                      <th className="py-1 pr-3 font-medium">Synced at</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-gray-100">
                    {data.locations.map(loc => (
                      <tr key={loc.id}>
                        <td className="py-1.5 pr-3 text-gray-800">
                          {storageMap.get(loc.storage_id) ?? loc.storage_id.slice(0, 8) + '...'}
                        </td>
                        <td className="py-1.5 pr-3">
                          <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                            loc.status === 'synced' ? 'bg-green-100 text-green-700' :
                            loc.status === 'archived' ? 'bg-blue-100 text-blue-700' :
                            loc.status === 'restoring' ? 'bg-yellow-100 text-yellow-700' :
                            'bg-gray-100 text-gray-600'
                          }`}>
                            {loc.status}
                          </span>
                        </td>
                        <td className="py-1.5 pr-3 text-gray-500">
                          {loc.synced_at ? new Date(loc.synced_at).toLocaleString() : '—'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
