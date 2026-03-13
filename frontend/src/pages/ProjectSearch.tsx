import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  Search, Plus, X, ChevronLeft, ChevronRight,
} from 'lucide-react';
import {
  LineChart, Line, AreaChart, Area,
  XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer,
} from 'recharts';
import { searchFiles, searchSummary } from '../api/client';
import {
  SearchResult, SearchSummary,
  FileReference, formatBytes,
} from '../api/types';
import { FilterRow, buildFilterNode } from '../utils/metadataFilters';

export default function ProjectSearch() {
  const { id } = useParams<{ id: string }>();
  const [filterRows, setFilterRows] = useState<FilterRow[]>([
    { key: '', value: '', mode: 'and' },
  ]);
  const [page, setPage] = useState(1);
  const [hasSearched, setHasSearched] = useState(false);
  const [submittedFilter, setSubmittedFilter] = useState<ReturnType<typeof buildFilterNode>>(undefined);
  const perPage = 50;

  const { data: searchResult, isFetching: isSearching } = useQuery<SearchResult>({
    queryKey: ['project-search', id, JSON.stringify(submittedFilter), page],
    queryFn: () =>
      searchFiles(id!, { filters: submittedFilter, page, per_page: perPage }).then(r => r.data),
    enabled: !!id && hasSearched,
  });

  const { data: summary } = useQuery<SearchSummary>({
    queryKey: ['project-search-summary', id, JSON.stringify(submittedFilter)],
    queryFn: () => searchSummary(id!, submittedFilter).then(r => r.data),
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
    setSubmittedFilter(buildFilterNode(filterRows));
    setHasSearched(true);
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
        <div className="flex items-center justify-between">
          <h3 className="font-medium text-gray-700">Filters</h3>
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
                      <Tooltip formatter={(v: number) => formatBytes(v)} />
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
          <h3 className="text-lg font-medium text-gray-700">
            Results ({searchResult.total} file{searchResult.total !== 1 ? 's' : ''})
          </h3>

          {searchResult.results.length === 0 ? (
            <p className="mt-2 text-gray-400">No files found matching your filters.</p>
          ) : (
            <div className="mt-2 overflow-hidden rounded-lg border border-gray-200">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Metadata</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Created</th>
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
  const metadataEntries = Object.entries(file.metadata || {});

  return (
    <tr>
      <td className="px-4 py-2 text-sm text-gray-900">{file.original_name}</td>
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
    </tr>
  );
}
