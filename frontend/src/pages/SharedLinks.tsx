import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  Copy, XCircle, Plus, Check, Lock, Globe,
} from 'lucide-react';
import apiClient from '../api/client';
import {
  SharedLink, CreateSharedLinkRequest, FileReference,
} from '../api/types';

export default function SharedLinks() {
  const { id: projectId } = useParams<{ id: string }>();
  const queryClient = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [copiedToken, setCopiedToken] = useState<string | null>(null);
  const [error, setError] = useState('');

  // Form state
  const [selectedFileId, setSelectedFileId] = useState('');
  const [password, setPassword] = useState('');
  const [expiresHours, setExpiresHours] = useState('');
  const [maxDownloads, setMaxDownloads] = useState('');

  const { data: links, isLoading } = useQuery<SharedLink[]>({
    queryKey: ['shared-links', projectId],
    queryFn: () =>
      apiClient.get(`/projects/${projectId}/shared-links`).then(r => r.data),
    enabled: !!projectId,
  });

  const { data: files } = useQuery<FileReference[]>({
    queryKey: ['project-files-all', projectId],
    queryFn: () =>
      apiClient.get(`/projects/${projectId}/files`, { params: { per_page: 1000 } }).then(r => r.data),
    enabled: !!projectId && showCreate,
  });

  const createMutation = useMutation({
    mutationFn: (req: CreateSharedLinkRequest) =>
      apiClient.post(`/projects/${projectId}/shared-links`, req).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['shared-links', projectId] });
      setShowCreate(false);
      resetForm();
      setError('');
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to create link';
      setError(msg);
    },
  });

  const deactivateMutation = useMutation({
    mutationFn: (linkId: string) =>
      apiClient.delete(`/shared-links/${linkId}`).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['shared-links', projectId] });
    },
  });

  function resetForm() {
    setSelectedFileId('');
    setPassword('');
    setExpiresHours('');
    setMaxDownloads('');
  }

  function handleCreate() {
    if (!selectedFileId) {
      setError('Please select a file');
      return;
    }
    const req: CreateSharedLinkRequest = {
      file_id: selectedFileId,
    };
    if (password) req.password = password;
    if (expiresHours) {
      const parsed = parseFloat(expiresHours);
      if (isNaN(parsed) || parsed <= 0) {
        setError('Expiration hours must be a positive number');
        return;
      }
      req.expires_in_seconds = Math.round(parsed * 3600);
    }
    if (maxDownloads) {
      const parsed = parseInt(maxDownloads, 10);
      if (isNaN(parsed) || parsed <= 0) {
        setError('Max downloads must be a positive number');
        return;
      }
      req.max_downloads = parsed;
    }
    createMutation.mutate(req);
  }

  function copyLink(token: string) {
    navigator.clipboard.writeText(getLinkUrl(token)).then(() => {
      setCopiedToken(token);
      setTimeout(() => setCopiedToken(null), 2000);
    });
  }

  function getLinkUrl(token: string) {
    return `${window.location.origin}/share/${token}`;
  }

  return (
    <div>
      <Link to={`/projects/${projectId}`} className="text-sm text-blue-600 hover:underline">
        &larr; Back to Project
      </Link>
      <div className="mt-2 flex items-center justify-between">
        <h2 className="text-2xl font-semibold text-gray-800">Shared Links</h2>
        <button
          onClick={() => { setShowCreate(!showCreate); setError(''); }}
          className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700"
          data-testid="create-link-button"
        >
          <Plus className="h-4 w-4" /> Create Link
        </button>
      </div>

      {showCreate && (
        <div className="mt-4 rounded border border-gray-200 bg-white p-4" data-testid="create-form">
          <h3 className="text-sm font-medium text-gray-700 mb-3">Create Shared Link</h3>

          <div className="space-y-3">
            <div>
              <label className="block text-xs text-gray-500 mb-1">File</label>
              <select
                value={selectedFileId}
                onChange={e => setSelectedFileId(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                data-testid="file-select"
              >
                <option value="">Select a file...</option>
                {files?.map(f => (
                  <option key={f.file_id} value={f.file_id}>
                    {f.original_name}
                  </option>
                ))}
              </select>
            </div>

            <div className="grid grid-cols-3 gap-3">
              <div>
                <label className="block text-xs text-gray-500 mb-1">Password (optional)</label>
                <input
                  type="password"
                  value={password}
                  onChange={e => setPassword(e.target.value)}
                  placeholder="Leave empty for public"
                  className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                  data-testid="password-input"
                />
              </div>
              <div>
                <label className="block text-xs text-gray-500 mb-1">Expires in (hours)</label>
                <input
                  type="number"
                  value={expiresHours}
                  onChange={e => setExpiresHours(e.target.value)}
                  placeholder="No expiration"
                  min="0.01"
                  className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                  data-testid="expires-input"
                />
              </div>
              <div>
                <label className="block text-xs text-gray-500 mb-1">Max downloads</label>
                <input
                  type="number"
                  value={maxDownloads}
                  onChange={e => setMaxDownloads(e.target.value)}
                  placeholder="Unlimited"
                  min="1"
                  className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                  data-testid="max-downloads-input"
                />
              </div>
            </div>

            {error && (
              <p className="text-sm text-red-600" data-testid="error-message">{error}</p>
            )}

            <div className="flex gap-2">
              <button
                onClick={handleCreate}
                disabled={createMutation.isPending}
                className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
                data-testid="submit-create"
              >
                {createMutation.isPending ? 'Creating...' : 'Create'}
              </button>
              <button
                onClick={() => { setShowCreate(false); resetForm(); setError(''); }}
                className="rounded bg-gray-200 px-4 py-1.5 text-sm text-gray-700 hover:bg-gray-300"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {isLoading && <p className="mt-4 text-gray-500">Loading shared links...</p>}

      {links && links.length === 0 && (
        <p className="mt-4 text-gray-500" data-testid="empty-state">No shared links yet.</p>
      )}

      {links && links.length > 0 && (
        <div className="mt-4 overflow-x-auto">
          <table className="min-w-full divide-y divide-gray-200 bg-white" data-testid="links-table">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">File</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Link</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Type</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Expires</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Downloads</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Actions</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200">
              {links.map(link => (
                <tr key={link.id} data-testid="link-row">
                  <td className="px-4 py-2 text-sm text-gray-800">{link.original_name}</td>
                  <td className="px-4 py-2 text-sm">
                    <div className="flex items-center gap-1">
                      <code className="text-xs text-gray-500 bg-gray-100 px-1 py-0.5 rounded max-w-[200px] truncate">
                        {getLinkUrl(link.token)}
                      </code>
                      <button
                        onClick={() => copyLink(link.token)}
                        className="p-1 text-gray-400 hover:text-blue-600"
                        title="Copy link"
                        data-testid={`copy-link-${link.token}`}
                      >
                        {copiedToken === link.token ? (
                          <Check className="h-3.5 w-3.5 text-green-600" />
                        ) : (
                          <Copy className="h-3.5 w-3.5" />
                        )}
                      </button>
                    </div>
                  </td>
                  <td className="px-4 py-2 text-sm">
                    {link.password_protected ? (
                      <span className="inline-flex items-center gap-1 text-orange-600">
                        <Lock className="h-3.5 w-3.5" /> Protected
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1 text-green-600">
                        <Globe className="h-3.5 w-3.5" /> Public
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">
                    {new Date(link.created_at).toLocaleDateString()}
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-500">
                    {link.expires_at
                      ? new Date(link.expires_at).toLocaleString()
                      : 'Never'}
                  </td>
                  <td className="px-4 py-2 text-sm text-gray-800">
                    {link.download_count}
                    {link.max_downloads != null && (
                      <span className="text-gray-400"> / {link.max_downloads}</span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-sm">
                    {link.is_active ? (
                      <span className="inline-block rounded-full bg-green-100 px-2 py-0.5 text-xs text-green-700">
                        Active
                      </span>
                    ) : (
                      <span className="inline-block rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-500">
                        Inactive
                      </span>
                    )}
                  </td>
                  <td className="px-4 py-2 text-sm">
                    <div className="flex gap-1">
                      {link.is_active && (
                        <button
                          onClick={() => deactivateMutation.mutate(link.id)}
                          className="p-1 text-gray-400 hover:text-orange-600"
                          title="Deactivate"
                          data-testid={`deactivate-${link.id}`}
                        >
                          <XCircle className="h-4 w-4" />
                        </button>
                      )}
                    </div>
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
