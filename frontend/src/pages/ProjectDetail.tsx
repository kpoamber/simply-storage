import { useState, useCallback, useRef } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Upload, Download, Link2, ArchiveRestore, Trash2,
  ChevronLeft, ChevronRight, Search, Check, Plus, X, Pencil,
} from 'lucide-react';
import apiClient from '../api/client';
import { ProjectWithStats, FileReference, TempLinkResponse, StorageBackend, ProjectStorageAssignment, AuthUser, formatBytes } from '../api/types';
import { useAuth } from '../contexts/AuthContext';

export default function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const { user: currentUser } = useAuth();
  const isAdmin = currentUser?.role === 'admin';
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState('');
  const perPage = 20;

  const { data: projectData, isLoading } = useQuery<ProjectWithStats>({
    queryKey: ['project', id],
    queryFn: () => apiClient.get(`/projects/${id}`).then(r => r.data),
    enabled: !!id,
  });

  const { data: files } = useQuery<FileReference[]>({
    queryKey: ['project-files', id, page],
    queryFn: () =>
      apiClient.get(`/projects/${id}/files`, { params: { page, per_page: perPage } }).then(r => r.data),
    enabled: !!id,
  });

  const filteredFiles = files?.filter(
    f => !search || f.original_name.toLowerCase().includes(search.toLowerCase())
  ) ?? [];

  if (isLoading) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">Project</h2>
        <p className="mt-2 text-gray-500">Loading project...</p>
      </div>
    );
  }

  if (!projectData) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">Project</h2>
        <p className="mt-2 text-red-500">Project not found.</p>
      </div>
    );
  }

  const { project, stats } = projectData;
  const canWrite = isAdmin || project.owner_id === currentUser?.id;

  return (
    <div>
      <Link to="/projects" className="text-sm text-blue-600 hover:underline">&larr; Back to Projects</Link>
      <h2 className="mt-2 text-2xl font-semibold text-gray-800">{project.name}</h2>
      <p className="text-gray-500">
        {stats.file_count} files &middot; {formatBytes(stats.total_size)}
      </p>

      {canWrite && <ProjectSettingsForm project={project} />}

      {canWrite && <ProjectStoragesSection projectId={id!} />}

      {isAdmin && <ProjectMembersSection projectId={id!} ownerId={project.owner_id} />}

      {canWrite && <FileUploadZone projectId={id!} />}

      <div className="mt-6">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-medium text-gray-700">Files</h3>
          <div className="relative">
            <Search className="absolute left-2 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
            <input
              value={search}
              onChange={e => setSearch(e.target.value)}
              placeholder="Search files..."
              className="rounded border border-gray-300 py-1.5 pl-8 pr-3 text-sm"
            />
          </div>
        </div>

        {filteredFiles.length === 0 ? (
          <p className="mt-4 text-gray-400">No files found.</p>
        ) : (
          <div className="mt-3 overflow-hidden rounded-lg border border-gray-200">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Sync</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                  <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white">
                {filteredFiles.map(f => (
                  <FileRow key={f.id} fileRef={f} projectId={id!} canWrite={canWrite} />
                ))}
              </tbody>
            </table>
          </div>
        )}

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
            disabled={(files?.length ?? 0) < perPage}
            className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
          >
            Next <ChevronRight className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

function ProjectSettingsForm({ project }: { project: ProjectWithStats['project'] }) {
  const queryClient = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(project.name);
  const [slug, setSlug] = useState(project.slug);
  const [hotToCold, setHotToCold] = useState(
    project.hot_to_cold_days != null ? String(project.hot_to_cold_days) : ''
  );

  const mutation = useMutation({
    mutationFn: (data: { name?: string; slug?: string; hot_to_cold_days?: number | null }) =>
      apiClient.put(`/projects/${project.id}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project', project.id] });
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      setEditing(false);
    },
  });

  if (!editing) {
    return (
      <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
        <div className="flex items-center justify-between">
          <h3 className="font-medium text-gray-800">Settings</h3>
          <button onClick={() => setEditing(true)} className="text-sm text-blue-600 hover:underline">
            Edit
          </button>
        </div>
        <dl className="mt-2 grid grid-cols-3 gap-4 text-sm">
          <div>
            <dt className="text-gray-500">Name</dt>
            <dd className="text-gray-900">{project.name}</dd>
          </div>
          <div>
            <dt className="text-gray-500">Slug</dt>
            <dd className="text-gray-900">{project.slug}</dd>
          </div>
          <div>
            <dt className="text-gray-500">Hot-Cold Days</dt>
            <dd className="text-gray-900">{project.hot_to_cold_days ?? '\u2014'}</dd>
          </div>
        </dl>
      </div>
    );
  }

  return (
    <div className="mt-4 rounded-lg border border-blue-200 bg-white p-4">
      <h3 className="font-medium text-gray-800">Edit Settings</h3>
      <form
        onSubmit={e => {
          e.preventDefault();
          mutation.mutate({
            name: name !== project.name ? name : undefined,
            slug: slug !== project.slug ? slug : undefined,
            hot_to_cold_days: hotToCold ? parseInt(hotToCold, 10) : null,
          });
        }}
        className="mt-2 grid grid-cols-3 gap-4"
      >
        <div>
          <label className="block text-xs text-gray-500">Name</label>
          <input value={name} onChange={e => setName(e.target.value)} className="mt-1 w-full rounded border px-2 py-1 text-sm" />
        </div>
        <div>
          <label className="block text-xs text-gray-500">Slug</label>
          <input value={slug} onChange={e => setSlug(e.target.value)} className="mt-1 w-full rounded border px-2 py-1 text-sm" />
        </div>
        <div>
          <label className="block text-xs text-gray-500">Hot-Cold Days</label>
          <input value={hotToCold} onChange={e => setHotToCold(e.target.value)} type="number" min="1" className="mt-1 w-full rounded border px-2 py-1 text-sm" />
        </div>
        <div className="col-span-3 flex gap-2">
          <button type="submit" disabled={mutation.isPending} className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700 disabled:opacity-50">
            {mutation.isPending ? 'Saving...' : 'Save'}
          </button>
          <button type="button" onClick={() => setEditing(false)} className="rounded bg-gray-200 px-3 py-1 text-sm hover:bg-gray-300">
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}

const CLOUD_TYPES = ['s3', 'azure', 'gcs'];

function ProjectStoragesSection({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [editingAssignment, setEditingAssignment] = useState<ProjectStorageAssignment | null>(null);
  const [editContainer, setEditContainer] = useState('');
  const [editPrefix, setEditPrefix] = useState('');
  const [editActive, setEditActive] = useState(true);

  const { data: assignments } = useQuery<ProjectStorageAssignment[]>({
    queryKey: ['project-storages', projectId],
    queryFn: () => apiClient.get(`/projects/${projectId}/storages`).then(r => r.data),
  });

  const removeMutation = useMutation({
    mutationFn: (storageId: string) =>
      apiClient.delete(`/projects/${projectId}/storages/${storageId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-storages', projectId] });
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ storageId, data }: { storageId: string; data: { container_override?: string | null; prefix_override?: string | null; is_active?: boolean } }) =>
      apiClient.put(`/projects/${projectId}/storages/${storageId}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-storages', projectId] });
      setEditingAssignment(null);
    },
  });

  const startEdit = (a: ProjectStorageAssignment) => {
    setEditingAssignment(a);
    setEditContainer(a.container_override ?? '');
    setEditPrefix(a.prefix_override ?? '');
    setEditActive(a.is_active);
  };

  const submitEdit = () => {
    if (!editingAssignment) return;
    updateMutation.mutate({
      storageId: editingAssignment.storage_id,
      data: {
        container_override: editContainer || null,
        prefix_override: editPrefix || null,
        is_active: editActive,
      },
    });
  };

  return (
    <div className="mt-6">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-medium text-gray-700">Assigned Storages</h3>
        <button
          onClick={() => setShowAddDialog(true)}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> Add Storage
        </button>
      </div>

      {!assignments || assignments.length === 0 ? (
        <p className="mt-2 text-gray-400">No storages assigned.</p>
      ) : (
        <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Storage</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Type</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Tier</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Container Override</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Prefix Override</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Active</th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {assignments.map(a => (
                <tr key={a.id} className={!a.is_active ? 'opacity-50' : ''}>
                  <td className="px-4 py-2 text-sm text-gray-900">{a.storage_name}</td>
                  <td className="px-4 py-2 text-sm text-gray-500">{a.storage_type}</td>
                  <td className="px-4 py-2 text-sm">
                    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${a.is_hot ? 'bg-orange-50 text-orange-600' : 'bg-blue-50 text-blue-600'}`}>
                      {a.is_hot ? 'hot' : 'cold'}
                    </span>
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">{a.container_override ?? '\u2014'}</td>
                  <td className="px-4 py-2 text-sm text-gray-500">{a.prefix_override ?? '\u2014'}</td>
                  <td className="px-4 py-2 text-sm">
                    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${a.is_active ? 'bg-green-50 text-green-600' : 'bg-gray-100 text-gray-500'}`}>
                      {a.is_active ? 'yes' : 'no'}
                    </span>
                  </td>
                  <td className="whitespace-nowrap px-4 py-2 text-center">
                    <div className="flex items-center justify-center gap-1">
                      <button onClick={() => startEdit(a)} title="Edit" className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
                        <Pencil className="h-4 w-4" />
                      </button>
                      <button
                        onClick={() => { if (window.confirm('Remove this storage assignment?')) removeMutation.mutate(a.storage_id); }}
                        title="Remove"
                        className="rounded p-1 text-red-400 hover:bg-red-50 hover:text-red-600"
                      >
                        <Trash2 className="h-4 w-4" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {editingAssignment && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
            <div className="flex items-center justify-between">
              <h3 className="text-lg font-medium text-gray-800">Edit Assignment: {editingAssignment.storage_name}</h3>
              <button onClick={() => setEditingAssignment(null)} className="text-gray-400 hover:text-gray-600"><X className="h-5 w-5" /></button>
            </div>
            <div className="mt-4 space-y-3">
              <div>
                <label className="block text-xs text-gray-500">Container Override</label>
                <input value={editContainer} onChange={e => setEditContainer(e.target.value)} placeholder="Leave empty for default" className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm" />
              </div>
              <div>
                <label className="block text-xs text-gray-500">Prefix Override</label>
                <input value={editPrefix} onChange={e => setEditPrefix(e.target.value)} placeholder="Leave empty for default" className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm" />
              </div>
              <label className="flex items-center gap-2 text-sm text-gray-700">
                <input type="checkbox" checked={editActive} onChange={e => setEditActive(e.target.checked)} className="rounded border-gray-300" />
                Active
              </label>
            </div>
            <div className="mt-4 flex gap-2">
              <button onClick={submitEdit} disabled={updateMutation.isPending} className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50">
                {updateMutation.isPending ? 'Saving...' : 'Save'}
              </button>
              <button onClick={() => setEditingAssignment(null)} className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300">Cancel</button>
            </div>
          </div>
        </div>
      )}

      {showAddDialog && (
        <AddStorageDialog projectId={projectId} onClose={() => setShowAddDialog(false)} />
      )}
    </div>
  );
}

function ProjectMembersSection({ projectId, ownerId }: { projectId: string; ownerId: string | null }) {
  const queryClient = useQueryClient();
  const [showAdd, setShowAdd] = useState(false);

  const { data: members } = useQuery<AuthUser[]>({
    queryKey: ['project-members', projectId],
    queryFn: () => apiClient.get(`/projects/${projectId}/members`).then(r => r.data),
  });

  const { data: allUsers } = useQuery<AuthUser[]>({
    queryKey: ['all-users-for-project-members'],
    queryFn: () => apiClient.get('/auth/users').then(r => r.data),
    enabled: showAdd,
  });

  // Fetch owner user separately since they may not be in user_projects table
  const { data: ownerUserData } = useQuery<AuthUser>({
    queryKey: ['project-owner', ownerId],
    queryFn: () => apiClient.get(`/auth/users/${ownerId}`).then(r => r.data.user),
    enabled: !!ownerId,
  });

  const unassignedUsers = allUsers?.filter(
    u => !members?.some(m => m.id === u.id) && u.id !== ownerId,
  );

  const addMutation = useMutation({
    mutationFn: (userId: string) =>
      apiClient.post(`/projects/${projectId}/members`, { user_id: userId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-members', projectId] });
      setShowAdd(false);
    },
  });

  const removeMutation = useMutation({
    mutationFn: (userId: string) =>
      apiClient.delete(`/projects/${projectId}/members/${userId}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-members', projectId] });
    },
  });

  const nonOwnerMembers = members?.filter(m => m.id !== ownerId) ?? [];

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

      {(!members || (nonOwnerMembers.length === 0 && !ownerUserData)) ? (
        <p className="mt-2 text-gray-400">No members assigned.</p>
      ) : (
        <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Username</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Role</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {ownerUserData && (
                <tr>
                  <td className="px-4 py-2 text-sm">
                    <Link to={`/users/${ownerUserData.id}`} className="text-blue-600 hover:underline">{ownerUserData.username}</Link>
                  </td>
                  <td className="px-4 py-2 text-sm">
                    <span className="inline-flex rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800">Owner</span>
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">{new Date(ownerUserData.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-2 text-center text-sm text-gray-400">&mdash;</td>
                </tr>
              )}
              {nonOwnerMembers.map(m => (
                <tr key={m.id}>
                  <td className="px-4 py-2 text-sm">
                    <Link to={`/users/${m.id}`} className="text-blue-600 hover:underline">{m.username}</Link>
                  </td>
                  <td className="px-4 py-2 text-sm">
                    <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${m.role === 'admin' ? 'bg-purple-100 text-purple-800' : 'bg-gray-100 text-gray-800'}`}>
                      {m.role}
                    </span>
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
              <button onClick={() => setShowAdd(false)} className="text-gray-400 hover:text-gray-600"><X className="h-5 w-5" /></button>
            </div>
            {!unassignedUsers || unassignedUsers.length === 0 ? (
              <p className="mt-4 text-sm text-gray-400">No available users to add.</p>
            ) : (
              <AddMemberSelect
                users={unassignedUsers}
                onSelect={id => addMutation.mutate(id)}
                isPending={addMutation.isPending}
              />
            )}
            <div className="mt-4 flex gap-2">
              <button onClick={() => setShowAdd(false)} className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300">Cancel</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function AddMemberSelect({ users, onSelect, isPending }: { users: AuthUser[]; onSelect: (id: string) => void; isPending: boolean }) {
  const [selectedId, setSelectedId] = useState('');
  return (
    <div className="mt-4">
      <select
        value={selectedId}
        onChange={e => setSelectedId(e.target.value)}
        className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
      >
        <option value="">Select a user...</option>
        {users.map(u => (
          <option key={u.id} value={u.id}>{u.username} ({u.role})</option>
        ))}
      </select>
      <button
        onClick={() => onSelect(selectedId)}
        disabled={!selectedId || isPending}
        className="mt-2 rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
      >
        {isPending ? 'Adding...' : 'Add'}
      </button>
    </div>
  );
}

function AddStorageDialog({ projectId, onClose }: { projectId: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [selectedStorageId, setSelectedStorageId] = useState('');
  const [containerOverride, setContainerOverride] = useState('');
  const [prefixOverride, setPrefixOverride] = useState('');
  const [newContainerName, setNewContainerName] = useState('');
  const [creatingContainer, setCreatingContainer] = useState(false);

  const { data: availableStorages } = useQuery<StorageBackend[]>({
    queryKey: ['available-storages', projectId],
    queryFn: () => apiClient.get(`/projects/${projectId}/available-storages`).then(r => r.data),
  });

  const selectedStorage = availableStorages?.find(s => s.id === selectedStorageId);
  const isCloudType = selectedStorage && CLOUD_TYPES.includes(selectedStorage.storage_type);

  const { data: containers, refetch: refetchContainers } = useQuery<string[]>({
    queryKey: ['storage-containers', selectedStorageId],
    queryFn: () => apiClient.get(`/storages/${selectedStorageId}/containers`).then(r => r.data),
    enabled: !!selectedStorageId && !!isCloudType,
  });

  const assignMutation = useMutation({
    mutationFn: (data: { storage_id: string; container_override?: string | null; prefix_override?: string | null }) =>
      apiClient.post(`/projects/${projectId}/storages`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-storages', projectId] });
      queryClient.invalidateQueries({ queryKey: ['available-storages', projectId] });
      onClose();
    },
  });

  const handleCreateContainer = async () => {
    if (!newContainerName || !selectedStorageId) return;
    setCreatingContainer(true);
    try {
      await apiClient.post(`/storages/${selectedStorageId}/containers`, { name: newContainerName });
      setContainerOverride(newContainerName);
      setNewContainerName('');
      refetchContainers();
    } catch {
      // error handled by axios interceptor
    } finally {
      setCreatingContainer(false);
    }
  };

  const handleAssign = () => {
    if (!selectedStorageId) return;
    assignMutation.mutate({
      storage_id: selectedStorageId,
      container_override: containerOverride || null,
      prefix_override: prefixOverride || null,
    });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-full max-w-lg rounded-lg bg-white p-6 shadow-xl">
        <div className="flex items-center justify-between">
          <h3 className="text-lg font-medium text-gray-800">Add Storage to Project</h3>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600"><X className="h-5 w-5" /></button>
        </div>

        <div className="mt-4 space-y-4">
          <div>
            <label className="block text-xs font-medium text-gray-500">Storage</label>
            <select
              value={selectedStorageId}
              onChange={e => { setSelectedStorageId(e.target.value); setContainerOverride(''); }}
              className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              <option value="">Select a storage...</option>
              {availableStorages?.map(s => (
                <option key={s.id} value={s.id}>
                  {s.name} ({s.storage_type}, {s.is_hot ? 'hot' : 'cold'})
                </option>
              ))}
            </select>
          </div>

          {isCloudType && (
            <div>
              <label className="block text-xs font-medium text-gray-500">Container / Bucket Override</label>
              <select
                value={containerOverride}
                onChange={e => setContainerOverride(e.target.value)}
                className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              >
                <option value="">Default (from storage config)</option>
                {containers?.map(c => (
                  <option key={c} value={c}>{c}</option>
                ))}
              </select>
              <div className="mt-2 flex gap-2">
                <input
                  value={newContainerName}
                  onChange={e => setNewContainerName(e.target.value)}
                  placeholder="New container name..."
                  className="flex-1 rounded border border-gray-300 px-3 py-1.5 text-sm"
                />
                <button
                  onClick={handleCreateContainer}
                  disabled={!newContainerName || creatingContainer}
                  className="rounded bg-green-600 px-3 py-1.5 text-sm text-white hover:bg-green-700 disabled:opacity-50"
                >
                  {creatingContainer ? 'Creating...' : 'Create'}
                </button>
              </div>
            </div>
          )}

          <div>
            <label className="block text-xs font-medium text-gray-500">Prefix Override</label>
            <input
              value={prefixOverride}
              onChange={e => setPrefixOverride(e.target.value)}
              placeholder="e.g. project-files/ (optional)"
              className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
        </div>

        <div className="mt-5 flex gap-2">
          <button
            onClick={handleAssign}
            disabled={!selectedStorageId || assignMutation.isPending}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {assignMutation.isPending ? 'Assigning...' : 'Assign Storage'}
          </button>
          <button onClick={onClose} className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300">Cancel</button>
        </div>
      </div>
    </div>
  );
}

function FileUploadZone({ projectId }: { projectId: string }) {
  const queryClient = useQueryClient();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [uploadStatus, setUploadStatus] = useState<string | null>(null);

  const handleUpload = useCallback(async (fileList: FileList) => {
    if (fileList.length === 0) return;
    setUploading(true);
    setUploadStatus(null);

    let successCount = 0;
    for (const file of Array.from(fileList)) {
      const formData = new FormData();
      formData.append('file', file);
      try {
        await apiClient.post(`/projects/${projectId}/files`, formData, {
          headers: { 'Content-Type': 'multipart/form-data' },
        });
        successCount++;
      } catch {
        setUploadStatus(`Failed to upload ${file.name}`);
      }
    }

    if (successCount > 0) {
      setUploadStatus(`Uploaded ${successCount} file(s)`);
      queryClient.invalidateQueries({ queryKey: ['project-files', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
    }
    setUploading(false);
  }, [projectId, queryClient]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback(() => setIsDragging(false), []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    if (e.dataTransfer.files.length > 0) {
      handleUpload(e.dataTransfer.files);
    }
  }, [handleUpload]);

  return (
    <div className="mt-6">
      <h3 className="text-lg font-medium text-gray-700">Upload Files</h3>
      <div
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
        onClick={() => fileInputRef.current?.click()}
        className={`mt-2 flex cursor-pointer flex-col items-center justify-center rounded-lg border-2 border-dashed p-6 transition ${
          isDragging ? 'border-blue-500 bg-blue-50' : 'border-gray-300 hover:border-gray-400'
        }`}
        role="button"
        aria-label="Upload files"
      >
        <Upload className="h-8 w-8 text-gray-400" />
        <p className="mt-2 text-sm text-gray-500">
          {uploading ? 'Uploading...' : 'Drag and drop files here, or click to browse'}
        </p>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={e => e.target.files && handleUpload(e.target.files)}
        />
      </div>
      {uploadStatus && (
        <p className={`mt-2 text-sm ${uploadStatus.includes('Failed') ? 'text-red-600' : 'text-green-600'}`}>
          {uploadStatus}
        </p>
      )}
    </div>
  );
}

function FileRow({ fileRef, projectId, canWrite }: { fileRef: FileReference; projectId: string; canWrite: boolean }) {
  const queryClient = useQueryClient();
  const [copiedLink, setCopiedLink] = useState(false);

  const deleteMutation = useMutation({
    mutationFn: () => apiClient.delete(`/files/${fileRef.file_id}`, { params: { project_id: projectId } }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-files', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
    },
  });

  const handleDownload = async () => {
    try {
      const res = await apiClient.get<TempLinkResponse>(`/files/${fileRef.file_id}/link`);
      window.open(res.data.url, '_blank');
    } catch {
      // error handled by axios interceptor
    }
  };

  const handleGetLink = async () => {
    try {
      const res = await apiClient.get<TempLinkResponse>(`/files/${fileRef.file_id}/link`);
      await navigator.clipboard.writeText(window.location.origin + res.data.url);
      setCopiedLink(true);
      setTimeout(() => setCopiedLink(false), 2000);
    } catch {
      // error handled by axios interceptor
    }
  };

  const handleRestore = async () => {
    try {
      await apiClient.post(`/files/${fileRef.file_id}/restore`);
    } catch {
      // error handled by axios interceptor
    }
  };

  const syncLabel = fileRef.sync_status === 'synced'
    ? `${fileRef.synced_storages}/${fileRef.total_storages}`
    : fileRef.sync_status === 'partial'
      ? `${fileRef.synced_storages}/${fileRef.total_storages}`
      : fileRef.sync_status === 'pending'
        ? `0/${fileRef.total_storages}`
        : '--';

  const syncColor = fileRef.sync_status === 'synced'
    ? 'text-green-600 bg-green-50'
    : fileRef.sync_status === 'partial'
      ? 'text-yellow-600 bg-yellow-50'
      : 'text-red-600 bg-red-50';

  return (
    <tr>
      <td className="px-4 py-2 text-sm text-gray-900">{fileRef.original_name}</td>
      <td className="whitespace-nowrap px-4 py-2 text-sm">
        <span className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${syncColor}`}>
          {syncLabel}
        </span>
      </td>
      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
        {new Date(fileRef.created_at).toLocaleDateString()}
      </td>
      <td className="whitespace-nowrap px-4 py-2 text-center">
        <div className="flex items-center justify-center gap-1">
          <button onClick={handleDownload} title="Download" className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
            <Download className="h-4 w-4" />
          </button>
          <button onClick={handleGetLink} title="Copy temp link" className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
            {copiedLink ? <Check className="h-4 w-4 text-green-600" /> : <Link2 className="h-4 w-4" />}
          </button>
          {canWrite && (
            <button onClick={handleRestore} title="Restore from cold" className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
              <ArchiveRestore className="h-4 w-4" />
            </button>
          )}
          {canWrite && (
            <button
              onClick={() => { if (window.confirm('Delete this file reference?')) deleteMutation.mutate(); }}
              title="Delete"
              className="rounded p-1 text-red-400 hover:bg-red-50 hover:text-red-600"
            >
              <Trash2 className="h-4 w-4" />
            </button>
          )}
        </div>
      </td>
    </tr>
  );
}
