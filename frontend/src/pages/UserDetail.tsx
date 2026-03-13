import { useState, useEffect } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, X, KeyRound } from 'lucide-react';
import apiClient from '../api/client';
import type {
  UserWithAssignments,
  Project,
  StorageBackend,
  StorageBase,
} from '../api/types';
import { useAuth } from '../contexts/AuthContext';

export default function UserDetail() {
  const { id } = useParams<{ id: string }>();
  const { user: currentUser } = useAuth();

  const { data, isLoading } = useQuery<UserWithAssignments>({
    queryKey: ['user', id],
    queryFn: () => apiClient.get(`/auth/users/${id}`).then((r) => r.data),
    enabled: !!id,
  });

  if (isLoading) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">User</h2>
        <p className="mt-2 text-gray-500">Loading user...</p>
      </div>
    );
  }

  if (!data) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">User</h2>
        <p className="mt-2 text-red-500">User not found.</p>
      </div>
    );
  }

  const { user, projects, storages } = data;
  const isSelf = user.id === currentUser?.id;

  return (
    <div>
      <Link to="/users" className="text-sm text-blue-600 hover:underline">
        &larr; Back to Users
      </Link>
      <div className="mt-2 flex items-center gap-3">
        <h2 className="text-2xl font-semibold text-gray-800">
          {user.username}
        </h2>
        <RoleBadge role={user.role} />
      </div>
      <p className="text-sm text-gray-500">
        Created {new Date(user.created_at).toLocaleDateString()}
      </p>

      <EditUserSection userId={user.id} currentRole={user.role} isSelf={isSelf} />

      <ProjectAssignmentsSection userId={id!} assignedProjects={projects} />

      <StorageAssignmentsSection userId={id!} assignedStorages={storages} />
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

function EditUserSection({
  userId,
  currentRole,
  isSelf,
}: {
  userId: string;
  currentRole: string;
  isSelf: boolean;
}) {
  const queryClient = useQueryClient();
  const [role, setRole] = useState(currentRole);
  useEffect(() => { setRole(currentRole); }, [currentRole]);
  const [showPasswordForm, setShowPasswordForm] = useState(false);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);

  const roleMutation = useMutation({
    mutationFn: (newRole: string) =>
      apiClient.put(`/auth/users/${userId}`, { role: newRole }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user', userId] });
      queryClient.invalidateQueries({ queryKey: ['users'] });
      setError(null);
      setSuccess('Role updated.');
      setTimeout(() => setSuccess(null), 3000);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to update role';
      setError(message);
      setSuccess(null);
    },
  });

  const passwordMutation = useMutation({
    mutationFn: (newPassword: string) =>
      apiClient.put(`/auth/users/${userId}`, { password: newPassword }),
    onSuccess: () => {
      setShowPasswordForm(false);
      setPassword('');
      setError(null);
      setSuccess('Password reset.');
      setTimeout(() => setSuccess(null), 3000);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to reset password';
      setError(message);
      setSuccess(null);
    },
  });

  return (
    <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
      <h3 className="font-medium text-gray-800">Edit User</h3>

      {error && (
        <div className="mt-2 rounded border border-red-200 bg-red-50 p-2 text-sm text-red-700">
          {error}
        </div>
      )}
      {success && (
        <div className="mt-2 rounded border border-green-200 bg-green-50 p-2 text-sm text-green-700">
          {success}
        </div>
      )}

      <div className="mt-3 flex items-end gap-3">
        <div>
          <label className="block text-xs text-gray-500">Role</label>
          <select
            value={role}
            onChange={(e) => setRole(e.target.value)}
            disabled={isSelf}
            className="mt-1 rounded border border-gray-300 px-3 py-1.5 text-sm disabled:opacity-50"
          >
            <option value="user">user</option>
            <option value="admin">admin</option>
          </select>
        </div>
        <button
          onClick={() => roleMutation.mutate(role)}
          disabled={role === currentRole || roleMutation.isPending || isSelf}
          className="rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {roleMutation.isPending ? 'Saving...' : 'Update Role'}
        </button>
        <button
          onClick={() => {
            setShowPasswordForm(!showPasswordForm);
            setPassword('');
            setError(null);
          }}
          className="ml-auto flex items-center gap-1 rounded bg-gray-200 px-3 py-1.5 text-sm hover:bg-gray-300"
        >
          <KeyRound className="h-4 w-4" /> Reset Password
        </button>
      </div>

      {showPasswordForm && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            passwordMutation.mutate(password);
          }}
          className="mt-3 flex items-end gap-3"
        >
          <div>
            <label className="block text-xs text-gray-500">New Password</label>
            <input
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              type="password"
              placeholder="Min 6 characters"
              required
              minLength={6}
              className="mt-1 rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
          <button
            type="submit"
            disabled={passwordMutation.isPending}
            className="rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {passwordMutation.isPending ? 'Resetting...' : 'Confirm Reset'}
          </button>
          <button
            type="button"
            onClick={() => {
              setShowPasswordForm(false);
              setPassword('');
            }}
            className="text-gray-400 hover:text-gray-600"
          >
            <X className="h-4 w-4" />
          </button>
        </form>
      )}
    </div>
  );
}

function ProjectAssignmentsSection({
  userId,
  assignedProjects,
}: {
  userId: string;
  assignedProjects: Project[];
}) {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: allProjects } = useQuery<Project[]>({
    queryKey: ['all-projects-for-assign'],
    queryFn: () => apiClient.get('/projects').then((r) => r.data),
    enabled: showAdd,
  });

  const unassignedProjects = allProjects?.filter(
    (p) => !assignedProjects.some((ap) => ap.id === p.id),
  );

  const addMutation = useMutation({
    mutationFn: (projectId: string) =>
      apiClient.post(`/projects/${projectId}/members`, { user_id: userId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user', userId] });
      setShowAdd(false);
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to add project assignment';
      setError(message);
    },
  });

  const removeMutation = useMutation({
    mutationFn: (projectId: string) =>
      apiClient.delete(`/projects/${projectId}/members/${userId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user', userId] });
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to remove project assignment';
      setError(message);
    },
  });

  return (
    <div className="mt-6">
      {error && (
        <div className="mb-2 rounded border border-red-200 bg-red-50 p-2 text-sm text-red-700">
          {error}
        </div>
      )}
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-medium text-gray-700">
          Assigned Projects ({assignedProjects.length})
        </h3>
        <button
          onClick={() => setShowAdd(true)}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> Add Project
        </button>
      </div>

      {assignedProjects.length === 0 ? (
        <p className="mt-2 text-gray-400">No projects assigned.</p>
      ) : (
        <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Name
                </th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Slug
                </th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {assignedProjects.map((p) => (
                <tr key={p.id}>
                  <td className="px-4 py-2 text-sm">
                    <Link
                      to={`/projects/${p.id}`}
                      className="text-blue-600 hover:underline"
                    >
                      {p.name}
                    </Link>
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">{p.slug}</td>
                  <td className="px-4 py-2 text-center">
                    <button
                      onClick={() => removeMutation.mutate(p.id)}
                      disabled={removeMutation.isPending}
                      title="Remove assignment"
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
        <AddAssignmentModal
          title="Add Project Assignment"
          items={unassignedProjects ?? []}
          getLabel={(p) => `${p.name} (${p.slug})`}
          getId={(p) => p.id}
          onSelect={(id) => addMutation.mutate(id)}
          onClose={() => setShowAdd(false)}
          isPending={addMutation.isPending}
          emptyMessage="No unassigned projects available."
        />
      )}
    </div>
  );
}

function StorageAssignmentsSection({
  userId,
  assignedStorages,
}: {
  userId: string;
  assignedStorages: StorageBase[];
}) {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { data: allStorages } = useQuery<StorageBackend[]>({
    queryKey: ['all-storages-for-assign'],
    queryFn: () => apiClient.get('/storages').then((r) => r.data),
    enabled: showAdd,
  });

  const unassignedStorages = allStorages?.filter(
    (s) => !assignedStorages.some((as_) => as_.id === s.id),
  );

  const addMutation = useMutation({
    mutationFn: (storageId: string) =>
      apiClient.post(`/storages/${storageId}/members`, { user_id: userId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user', userId] });
      setShowAdd(false);
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to add storage assignment';
      setError(message);
    },
  });

  const removeMutation = useMutation({
    mutationFn: (storageId: string) =>
      apiClient.delete(`/storages/${storageId}/members/${userId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['user', userId] });
      setError(null);
    },
    onError: (err: unknown) => {
      const message =
        (err as { response?: { data?: { error?: string } } })?.response?.data
          ?.error || 'Failed to remove storage assignment';
      setError(message);
    },
  });

  return (
    <div className="mt-6">
      {error && (
        <div className="mb-2 rounded border border-red-200 bg-red-50 p-2 text-sm text-red-700">
          {error}
        </div>
      )}
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-medium text-gray-700">
          Assigned Storages ({assignedStorages.length})
        </h3>
        <button
          onClick={() => setShowAdd(true)}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> Add Storage
        </button>
      </div>

      {assignedStorages.length === 0 ? (
        <p className="mt-2 text-gray-400">No storages assigned.</p>
      ) : (
        <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Name
                </th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Type
                </th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">
                  Tier
                </th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {assignedStorages.map((s) => (
                <tr key={s.id}>
                  <td className="px-4 py-2 text-sm">
                    <Link
                      to={`/storages/${s.id}`}
                      className="text-blue-600 hover:underline"
                    >
                      {s.name}
                    </Link>
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">
                    {s.storage_type}
                  </td>
                  <td className="px-4 py-2 text-sm">
                    <span
                      className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${
                        s.is_hot
                          ? 'bg-orange-50 text-orange-600'
                          : 'bg-blue-50 text-blue-600'
                      }`}
                    >
                      {s.is_hot ? 'hot' : 'cold'}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-center">
                    <button
                      onClick={() => removeMutation.mutate(s.id)}
                      disabled={removeMutation.isPending}
                      title="Remove assignment"
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
        <AddAssignmentModal
          title="Add Storage Assignment"
          items={unassignedStorages ?? []}
          getLabel={(s) => `${s.name} (${s.storage_type}, ${s.is_hot ? 'hot' : 'cold'})`}
          getId={(s) => s.id}
          onSelect={(id) => addMutation.mutate(id)}
          onClose={() => setShowAdd(false)}
          isPending={addMutation.isPending}
          emptyMessage="No unassigned storages available."
        />
      )}
    </div>
  );
}

function AddAssignmentModal<T>({
  title,
  items,
  getLabel,
  getId,
  onSelect,
  onClose,
  isPending,
  emptyMessage,
}: {
  title: string;
  items: T[];
  getLabel: (item: T) => string;
  getId: (item: T) => string;
  onSelect: (id: string) => void;
  onClose: () => void;
  isPending: boolean;
  emptyMessage: string;
}) {
  const [selectedId, setSelectedId] = useState('');

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-medium text-gray-800">{title}</h3>
          <button
            onClick={onClose}
            className="text-gray-400 hover:text-gray-600"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {items.length === 0 ? (
          <p className="mt-4 text-sm text-gray-400">{emptyMessage}</p>
        ) : (
          <div className="mt-4">
            <select
              value={selectedId}
              onChange={(e) => setSelectedId(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              <option value="">Select...</option>
              {items.map((item) => (
                <option key={getId(item)} value={getId(item)}>
                  {getLabel(item)}
                </option>
              ))}
            </select>
          </div>
        )}

        <div className="mt-4 flex gap-2">
          <button
            onClick={() => onSelect(selectedId)}
            disabled={!selectedId || isPending}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {isPending ? 'Adding...' : 'Add'}
          </button>
          <button
            onClick={onClose}
            className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
