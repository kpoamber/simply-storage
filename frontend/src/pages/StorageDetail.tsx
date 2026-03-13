import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, Download, ChevronLeft, ChevronRight, Plus, Trash2, X } from 'lucide-react';
import apiClient from '../api/client';
import { StorageBackend, FileLocation, ExportStatus, AuthUser, formatBytes } from '../api/types';
import { useAuth } from '../contexts/AuthContext';

export default function StorageDetail() {
  const { id } = useParams<{ id: string }>();
  const { user: currentUser } = useAuth();
  const isAdmin = currentUser?.role === 'admin';
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

      {isAdmin && <StorageMembersSection storageId={id!} />}

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

function StorageMembersSection({ storageId }: { storageId: string }) {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [selectedUserId, setSelectedUserId] = useState('');

  const { data: members } = useQuery<AuthUser[]>({
    queryKey: ['storage-members', storageId],
    queryFn: () => apiClient.get(`/storages/${storageId}/members`).then(r => r.data),
  });

  const { data: allUsers } = useQuery<AuthUser[]>({
    queryKey: ['all-users-for-storage-members'],
    queryFn: () => apiClient.get('/auth/users').then(r => r.data),
    enabled: showAdd,
  });

  const unassignedUsers = allUsers?.filter(
    u => !members?.some(m => m.id === u.id),
  );

  const addMutation = useMutation({
    mutationFn: (userId: string) =>
      apiClient.post(`/storages/${storageId}/members`, { user_id: userId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storage-members', storageId] });
      setShowAdd(false);
      setSelectedUserId('');
    },
  });

  const removeMutation = useMutation({
    mutationFn: (userId: string) =>
      apiClient.delete(`/storages/${storageId}/members/${userId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storage-members', storageId] });
    },
  });

  return (
    <div className="mt-6">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-medium text-gray-700">Members</h3>
        <button
          onClick={() => setShowAdd(true)}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> Add Member
        </button>
      </div>

      {!members || members.length === 0 ? (
        <p className="mt-2 text-gray-400">No members assigned.</p>
      ) : (
        <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Username</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Assigned</th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {members.map(m => (
                <tr key={m.id}>
                  <td className="px-4 py-2 text-sm">
                    <Link to={`/users/${m.id}`} className="text-blue-600 hover:underline">{m.username}</Link>
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">{new Date(m.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-2 text-center">
                    <button
                      onClick={() => removeMutation.mutate(m.id)}
                      disabled={removeMutation.isPending}
                      title="Remove member"
                      className="text-red-500 hover:text-red-700 disabled:opacity-50"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {showAdd && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
            <div className="flex items-center justify-between">
              <h3 className="text-lg font-medium text-gray-800">Add Member</h3>
              <button onClick={() => { setShowAdd(false); setSelectedUserId(''); }} className="text-gray-400 hover:text-gray-600"><X className="h-5 w-5" /></button>
            </div>
            {!unassignedUsers || unassignedUsers.length === 0 ? (
              <p className="mt-4 text-sm text-gray-400">No available users to add.</p>
            ) : (
              <div className="mt-4">
                <select
                  value={selectedUserId}
                  onChange={e => setSelectedUserId(e.target.value)}
                  className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                >
                  <option value="">Select a user...</option>
                  {unassignedUsers.map(u => (
                    <option key={u.id} value={u.id}>{u.username} ({u.role})</option>
                  ))}
                </select>
                <button
                  onClick={() => addMutation.mutate(selectedUserId)}
                  disabled={!selectedUserId || addMutation.isPending}
                  className="mt-2 rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
                >
                  {addMutation.isPending ? 'Adding...' : 'Add'}
                </button>
              </div>
            )}
            <div className="mt-4 flex gap-2">
              <button onClick={() => { setShowAdd(false); setSelectedUserId(''); }} className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300">Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
