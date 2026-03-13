import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useMutation } from '@tanstack/react-query';
import { Plus, X, Trash2, Eye, AlertTriangle } from 'lucide-react';
import { bulkDeletePreview, bulkDeleteExecute } from '../api/client';
import {
  BulkDeleteRequest, BulkDeletePreview, BulkDeleteResult,
  MetadataFilterNode, formatBytes,
} from '../api/types';

type FilterMode = 'and' | 'or' | 'not';

interface FilterRow {
  key: string;
  value: string;
  mode: FilterMode;
}

function buildFilterNode(rows: FilterRow[]): MetadataFilterNode | undefined {
  const validRows = rows.filter(r => r.key.trim() && r.value.trim());
  if (validRows.length === 0) return undefined;

  const nodes: MetadataFilterNode[] = validRows.map(r => {
    const leaf: MetadataFilterNode = { key: r.key.trim(), value: r.value.trim() };
    if (r.mode === 'not') return { not: leaf };
    return leaf;
  });

  const andNodes = nodes.filter((_, i) => validRows[i].mode !== 'or');
  const orNodes = nodes.filter((_, i) => validRows[i].mode === 'or');

  if (orNodes.length > 0 && andNodes.length > 0) {
    const andPart: MetadataFilterNode = andNodes.length === 1 ? andNodes[0] : { and: andNodes };
    return { or: [andPart, ...orNodes] };
  }
  if (orNodes.length > 0) {
    return orNodes.length === 1 ? orNodes[0] : { or: orNodes };
  }
  return andNodes.length === 1 ? andNodes[0] : { and: andNodes };
}

function buildRequest(
  metadataRows: FilterRow[],
  createdBefore: string,
  createdAfter: string,
  lastAccessedBefore: string,
  sizeMin: string,
  sizeMax: string,
): BulkDeleteRequest {
  const req: BulkDeleteRequest = {};
  const metaFilter = buildFilterNode(metadataRows);
  if (metaFilter) req.metadata_filters = metaFilter;
  if (createdBefore) req.created_before = new Date(createdBefore).toISOString();
  if (createdAfter) req.created_after = new Date(createdAfter).toISOString();
  if (lastAccessedBefore) req.last_accessed_before = new Date(lastAccessedBefore).toISOString();
  if (sizeMin) req.size_min = parseInt(sizeMin, 10);
  if (sizeMax) req.size_max = parseInt(sizeMax, 10);
  return req;
}

function hasAnyFilter(req: BulkDeleteRequest): boolean {
  return !!(
    req.metadata_filters ||
    req.created_before ||
    req.created_after ||
    req.last_accessed_before ||
    req.size_min ||
    req.size_max
  );
}

export default function ProjectBulkDelete() {
  const { id } = useParams<{ id: string }>();
  const [metadataRows, setMetadataRows] = useState<FilterRow[]>([]);
  const [createdBefore, setCreatedBefore] = useState('');
  const [createdAfter, setCreatedAfter] = useState('');
  const [lastAccessedBefore, setLastAccessedBefore] = useState('');
  const [sizeMin, setSizeMin] = useState('');
  const [sizeMax, setSizeMax] = useState('');
  const [preview, setPreview] = useState<BulkDeletePreview | null>(null);
  const [result, setResult] = useState<BulkDeleteResult | null>(null);
  const [showConfirm, setShowConfirm] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const previewMutation = useMutation({
    mutationFn: (req: BulkDeleteRequest) => bulkDeletePreview(id!, req),
    onSuccess: (res) => {
      setPreview(res.data);
      setResult(null);
      setError(null);
    },
    onError: (err: { response?: { data?: { error?: string } } }) => {
      setError(err.response?.data?.error || 'Preview failed');
      setPreview(null);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (req: BulkDeleteRequest) => bulkDeleteExecute(id!, req),
    onSuccess: (res) => {
      setResult(res.data);
      setPreview(null);
      setShowConfirm(false);
      setError(null);
    },
    onError: (err: { response?: { data?: { error?: string } } }) => {
      setError(err.response?.data?.error || 'Deletion failed');
      setShowConfirm(false);
    },
  });

  const addMetadataRow = () =>
    setMetadataRows(prev => [...prev, { key: '', value: '', mode: 'and' }]);

  const removeMetadataRow = (index: number) =>
    setMetadataRows(prev => prev.filter((_, i) => i !== index));

  const updateMetadataRow = (index: number, field: keyof FilterRow, val: string) =>
    setMetadataRows(prev =>
      prev.map((row, i) => (i === index ? { ...row, [field]: val } : row)),
    );

  const handlePreview = () => {
    setError(null);
    const req = buildRequest(metadataRows, createdBefore, createdAfter, lastAccessedBefore, sizeMin, sizeMax);
    if (!hasAnyFilter(req)) {
      setError('At least one filter is required');
      return;
    }
    previewMutation.mutate(req);
  };

  const handleDelete = () => {
    const req = buildRequest(metadataRows, createdBefore, createdAfter, lastAccessedBefore, sizeMin, sizeMax);
    deleteMutation.mutate(req);
  };

  return (
    <div>
      <Link to={`/projects/${id}`} className="text-sm text-blue-600 hover:underline">
        &larr; Back to Project
      </Link>
      <h2 className="mt-2 text-2xl font-semibold text-gray-800">Bulk Delete Files</h2>

      {/* Filter Form */}
      <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
        <h3 className="font-medium text-gray-700">Filters</h3>

        {/* Date Range */}
        <div className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-3">
          <div>
            <label className="block text-xs text-gray-500">Created After</label>
            <input
              type="datetime-local"
              value={createdAfter}
              onChange={e => setCreatedAfter(e.target.value)}
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="created-after"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500">Created Before</label>
            <input
              type="datetime-local"
              value={createdBefore}
              onChange={e => setCreatedBefore(e.target.value)}
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="created-before"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500">Last Accessed Before</label>
            <input
              type="datetime-local"
              value={lastAccessedBefore}
              onChange={e => setLastAccessedBefore(e.target.value)}
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="last-accessed-before"
            />
          </div>
        </div>

        {/* Size Range */}
        <div className="mt-3 grid grid-cols-2 gap-4">
          <div>
            <label className="block text-xs text-gray-500">Min Size (bytes)</label>
            <input
              type="number"
              min="0"
              value={sizeMin}
              onChange={e => setSizeMin(e.target.value)}
              placeholder="e.g. 1048576"
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="size-min"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500">Max Size (bytes)</label>
            <input
              type="number"
              min="0"
              value={sizeMax}
              onChange={e => setSizeMax(e.target.value)}
              placeholder="e.g. 10485760"
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="size-max"
            />
          </div>
        </div>

        {/* Metadata Filters */}
        <div className="mt-4">
          <div className="flex items-center justify-between">
            <span className="text-sm text-gray-600">Metadata Filters</span>
            <button
              onClick={addMetadataRow}
              className="flex items-center gap-1 text-sm text-blue-600 hover:text-blue-700"
              data-testid="add-metadata-filter"
            >
              <Plus className="h-3 w-3" /> Add filter
            </button>
          </div>
          {metadataRows.map((row, idx) => (
            <div key={idx} className="mt-1 flex items-center gap-2" data-testid="metadata-filter-row">
              <select
                value={row.mode}
                onChange={e => updateMetadataRow(idx, 'mode', e.target.value)}
                className="rounded border border-gray-300 px-2 py-1.5 text-sm"
              >
                <option value="and">AND</option>
                <option value="or">OR</option>
                <option value="not">NOT</option>
              </select>
              <input
                value={row.key}
                onChange={e => updateMetadataRow(idx, 'key', e.target.value)}
                placeholder="Key"
                className="w-1/3 rounded border border-gray-300 px-2 py-1.5 text-sm"
              />
              <input
                value={row.value}
                onChange={e => updateMetadataRow(idx, 'value', e.target.value)}
                placeholder="Value"
                className="flex-1 rounded border border-gray-300 px-2 py-1.5 text-sm"
              />
              <button
                onClick={() => removeMetadataRow(idx)}
                className="rounded p-1 text-red-400 hover:bg-red-50 hover:text-red-600"
                data-testid="remove-metadata-filter"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>

        {/* Actions */}
        <div className="mt-4 flex gap-2">
          <button
            onClick={handlePreview}
            disabled={previewMutation.isPending}
            className="flex items-center gap-2 rounded bg-gray-600 px-4 py-1.5 text-sm text-white hover:bg-gray-700 disabled:opacity-50"
            data-testid="preview-button"
          >
            <Eye className="h-4 w-4" />
            {previewMutation.isPending ? 'Loading...' : 'Preview'}
          </button>
          <button
            onClick={() => {
              const req = buildRequest(metadataRows, createdBefore, createdAfter, lastAccessedBefore, sizeMin, sizeMax);
              if (!hasAnyFilter(req)) {
                setError('At least one filter is required');
                return;
              }
              setShowConfirm(true);
            }}
            disabled={deleteMutation.isPending}
            className="flex items-center gap-2 rounded bg-red-600 px-4 py-1.5 text-sm text-white hover:bg-red-700 disabled:opacity-50"
            data-testid="delete-button"
          >
            <Trash2 className="h-4 w-4" />
            Delete Matching Files
          </button>
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="mt-4 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700" data-testid="error-message">
          {error}
        </div>
      )}

      {/* Preview Result */}
      {preview && (
        <div className="mt-4 rounded-lg border border-yellow-200 bg-yellow-50 p-4" data-testid="preview-result">
          <h3 className="flex items-center gap-2 font-medium text-yellow-800">
            <AlertTriangle className="h-4 w-4" />
            Preview Result
          </h3>
          <div className="mt-2 grid grid-cols-2 gap-4 text-sm">
            <div>
              <span className="text-yellow-700">Matching files</span>
              <p className="text-lg font-semibold text-yellow-900">{preview.matching_references}</p>
            </div>
            <div>
              <span className="text-yellow-700">Total size</span>
              <p className="text-lg font-semibold text-yellow-900">{formatBytes(preview.total_size)}</p>
            </div>
          </div>
        </div>
      )}

      {/* Delete Result */}
      {result && (
        <div className="mt-4 rounded-lg border border-green-200 bg-green-50 p-4" data-testid="delete-result">
          <h3 className="font-medium text-green-800">Deletion Complete</h3>
          <div className="mt-2 grid grid-cols-3 gap-4 text-sm">
            <div>
              <span className="text-green-700">Deleted references</span>
              <p className="text-lg font-semibold text-green-900">{result.deleted_references}</p>
            </div>
            <div>
              <span className="text-green-700">Orphans cleaned</span>
              <p className="text-lg font-semibold text-green-900">{result.orphaned_files_cleaned}</p>
            </div>
            <div>
              <span className="text-green-700">Freed space</span>
              <p className="text-lg font-semibold text-green-900">{formatBytes(result.freed_bytes)}</p>
            </div>
          </div>
        </div>
      )}

      {/* Confirmation Dialog */}
      {showConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40" data-testid="confirm-dialog">
          <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl">
            <div className="flex items-center gap-2 text-red-600">
              <AlertTriangle className="h-5 w-5" />
              <h3 className="text-lg font-medium">Confirm Deletion</h3>
            </div>
            <p className="mt-3 text-sm text-gray-600">
              Are you sure you want to delete all files matching the current filters?
              {preview && (
                <span className="block mt-1 font-medium text-gray-800">
                  This will affect {preview.matching_references} file(s) ({formatBytes(preview.total_size)}).
                </span>
              )}
              This action cannot be undone.
            </p>
            <div className="mt-4 flex gap-2">
              <button
                onClick={handleDelete}
                disabled={deleteMutation.isPending}
                className="rounded bg-red-600 px-4 py-1.5 text-sm text-white hover:bg-red-700 disabled:opacity-50"
                data-testid="confirm-delete"
              >
                {deleteMutation.isPending ? 'Deleting...' : 'Yes, Delete'}
              </button>
              <button
                onClick={() => setShowConfirm(false)}
                className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300"
                data-testid="cancel-delete"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
