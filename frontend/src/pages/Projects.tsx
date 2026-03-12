import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Eye, Pencil, Plus, X } from 'lucide-react';
import apiClient from '../api/client';
import { Project, ProjectWithStats, formatBytes } from '../api/types';

export default function Projects() {
  const queryClient = useQueryClient();
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  const { data: projects, isLoading } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: () => apiClient.get('/projects').then(r => r.data),
  });

  const projectIds = projects?.map(p => p.id) ?? [];
  const { data: projectDetails } = useQuery<Record<string, ProjectWithStats>>({
    queryKey: ['project-details', projectIds],
    queryFn: async () => {
      const details: Record<string, ProjectWithStats> = {};
      await Promise.all(
        projectIds.map(async (id) => {
          const res = await apiClient.get(`/projects/${id}`);
          details[id] = res.data;
        })
      );
      return details;
    },
    enabled: projectIds.length > 0,
  });

  const createMutation = useMutation({
    mutationFn: (data: { name: string; slug: string; hot_to_cold_days: number | null }) =>
      apiClient.post('/projects', data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      setShowCreateForm(false);
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, data }: { id: string; data: { name?: string; slug?: string; hot_to_cold_days?: number | null } }) =>
      apiClient.put(`/projects/${id}`, data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['projects'] });
      queryClient.invalidateQueries({ queryKey: ['project-details'] });
      setEditingId(null);
    },
  });

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-semibold text-gray-800">Projects</h2>
          <p className="mt-1 text-gray-500">Manage storage projects.</p>
        </div>
        <button
          onClick={() => setShowCreateForm(true)}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-2 text-sm text-white hover:bg-blue-700"
        >
          <Plus className="h-4 w-4" /> New Project
        </button>
      </div>

      {showCreateForm && (
        <CreateProjectForm
          onSubmit={(data) => createMutation.mutate(data)}
          onCancel={() => setShowCreateForm(false)}
          isLoading={createMutation.isPending}
        />
      )}

      {isLoading ? (
        <p className="mt-6 text-gray-400">Loading projects...</p>
      ) : !projects?.length ? (
        <p className="mt-6 text-gray-400">No projects yet.</p>
      ) : (
        <div className="mt-6 overflow-hidden rounded-lg border border-gray-200">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Slug</th>
                <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Files</th>
                <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Storage</th>
                <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Hot-Cold</th>
                <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 bg-white">
              {projects.map(project => {
                const detail = projectDetails?.[project.id];
                return editingId === project.id ? (
                  <EditProjectRow
                    key={project.id}
                    project={project}
                    onSubmit={(data) => updateMutation.mutate({ id: project.id, data })}
                    onCancel={() => setEditingId(null)}
                    isLoading={updateMutation.isPending}
                  />
                ) : (
                  <tr key={project.id}>
                    <td className="whitespace-nowrap px-4 py-2 text-sm font-medium text-gray-900">{project.name}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">{project.slug}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">
                      {detail ? detail.stats.file_count : '...'}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">
                      {detail ? formatBytes(detail.stats.total_size) : '...'}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-500">
                      {project.hot_to_cold_days != null ? `${project.hot_to_cold_days}d` : '\u2014'}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-center">
                      <div className="flex items-center justify-center gap-2">
                        <Link
                          to={`/projects/${project.id}`}
                          className="text-blue-600 hover:text-blue-800"
                          title="View"
                        >
                          <Eye className="h-4 w-4" />
                        </Link>
                        <button
                          onClick={() => setEditingId(project.id)}
                          className="text-gray-500 hover:text-gray-700"
                          title="Edit"
                        >
                          <Pencil className="h-4 w-4" />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function CreateProjectForm({
  onSubmit,
  onCancel,
  isLoading,
}: {
  onSubmit: (data: { name: string; slug: string; hot_to_cold_days: number | null }) => void;
  onCancel: () => void;
  isLoading: boolean;
}) {
  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [hotToCold, setHotToCold] = useState('');

  return (
    <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="font-medium text-gray-800">Create Project</h3>
        <button onClick={onCancel} className="text-gray-400 hover:text-gray-600">
          <X className="h-4 w-4" />
        </button>
      </div>
      <form
        onSubmit={e => {
          e.preventDefault();
          onSubmit({
            name,
            slug,
            hot_to_cold_days: hotToCold ? parseInt(hotToCold, 10) : null,
          });
        }}
        className="flex flex-wrap gap-3"
      >
        <input
          value={name}
          onChange={e => setName(e.target.value)}
          placeholder="Project name"
          required
          className="rounded border border-gray-300 px-3 py-1.5 text-sm"
        />
        <input
          value={slug}
          onChange={e => setSlug(e.target.value)}
          placeholder="slug"
          required
          className="rounded border border-gray-300 px-3 py-1.5 text-sm"
        />
        <input
          value={hotToCold}
          onChange={e => setHotToCold(e.target.value)}
          placeholder="Hot-Cold (days)"
          type="number"
          min="1"
          className="w-36 rounded border border-gray-300 px-3 py-1.5 text-sm"
        />
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

function EditProjectRow({
  project,
  onSubmit,
  onCancel,
  isLoading,
}: {
  project: Project;
  onSubmit: (data: { name?: string; slug?: string; hot_to_cold_days?: number | null }) => void;
  onCancel: () => void;
  isLoading: boolean;
}) {
  const [name, setName] = useState(project.name);
  const [slug, setSlug] = useState(project.slug);
  const [hotToCold, setHotToCold] = useState(
    project.hot_to_cold_days != null ? String(project.hot_to_cold_days) : ''
  );

  return (
    <tr>
      <td className="px-4 py-2">
        <input value={name} onChange={e => setName(e.target.value)} className="w-full rounded border px-2 py-1 text-sm" />
      </td>
      <td className="px-4 py-2">
        <input value={slug} onChange={e => setSlug(e.target.value)} className="w-full rounded border px-2 py-1 text-sm" />
      </td>
      <td className="px-4 py-2" colSpan={2}></td>
      <td className="px-4 py-2">
        <input value={hotToCold} onChange={e => setHotToCold(e.target.value)} type="number" min="1" className="w-20 rounded border px-2 py-1 text-sm" />
      </td>
      <td className="px-4 py-2 text-center">
        <div className="flex items-center justify-center gap-2">
          <button
            onClick={() => onSubmit({
              name: name !== project.name ? name : undefined,
              slug: slug !== project.slug ? slug : undefined,
              hot_to_cold_days: hotToCold ? parseInt(hotToCold, 10) : null,
            })}
            disabled={isLoading}
            className="rounded bg-green-600 px-2 py-1 text-xs text-white hover:bg-green-700"
          >
            Save
          </button>
          <button onClick={onCancel} className="rounded bg-gray-200 px-2 py-1 text-xs hover:bg-gray-300">
            Cancel
          </button>
        </div>
      </td>
    </tr>
  );
}
