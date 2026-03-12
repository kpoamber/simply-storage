import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, Download, ChevronLeft, ChevronRight } from 'lucide-react';
import apiClient from '../api/client';
import { StorageBackend, FileLocation, ExportStatus, formatBytes } from '../api/types';

export default function StorageDetail() {
  const { id } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [page, setPage] = useState(1);
  const [exportJobId, setExportJobId] = useState<string | null>(null);
  const perPage = 20;

  const { data: storage, isLoading } = useQuery<StorageBackend>({
    queryKey: ['storage', id],
    queryFn: () => apiClient.get(`/storages/${id}`).then(r => r.data),
    enabled: !!id,
  });

  const { data: fileLocations } = useQuery<FileLocation[]>({
    queryKey: ['storage-files', id, page],
    queryFn: () =>
      apiClient.get(`/storages/${id}/files`, { params: { page, per_page: perPage } }).then(r => r.data),
    enabled: !!id,
  });

  const { data: exportStatus, refetch: refetchExport } = useQuery<ExportStatus>({
    queryKey: ['storage-export-status', id, exportJobId],
    queryFn: () => apiClient.get(`/storages/${id}/export/status`, { params: { job_id: exportJobId } }).then(r => r.data),
    enabled: !!id && !!exportJobId,
    refetchInterval: (query) => {
      const data = query.state.data;
      return data && data.status === 'in_progress' ? 2000 : false;
    },
  });

  const syncAllMutation = useMutation({
    mutationFn: () => apiClient.post(`/storages/${id}/sync-all`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storage', id] });
      queryClient.invalidateQueries({ queryKey: ['sync-tasks'] });
    },
  });

  const exportMutation = useMutation({
    mutationFn: () => apiClient.post(`/storages/${id}/export`).then(r => r.data),
    onSuccess: (data: { job_id: string }) => {
      setExportJobId(data.job_id);
      refetchExport();
    },
  });

  if (isLoading) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">Storage</h2>
        <p className="mt-2 text-gray-500">Loading storage...</p>
      </div>
    );
  }

  if (!storage) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">Storage</h2>
        <p className="mt-2 text-red-500">Storage not found.</p>
      </div>
    );
  }

  return (
    <div>
      <Link to="/storages" className="text-sm text-blue-600 hover:underline">&larr; Back to Storages</Link>
      <h2 className="mt-2 text-2xl font-semibold text-gray-800">{storage.name}</h2>
      <p className="text-gray-500">
        {storage.storage_type} &middot; {storage.is_hot ? 'Hot' : 'Cold'} &middot; {storage.enabled ? 'Enabled' : 'Disabled'}
      </p>

      <div className="mt-4 grid grid-cols-3 gap-4">
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <p className="text-sm text-gray-500">Files</p>
          <p className="text-2xl font-semibold text-gray-900">{storage.file_count}</p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <p className="text-sm text-gray-500">Used Space</p>
          <p className="text-2xl font-semibold text-gray-900">{formatBytes(storage.used_space)}</p>
        </div>
        <div className="rounded-lg border border-gray-200 bg-white p-4">
          <p className="text-sm text-gray-500">Type</p>
          <p className="text-2xl font-semibold text-gray-900">{storage.storage_type}</p>
        </div>
      </div>

      <div className="mt-6 flex gap-3">
        <button
          onClick={() => syncAllMutation.mutate()}
          disabled={syncAllMutation.isPending}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-2 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
        >
          <RefreshCw className={`h-4 w-4 ${syncAllMutation.isPending ? 'animate-spin' : ''}`} />
          {syncAllMutation.isPending ? 'Syncing...' : 'Sync All'}
        </button>
        <button
          onClick={() => exportMutation.mutate()}
          disabled={exportMutation.isPending || exportStatus?.status === 'in_progress'}
          className="flex items-center gap-1 rounded bg-green-600 px-3 py-2 text-sm text-white hover:bg-green-700 disabled:opacity-50"
        >
          <Download className="h-4 w-4" />
          {exportMutation.isPending ? 'Starting...' : 'Export'}
        </button>
      </div>

      {syncAllMutation.isSuccess && (
        <p className="mt-2 text-sm text-green-600">Sync tasks created successfully.</p>
      )}

      {exportStatus && (
        <div className="mt-3 rounded-lg border border-gray-200 bg-white p-4">
          <h4 className="text-sm font-medium text-gray-700">Export Status</h4>
          <div className="mt-2">
            <div className="flex items-center justify-between text-sm text-gray-600">
              <span>Status: {exportStatus.status}</span>
              <span>{exportStatus.processed_files}/{exportStatus.total_files} files</span>
            </div>
            {exportStatus.status === 'in_progress' && (
              <div className="mt-2 h-2 overflow-hidden rounded-full bg-gray-200">
                <div
                  className="h-full rounded-full bg-blue-600 transition-all"
                  style={{ width: `${exportStatus.total_files > 0 ? Math.round((exportStatus.processed_files / exportStatus.total_files) * 100) : 0}%` }}
                />
              </div>
            )}
            {exportStatus.error && (
              <p className="mt-2 text-sm text-red-600">{exportStatus.error}</p>
            )}
            {exportStatus.status === 'completed' && exportJobId && (
              <a
                href={`/api/storages/${id}/export/download?job_id=${exportJobId}`}
                className="mt-2 inline-block text-sm text-blue-600 hover:underline"
              >
                Download archive
              </a>
            )}
          </div>
        </div>
      )}

      <div className="mt-6">
        <h3 className="text-lg font-medium text-gray-700">Files on this Storage</h3>
        {!fileLocations?.length ? (
          <p className="mt-4 text-gray-400">No files on this storage.</p>
        ) : (
          <>
            <div className="mt-3 overflow-hidden rounded-lg border border-gray-200">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">File ID</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Path</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Synced At</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 bg-white">
                  {fileLocations.map(loc => (
                    <tr key={loc.id}>
                      <td className="whitespace-nowrap px-4 py-2 text-sm font-mono text-gray-900">{loc.file_id.slice(0, 8)}...</td>
                      <td className="px-4 py-2 text-sm text-gray-500">{loc.storage_path}</td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm">
                        <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${
                          loc.status === 'synced' ? 'bg-green-100 text-green-700' :
                          loc.status === 'pending' ? 'bg-yellow-100 text-yellow-700' :
                          loc.status === 'archived' ? 'bg-blue-100 text-blue-700' :
                          'bg-gray-100 text-gray-500'
                        }`}>
                          {loc.status}
                        </span>
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                        {loc.synced_at ? new Date(loc.synced_at).toLocaleString() : '\u2014'}
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
                disabled={(fileLocations?.length ?? 0) < perPage}
                className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
              >
                Next <ChevronRight className="h-4 w-4" />
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
