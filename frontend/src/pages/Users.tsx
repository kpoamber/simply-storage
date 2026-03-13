import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, X } from 'lucide-react';
import apiClient from '../api/client';
import type { AuthUser } from '../api/types';
import { useAuth } from '../contexts/AuthContext';

export default function Users() {
  const queryClient = useQueryClient();
  const { user: currentUser } = useAuth();
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const { data: users, isLoading } = useQuery<AuthUser[]>({
    queryKey: ['users'],
    queryFn: () => apiClient.get('/auth/users').then((r) => r.data),
  });

  const createMutation = useMutation({
    mutationFn: (data: { username: string; password: string; role: string }) =>
      apiClient.post('/auth/users', data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      setShowCreateForm(false);
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to create user';
      setError(message);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (userId: string) =>
      apiClient.delete(`/auth/users/${userId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] });
      setDeletingId(null);
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to delete user';
      setError(message);
    },
  });

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-semibold text-gray-800">Users</h2>
          <p className="mt-1 text-gray-500">Manage user accounts.</p>
        </div>
        <button
          onClick={() => {
            setShowCreateForm(true);
            setError(null);
          }}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-2 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> New User
        </button>
      </div>

      {error && (
        <div className="mt-4 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700">
          {error}
        </div>
      )}

      {showCreateForm && (
        <CreateUserForm
          onSubmit={(data) => createMutation.mutate(data)}
          onCancel={() => {
            setShowCreateForm(false);
            setError(null);
          }}
          isLoading={createMutation.isPending}
        />
      )}

      {isLoading ? (
        <p className="mt-6 text-gray-400">Loading users...</p>
      ) : !users?.length ? (
        <p className="mt-6 text-gray-400">No users found.</p>
      ) : (
        <div className="mt-6 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Username
                </th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Role
                </th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Created
                </th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {users.map((u) => (
                <tr key={u.id}>
                  <td className="whitespace-nowrap px-4 py-2 text-sm font-medium text-gray-900">
                    <Link
                      to={`/users/${u.id}`}
                      className="text-blue-600 hover:text-blue-800 hover:underline"
                    >
                      {u.username}
                    </Link>
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-sm">
                    <RoleBadge role={u.role} />
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                    {new Date(u.created_at).toLocaleDateString()}
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-center">
                    {deletingId === u.id ? (
                      <div className="flex items-center justify-center gap-2">
                        <span className="text-xs text-red-600">Delete?</span>
                        <button
                          onClick={() => deleteMutation.mutate(u.id)}
                          disabled={deleteMutation.isPending}
                          className="rounded bg-red-600 px-2 py-1 text-xs text-white hover:bg-red-700 disabled:opacity-50"
                        >
                          Yes
                        </button>
                        <button
                          onClick={() => setDeletingId(null)}
                          className="rounded bg-gray-200 px-2 py-1 text-xs hover:bg-gray-300"
                        >
                          No
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setDeletingId(u.id)}
                        disabled={u.id === currentUser?.id}
                        className="text-red-500 hover:text-red-700 disabled:cursor-not-allowed disabled:opacity-30"
                        title={
                          u.id === currentUser?.id
                            ? 'Cannot delete yourself'
                            : 'Delete user'
                        }
                      >
                        <Trash2 className="h-4 w-4" />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function RoleBadge({ role }: { role: string }) {
  const colors =
    role === 'admin'
      ? 'bg-purple-100 text-purple-800'
      : 'bg-gray-100 text-gray-800';
  return (
    <span
      className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${colors}`}
    >
      {role}
    </span>
  );
}

function CreateUserForm({
  onSubmit,
  onCancel,
  isLoading,
}: {
  onSubmit: (data: { username: string; password: string; role: string }) => void;
  onCancel: () => void;
  isLoading: boolean;
}) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [role, setRole] = useState('user');

  return (
    <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="font-medium text-gray-800">Create User</h3>
        <button
          onClick={onCancel}
          className="text-gray-400 hover:text-gray-600"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          onSubmit({ username, password, role });
        }}
        className="flex flex-wrap gap-3"
      >
        <input
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="Username"
          required
          className="rounded border border-gray-300 px-3 py-1.5 text-sm"
        />
        <input
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Password"
          type="password"
          required
          className="rounded border border-gray-300 px-3 py-1.5 text-sm"
        />
        <select
          value={role}
          onChange={(e) => setRole(e.target.value)}
          className="rounded border border-gray-300 px-3 py-1.5 text-sm"
        >
          <option value="user">user</option>
          <option value="admin">admin</option>
        </select>
        <button
          type="submit"
          disabled={isLoading}
          className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {isLoading ? 'Creating...' : 'Create'}
        </button>
      </form>
    </div>
  );
}
