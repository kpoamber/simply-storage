import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  Search, Plus, X, ChevronLeft, ChevronRight,
  ChevronDown, ChevronRight as ChevronRightIcon, Download, Archive,
} from 'lucide-react';
import {
  LineChart, Line, AreaChart, Area,
  XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer,
} from 'recharts';
import { searchFiles, searchSummary, downloadFileBlob, bulkDownload, getMetadataKeys } from '../api/client';
import {
  SearchResult, SearchSummary, SearchRequest,
  FileReference, formatBytes,
} from '../api/types';
import { FilterRow, buildFilterNode } from '../utils/metadataFilters';

/// Convert a yyyy-MM-dd string from <input type="date"> into an ISO datetime
/// in the user's local timezone, optionally pinned to end-of-day for upper bounds.
function dateInputToIso(dateStr: string, endOfDay: boolean): string | undefined {
  if (!dateStr) return undefined;
  const [y, m, d] = dateStr.split('-').map(Number);
  if (!y || !m || !d) return undefined;
  const date = endOfDay
    ? new Date(y, m - 1, d, 23, 59, 59, 999)
    : new Date(y, m - 1, d, 0, 0, 0, 0);
  return date.toISOString();
}

function triggerBlobDownload(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

const SYNC_BADGE_CLASSES: Record<string, string> = {
  synced: 'bg-green-100 text-green-700',
  partial: 'bg-yellow-100 text-yellow-700',
  pending: 'bg-red-100 text-red-700',
  no_storage: 'bg-gray-100 text-gray-500',
};

export default function ProjectSearch() {
  const { id } = useParams<{ id: string }>();
  const [filterRows, setFilterRows] = useState<FilterRow[]>([
    { key: '', value: '', mode: 'and' },
  ]);
  const [nameContains, setNameContains] = useState('');
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');
  const [page, setPage] = useState(1);
  const [hasSearched, setHasSearched] = useState(false);
  const [submittedRequest, setSubmittedRequest] = useState<SearchRequest>({});
  const [downloadingArchive, setDownloadingArchive] = useState(false);
  const [archiveError, setArchiveError] = useState<string | null>(null);
  const perPage = 50;

  const { data: metadataKeys = [] } = useQuery<string[]>({
    queryKey: ['project-metadata-keys', id],
    queryFn: () => getMetadataKeys(id!).then(r => r.data),
    enabled: !!id,
    staleTime: 60_000,
  });

  const { data: searchResult, isFetching: isSearching } = useQuery<SearchResult>({
    queryKey: ['project-search', id, JSON.stringify(submittedRequest), page],
    queryFn: () =>
      searchFiles(id!, { ...submittedRequest, page, per_page: perPage }).then(r => r.data),
    enabled: !!id && hasSearched,
  });

  const { data: summary } = useQuery<SearchSummary>({
    queryKey: ['project-search-summary', id, JSON.stringify(submittedRequest)],
    queryFn: () => searchSummary(id!, submittedRequest).then(r => r.data),
    enabled: !!id && hasSearched,
  });

  const addFilter = () =>
    setFilterRows(prev => [...prev, { key: '', value: '', mode: 'and' }]);

  const removeFilter = (index: number) =>
    setFilterRows(prev => prev.filter((_, i) => i !== index));

  const updateFilter = (index: number, field: keyof FilterRow, val: string) =>
    setFilterRows(prev =>
      prev.map((row, i) => (i === index ? { ...row, [field]: val } : row)),
    );

  const handleSearch = () => {
    setPage(1);
    const trimmedName = nameContains.trim();
    const req: SearchRequest = {
      filters: buildFilterNode(filterRows),
      name_contains: trimmedName || undefined,
      created_after: dateInputToIso(dateFrom, false),
      created_before: dateInputToIso(dateTo, true),
    };
    setSubmittedRequest(req);
    setHasSearched(true);
    setArchiveError(null);
  };

  const handleDownloadArchive = async () => {
    if (!id || !searchResult || searchResult.total === 0) return;
    setDownloadingArchive(true);
    setArchiveError(null);
    try {
      const response = await bulkDownload(id, submittedRequest);
      const stamp = new Date().toISOString().replace(/[-:]/g, '').slice(0, 15);
      triggerBlobDownload(response.data, `search-results-${stamp}.tar.gz`);
    } catch (err: unknown) {
      // Error body is a Blob; try to read JSON message.
      let msg = 'Failed to download archive';
      const errObj = err as { response?: { data?: Blob | { error?: string } } };
      const data = errObj?.response?.data;
      if (data instanceof Blob) {
        try {
          const text = await data.text();
          const parsed = JSON.parse(text);
          if (parsed?.error) msg = parsed.error;
        } catch { /* ignore parse failure */ }
      } else if (data && typeof data === 'object' && 'error' in data && data.error) {
        msg = String(data.error);
      }
      setArchiveError(msg);
    } finally {
      setDownloadingArchive(false);
    }
  };

  const totalPages = searchResult ? Math.ceil(searchResult.total / perPage) : 0;

  return (
    <div>
      <Link to={`/projects/${id}`} className="text-sm text-blue-600 hover:underline">
        &larr; Back to Project
      </Link>
      <h2 className="mt-2 text-2xl font-semibold text-gray-800">Search Files</h2>

      {/* Query Builder */}
      <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4">
        <datalist id="metadata-keys-list">
          {metadataKeys.map(k => (
            <option key={k} value={k} />
          ))}
        </datalist>

        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <div>
            <label className="text-xs font-medium uppercase text-gray-500">File name contains</label>
            <input
              value={nameContains}
              onChange={e => setNameContains(e.target.value)}
              placeholder="e.g. report"
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="name-contains"
            />
          </div>
          <div>
            <label className="text-xs font-medium uppercase text-gray-500">Uploaded from</label>
            <input
              type="date"
              value={dateFrom}
              onChange={e => setDateFrom(e.target.value)}
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="date-from"
            />
          </div>
          <div>
            <label className="text-xs font-medium uppercase text-gray-500">Uploaded to</label>
            <input
              type="date"
              value={dateTo}
              onChange={e => setDateTo(e.target.value)}
              className="mt-1 w-full rounded border border-gray-300 px-2 py-1.5 text-sm"
              data-testid="date-to"
            />
          </div>
        </div>

        <div className="mt-4 flex items-center justify-between">
          <h3 className="font-medium text-gray-700">Metadata filters</h3>
          <button
            onClick={addFilter}
            className="flex items-center gap-1 text-sm text-blue-600 hover:text-blue-700"
            data-testid="add-filter"
          >
            <Plus className="h-3 w-3" /> Add filter
          </button>
        </div>

        <div className="mt-3 space-y-2">
          {filterRows.map((row, idx) => (
            <div key={idx} className="flex items-center gap-2" data-testid="filter-row">
              <select
                value={row.mode}
                onChange={e => updateFilter(idx, 'mode', e.target.value)}
                className="rounded border border-gray-300 px-2 py-1.5 text-sm"
                data-testid="filter-mode"
              >
                <option value="and">AND</option>
                <option value="or">OR</option>
                <option value="not">NOT</option>
              </select>
              <input
                value={row.key}
                onChange={e => updateFilter(idx, 'key', e.target.value)}
                placeholder="Key"
                list="metadata-keys-list"
                className="w-1/3 rounded border border-gray-300 px-2 py-1.5 text-sm"
                data-testid="filter-key"
              />
              <input
                value={row.value}
                onChange={e => updateFilter(idx, 'value', e.target.value)}
                placeholder="Value"
                className="flex-1 rounded border border-gray-300 px-2 py-1.5 text-sm"
                data-testid="filter-value"
              />
              {filterRows.length > 1 && (
                <button
                  onClick={() => removeFilter(idx)}
                  className="rounded p-1 text-red-400 hover:bg-red-50 hover:text-red-600"
                  data-testid="remove-filter"
                >
                  <X className="h-4 w-4" />
                </button>
              )}
            </div>
          ))}
        </div>

        <button
          onClick={handleSearch}
          disabled={isSearching}
          className="mt-3 flex items-center gap-2 rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          data-testid="search-button"
        >
          <Search className="h-4 w-4" />
          {isSearching ? 'Searching...' : 'Search'}
        </button>
      </div>

      {/* Summary */}
      {summary && (
        <div className="mt-4 rounded-lg border border-gray-200 bg-white p-4" data-testid="search-summary">
          <h3 className="font-medium text-gray-700">Summary</h3>
          <div className="mt-2 grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
            <div>
              <span className="text-gray-500">Total files</span>
              <p className="text-lg font-semibold text-gray-900">{summary.total_files}</p>
            </div>
            <div>
              <span className="text-gray-500">Total size</span>
              <p className="text-lg font-semibold text-gray-900">{formatBytes(summary.total_size)}</p>
            </div>
            <div>
              <span className="text-gray-500">Earliest upload</span>
              <p className="text-gray-900">
                {summary.earliest_upload
                  ? new Date(summary.earliest_upload).toLocaleDateString()
                  : '\u2014'}
              </p>
            </div>
            <div>
              <span className="text-gray-500">Latest upload</span>
              <p className="text-gray-900">
                {summary.latest_upload
                  ? new Date(summary.latest_upload).toLocaleDateString()
                  : '\u2014'}
              </p>
            </div>
          </div>

          {summary.timeline.length > 0 && (
            <div className="mt-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
              <div>
                <h4 className="text-sm font-medium text-gray-600">File Count Over Time</h4>
                <div className="mt-2 h-48" data-testid="count-chart">
                  <ResponsiveContainer width="100%" height="100%">
                    <LineChart data={summary.timeline}>
                      <CartesianGrid strokeDasharray="3 3" />
                      <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                      <YAxis tick={{ fontSize: 11 }} />
                      <Tooltip />
                      <Line type="monotone" dataKey="count" stroke="#3b82f6" strokeWidth={2} />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
              </div>
              <div>
                <h4 className="text-sm font-medium text-gray-600">Size Over Time</h4>
                <div className="mt-2 h-48" data-testid="size-chart">
                  <ResponsiveContainer width="100%" height="100%">
                    <AreaChart data={summary.timeline}>
                      <CartesianGrid strokeDasharray="3 3" />
                      <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                      <YAxis tick={{ fontSize: 11 }} tickFormatter={(v: number) => formatBytes(v)} />
                      <Tooltip formatter={(v) => formatBytes(Number(v ?? 0))} />
                      <Area type="monotone" dataKey="size" stroke="#8b5cf6" fill="#c4b5fd" />
                    </AreaChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Results Table */}
      {searchResult && (
        <div className="mt-4">
          <div className="flex items-center justify-between">
            <h3 className="text-lg font-medium text-gray-700">
              Results ({searchResult.total} file{searchResult.total !== 1 ? 's' : ''})
            </h3>
            {searchResult.total > 0 && (
              <button
                onClick={handleDownloadArchive}
                disabled={downloadingArchive}
                className="flex items-center gap-2 rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
                data-testid="download-archive"
              >
                <Archive className="h-4 w-4" />
                {downloadingArchive ? 'Building archive…' : 'Download archive (.tar.gz)'}
              </button>
            )}
          </div>
          {archiveError && (
            <div className="mt-2 rounded border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700" data-testid="archive-error">
              {archiveError}
            </div>
          )}

          {searchResult.results.length === 0 ? (
            <p className="mt-2 text-gray-400">No files found matching your filters.</p>
          ) : (
            <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="w-8 px-2 py-2"></th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Size</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Sync</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Metadata</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
                    <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 bg-white">
                  {searchResult.results.map((f: FileReference) => (
                    <SearchResultRow key={f.id} file={f} />
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="mt-3 flex items-center justify-between">
              <button
                onClick={() => setPage(p => Math.max(1, p - 1))}
                disabled={page === 1}
                className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
              >
                <ChevronLeft className="h-4 w-4" /> Previous
              </button>
              <span className="text-sm text-gray-500">
                Page {searchResult.page} of {totalPages}
              </span>
              <button
                onClick={() => setPage(p => p + 1)}
                disabled={page >= totalPages}
                className="flex items-center gap-1 rounded border px-2 py-1 text-sm disabled:opacity-30"
              >
                Next <ChevronRight className="h-4 w-4" />
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SearchResultRow({ file }: { file: FileReference }) {
  const [expanded, setExpanded] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const metadataEntries = Object.entries(file.metadata || {});
  const syncDetails = file.sync_details || [];
  const hasSyncDetails = syncDetails.length > 0;

  const syncStatus = file.sync_status || 'no_storage';
  const badgeClass = SYNC_BADGE_CLASSES[syncStatus] || SYNC_BADGE_CLASSES.no_storage;
  const syncLabel = file.total_storages !== undefined && file.total_storages > 0
    ? `${file.synced_storages ?? 0}/${file.total_storages}`
    : '—';

  const handleDownload = async () => {
    setDownloading(true);
    try {
      const response = await downloadFileBlob(file.file_id);
      triggerBlobDownload(response.data, file.original_name);
    } catch (err) {
      console.error('Download failed', err);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <>
      <tr className={expanded ? 'bg-gray-50' : ''}>
        <td className="px-2 py-2">
          {hasSyncDetails && (
            <button
              onClick={() => setExpanded(e => !e)}
              className="rounded p-1 text-gray-400 hover:bg-gray-200 hover:text-gray-700"
              data-testid="toggle-row"
              aria-label={expanded ? 'Collapse' : 'Expand'}
            >
              {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRightIcon className="h-4 w-4" />}
            </button>
          )}
        </td>
        <td className="px-4 py-2 text-sm text-gray-900">{file.original_name}</td>
        <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-700">
          {file.file_size !== undefined ? formatBytes(file.file_size) : '—'}
        </td>
        <td className="px-4 py-2 text-sm">
          <span
            className={`inline-flex rounded-full px-2 py-0.5 text-xs font-medium ${badgeClass}`}
            title={syncStatus}
            data-testid="sync-badge"
          >
            {syncLabel}
          </span>
        </td>
        <td className="px-4 py-2 text-sm">
          {metadataEntries.length === 0 ? (
            <span className="text-gray-400">--</span>
          ) : (
            <div className="flex flex-wrap gap-1">
              {metadataEntries.map(([k, v]) => (
                <span
                  key={k}
                  className="inline-flex rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-700"
                >
                  {k}={String(v)}
                </span>
              ))}
            </div>
          )}
        </td>
        <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
          {new Date(file.created_at).toLocaleDateString()}
        </td>
        <td className="whitespace-nowrap px-4 py-2 text-right text-sm">
          <button
            onClick={handleDownload}
            disabled={downloading}
            className="inline-flex items-center gap-1 rounded border border-gray-300 px-2 py-1 text-xs text-gray-700 hover:bg-gray-100 disabled:opacity-50"
            data-testid="download-file"
            title="Download file"
          >
            <Download className="h-3 w-3" />
            {downloading ? '…' : 'Download'}
          </button>
        </td>
      </tr>
      {expanded && hasSyncDetails && (
        <tr className="bg-gray-50">
          <td colSpan={7} className="px-4 py-3">
            <div className="text-xs text-gray-600">
              <div className="mb-2 font-medium uppercase tracking-wide">Storage locations</div>
              <table className="min-w-full">
                <thead>
                  <tr className="text-left text-gray-500">
                    <th className="py-1 pr-4 font-normal">Storage</th>
                    <th className="py-1 pr-4 font-normal">Type</th>
                    <th className="py-1 pr-4 font-normal">Status</th>
                    <th className="py-1 pr-4 font-normal">Path</th>
                    <th className="py-1 font-normal">Synced at</th>
                  </tr>
                </thead>
                <tbody>
                  {syncDetails.map(d => (
                    <tr key={d.storage_id}>
                      <td className="py-1 pr-4 text-gray-900">{d.storage_name}</td>
                      <td className="py-1 pr-4 text-gray-500">{d.storage_type}</td>
                      <td className="py-1 pr-4">
                        <span
                          className={`inline-flex rounded-full px-2 py-0.5 text-xxs font-medium ${
                            d.status === 'synced'
                              ? 'bg-green-100 text-green-700'
                              : d.status === 'pending'
                                ? 'bg-yellow-100 text-yellow-700'
                                : 'bg-gray-100 text-gray-500'
                          }`}
                        >
                          {d.status}
                        </span>
                      </td>
                      <td className="py-1 pr-4 font-mono text-gray-600">
                        {d.storage_path || <span className="text-gray-400">—</span>}
                      </td>
                      <td className="py-1 text-gray-500">
                        {d.synced_at ? new Date(d.synced_at).toLocaleString() : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
