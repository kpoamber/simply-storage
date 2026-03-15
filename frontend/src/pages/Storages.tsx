import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Eye, Plus, X, Pencil, Trash2 } from 'lucide-react';
import apiClient from '../api/client';
import { StorageBackend, formatBytes } from '../api/types';
import StorageForm from '../components/StorageForm';
import { useAuth } from '../contexts/AuthContext';

export default function Storages() {
  const queryClient = useQueryClient();
  const { user: currentUser } = useAuth();
  const isAdmin = currentUser?.role === 'admin';
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  const { data: storages, isLoading } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  const createMutation = useMutation({
    mutationFn: (data: { name: string; storage_type: string; config: Record<string, unknown>; is_hot: boolean; supports_direct_links: boolean }) =>
      apiClient.post('/storages', data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storages'] });
      setShowCreateForm(false);
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, data }: { id: string; data: { name?: string; config?: Record<string, unknown>; is_hot?: boolean; enabled?: boolean; supports_direct_links?: boolean } }) =>
      apiClient.put(`/storages/${id}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storages'] });
      setEditingId(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (id: string) => apiClient.delete(`/storages/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['storages'] });
    },
  });

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-semibold text-gray-800">Storages</h2>
          <p className="mt-1 text-gray-500">Manage storage backends.</p>
        </div>
        {isAdmin && (
          <button
            onClick={() => setShowCreateForm(true)}
            className="flex items-center gap-1 rounded bg-blue-600 px-3 py-2 text-sm text-white hover:bg-blue-700"
          >
            <Plus className="h-4 w-4" /> Add Storage
          </button>
        )}
      </div>

      {showCreateForm && (
        <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="font-medium text-gray-800">Add Storage</h3>
            <button onClick={() => setShowCreateForm(false)} className="text-gray-400 hover:text-gray-600">
              <X className="h-4 w-4" />
            </button>
          </div>
          <StorageForm
            onSubmit={(data) => createMutation.mutate(data)}
            isLoading={createMutation.isPending}
            onCancel={() => setShowCreateForm(false)}
          />
        </div>
      )}

      {isLoading ? (
        <p className="mt-6 text-gray-400">Loading storages...</p>
      ) : !storages?.length ? (
        <p className="mt-6 text-gray-400">No storages configured.</p>
      ) : (
        <div className="mt-6 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Type</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Tier</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Files</th>
                <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Used</th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {storages.map(storage => (
                <tr key={storage.id}>
                  <td className="whitespace-nowrap px-4 py-2 text-sm font-medium text-gray-900">{storage.name}</td>
                  <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">{storage.storage_type}</td>
                  <td className="whitespace-nowrap px-4 py-2 text-sm">
                    <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${storage.is_hot ? 'bg-red-100 text-red-700' : 'bg-blue-100 text-blue-700'}`}>
                      {storage.is_hot ? 'Hot' : 'Cold'}
                    </span>
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-sm">
                    <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${storage.enabled ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-500'}`}>
                      {storage.enabled ? 'Enabled' : 'Disabled'}
                    </span>
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">{storage.file_count}</td>
                  <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">{formatBytes(storage.used_space)}</td>
                  <td className="whitespace-nowrap px-4 py-2 text-center">
                    <div className="flex items-center justify-center gap-2">
                      <Link to={`/storages/${storage.id}`} className="text-blue-600 hover:text-blue-800" title="View">
                        <Eye className="h-4 w-4" />
                      </Link>
                      {isAdmin && (
                        <>
                          <button onClick={() => setEditingId(storage.id)} className="text-gray-500 hover:text-gray-700" title="Edit">
                            <Pencil className="h-4 w-4" />
                          </button>
                          <button
                            onClick={() => { if (window.confirm('Disable this storage?')) deleteMutation.mutate(storage.id); }}
                            className="text-red-400 hover:text-red-600"
                            title="Disable"
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        </>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {editingId && (
        <EditStorageModal
          storageId={editingId}
          storages={storages ?? []}
          onSubmit={(data) => updateMutation.mutate({ id: editingId, data })}
          onCancel={() => setEditingId(null)}
          isLoading={updateMutation.isPending}
        />
      )}
    </div>
  );
}

function EditStorageModal({
  storageId,
  storages,
  onSubmit,
  onCancel,
  isLoading,
}: {
  storageId: string;
  storages: StorageBackend[];
  onSubmit: (data: { name?: string; config?: Record<string, unknown>; is_hot?: boolean; enabled?: boolean; supports_direct_links?: boolean }) => void;
  onCancel: () => void;
  isLoading: boolean;
}) {
  const storage = storages.find(s => s.id === storageId);
  if (!storage) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
      <div className="w-full max-w-lg rounded-lg border border-gray-200 bg-white p-6 shadow-lg">
        <div className="mb-4 flex items-center justify-between">
          <h3 className="text-lg font-medium text-gray-800">Edit Storage: {storage.name}</h3>
          <button onClick={onCancel} className="text-gray-400 hover:text-gray-600">
            <X className="h-5 w-5" />
          </button>
        </div>
        <StorageForm
          initialValues={{
            name: storage.name,
            storage_type: storage.storage_type,
            config: storage.config,
            is_hot: storage.is_hot,
            supports_direct_links: storage.supports_direct_links,
          }}
          onSubmit={(data) => onSubmit({ name: data.name, config: data.config, is_hot: data.is_hot, supports_direct_links: data.supports_direct_links })}
          isLoading={isLoading}
          onCancel={onCancel}
          isEdit
        />
      </div>
    </div>
  );
}
