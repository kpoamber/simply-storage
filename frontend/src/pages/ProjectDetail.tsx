import { useState, useCallback, useRef } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Upload, Download, Link2, ArchiveRestore, Trash2,
  ChevronLeft, ChevronRight, Search, Check,
} from 'lucide-react';
import apiClient from '../api/client';
import { ProjectWithStats, FileReference, TempLinkResponse, StorageBackend, formatBytes } from '../api/types';

export default function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
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

  const { data: storages } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  const assignedStorages = storages?.filter(
    s => s.project_id === id || s.project_id === null
  ) ?? [];

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

  return (
    <div>
      <Link to="/projects" className="text-sm text-blue-600 hover:underline">&larr; Back to Projects</Link>
      <h2 className="mt-2 text-2xl font-semibold text-gray-800">{project.name}</h2>
      <p className="text-gray-500">
        {stats.file_count} files &middot; {formatBytes(stats.total_size)}
      </p>

      <ProjectSettingsForm project={project} />

      <div className="mt-6">
        <h3 className="text-lg font-medium text-gray-700">Assigned Storages</h3>
        {assignedStorages.length === 0 ? (
          <p className="mt-2 text-gray-400">No storages assigned.</p>
        ) : (
          <div className="mt-2 flex flex-wrap gap-2">
            {assignedStorages.map(s => (
              <span key={s.id} className="rounded bg-gray-100 px-2 py-1 text-sm text-gray-700">
                {s.name} ({s.storage_type}, {s.is_hot ? 'hot' : 'cold'})
              </span>
            ))}
          </div>
        )}
      </div>

      <FileUploadZone projectId={id!} />

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
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                  <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white">
                {filteredFiles.map(f => (
                  <FileRow key={f.id} fileRef={f} projectId={id!} />
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

function FileRow({ fileRef, projectId }: { fileRef: FileReference; projectId: string }) {
  const queryClient = useQueryClient();
  const [copiedLink, setCopiedLink] = useState(false);

  const deleteMutation = useMutation({
    mutationFn: () => apiClient.delete(`/files/${fileRef.file_id}`, { params: { project_id: projectId } }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['project-files', projectId] });
      queryClient.invalidateQueries({ queryKey: ['project', projectId] });
    },
  });

  const handleDownload = () => {
    window.open(`/api/files/${fileRef.file_id}/download`, '_blank');
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

  return (
    <tr>
      <td className="px-4 py-2 text-sm text-gray-900">{fileRef.original_name}</td>
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
          <button onClick={handleRestore} title="Restore from cold" className="rounded p-1 text-gray-500 hover:bg-gray-100 hover:text-gray-700">
            <ArchiveRestore className="h-4 w-4" />
          </button>
          <button
            onClick={() => { if (window.confirm('Delete this file reference?')) deleteMutation.mutate(); }}
            title="Delete"
            className="rounded p-1 text-red-400 hover:bg-red-50 hover:text-red-600"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </td>
    </tr>
  );
}
