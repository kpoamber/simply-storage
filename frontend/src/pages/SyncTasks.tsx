import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import apiClient from '../api/client';
import { SyncTask, StorageBackend } from '../api/types';

const STATUS_OPTIONS = ['all', 'pending', 'in_progress', 'completed', 'failed'];

export default function SyncTasks() {
  const [statusFilter, setStatusFilter] = useState('all');
  const [page, setPage] = useState(1);
  const perPage = 20;

  const { data: syncTasks, isLoading } = useQuery<SyncTask[]>({
    queryKey: ['sync-tasks', statusFilter, page],
    queryFn: () => {
      const params: Record<string, string | number> = { page, per_page: perPage };
      if (statusFilter !== 'all') params.status = statusFilter;
      return apiClient.get('/sync-tasks', { params }).then(r => r.data);
    },
  });

  const { data: storages } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  const storageMap = new Map(storages?.map(s => [s.id, s.name]) ?? []);
  const getStorageName = (id: string) => storageMap.get(id) ?? id.slice(0, 8) + '...';

  return (
    <div>
      <h2 className="text-2xl font-semibold text-gray-800">Sync Tasks</h2>
      <p className="mt-1 text-gray-500">Monitor file synchronization tasks.</p>

      <div className="mt-4 flex items-center gap-2">
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
                    <td className="whitespace-nowrap px-4 py-2 text-sm font-mono text-gray-900">{task.file_id.slice(0, 8)}...</td>
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
                      {task.error_msg ?? '\u2014'}
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
    </div>
  );
}
