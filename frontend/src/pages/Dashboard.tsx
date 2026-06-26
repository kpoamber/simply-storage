import { useState } from 'react';
import { Link } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  Files, HardDrive, RefreshCw, Server, Download, AlertTriangle, Check,
} from 'lucide-react';
import {
  AreaChart, Area, LineChart, Line, BarChart, Bar,
  XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Legend,
} from 'recharts';
import apiClient, { getDashboard } from '../api/client';
import {
  DashboardPeriod, DashboardResponse, Project, StorageBackend, formatBytes,
} from '../api/types';
import { useAuth } from '../contexts/AuthContext';

const PERIODS: { value: DashboardPeriod; label: string }[] = [
  { value: 'today', label: 'Today' },
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: '90d', label: '90 days' },
  { value: '1y', label: '1 year' },
  { value: 'all', label: 'All time' },
];

interface Node {
  id: string;
  node_id: string;
  address: string;
  started_at: string;
  last_heartbeat: string;
  created_at: string;
}

export default function Dashboard() {
  const { user } = useAuth();
  const isAdmin = user?.role === 'admin';

  const [period, setPeriod] = useState<DashboardPeriod>('30d');
  const [projectId, setProjectId] = useState<string>('');
  const [storageId, setStorageId] = useState<string>('');

  const { data: projects } = useQuery<Project[]>({
    queryKey: ['projects'],
    queryFn: () => apiClient.get('/projects').then(r => r.data),
    enabled: isAdmin,
    staleTime: 60_000,
  });

  const { data: storages } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
    enabled: isAdmin,
    staleTime: 60_000,
  });

  const { data: nodes } = useQuery<Node[]>({
    queryKey: ['nodes'],
    queryFn: () => apiClient.get('/system/nodes').then(r => r.data),
    refetchInterval: 30_000,
    enabled: isAdmin,
  });

  const { data: dashboard, isFetching } = useQuery<DashboardResponse>({
    queryKey: ['dashboard', period, projectId, storageId],
    queryFn: () =>
      getDashboard({
        period,
        project_id: projectId || undefined,
        storage_id: storageId || undefined,
      }).then(r => r.data),
    refetchInterval: 60_000,
    enabled: isAdmin,
  });

  if (!isAdmin) {
    return (
      <div>
        <h2 className="text-2xl font-semibold text-gray-800">Dashboard</h2>
        <p className="mt-1 text-gray-500">Welcome to Simply Storage.</p>
        <div className="mt-6 rounded-lg border border-gray-200 bg-white p-6">
          <p className="text-gray-600">Navigate to <strong>Projects</strong> to manage your files.</p>
        </div>
      </div>
    );
  }

  const totals = dashboard?.totals;

  return (
    <div>
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-2xl font-semibold text-gray-800">Dashboard</h2>
          <p className="mt-1 text-gray-500">
            System metrics{isFetching ? ' · refreshing…' : ''}
          </p>
        </div>
      </div>

      {/* Controls */}
      <div className="mt-4 flex flex-wrap items-center gap-3 rounded-lg border border-gray-200 bg-white p-3">
        <div className="inline-flex rounded-md border border-gray-300" role="group">
          {PERIODS.map(p => (
            <button
              key={p.value}
              onClick={() => setPeriod(p.value)}
              className={`px-3 py-1.5 text-sm font-medium first:rounded-l-md last:rounded-r-md ${
                period === p.value
                  ? 'bg-accent text-white'
                  : 'bg-elev text-ink-2 hover:bg-sunk'
              }`}
              data-testid={`period-${p.value}`}
            >
              {p.label}
            </button>
          ))}
        </div>

        <select
          value={projectId}
          onChange={e => setProjectId(e.target.value)}
          className="rounded border border-gray-300 px-2 py-1.5 text-sm"
          data-testid="project-filter"
        >
          <option value="">All projects</option>
          {projects?.map(p => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>

        <select
          value={storageId}
          onChange={e => setStorageId(e.target.value)}
          className="rounded border border-gray-300 px-2 py-1.5 text-sm"
          data-testid="storage-filter"
        >
          <option value="">All storages</option>
          {storages?.map(s => (
            <option key={s.id} value={s.id}>{s.name}</option>
          ))}
        </select>
      </div>

      {/* Stat cards (all scoped to the selected period + filters).
          Sync-queue metrics moved to their own block below — they're
          intentionally NOT bound to the period selector. */}
      <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          icon={<Files className="h-5 w-5 text-blue-600" />}
          label={`Files · ${period}`}
          value={totals ? totals.files.toLocaleString() : '—'}
          sub={totals ? formatBytes(totals.bytes) : undefined}
        />
        <StatCard
          icon={<Download className="h-5 w-5 text-purple-500" />}
          label={`Accesses · ${period}`}
          value={totals ? `${totals.accesses_in_period}` : '—'}
          sub={totals ? formatBytes(totals.bytes_accessed_in_period) : undefined}
        />
        <StatCard
          icon={<HardDrive className="h-5 w-5 text-green-600" />}
          label={`Bytes uploaded · ${period}`}
          value={totals ? formatBytes(totals.bytes_uploaded_in_period) : '—'}
          sub={totals ? `${totals.uploads_in_period} files` : undefined}
        />
        <StatCard
          icon={<Server className="h-5 w-5 text-purple-600" />}
          label="Active Nodes"
          value={String(nodes?.length ?? 0)}
        />
      </div>

      {/* Charts row */}
      <div className="mt-6 grid grid-cols-1 gap-4 lg:grid-cols-2">
        <ChartCard title="Uploads over time" subtitle="Bytes uploaded per bucket">
          {dashboard && dashboard.upload_timeline.length > 0 ? (
            <ResponsiveContainer width="100%" height={220}>
              <AreaChart data={dashboard.upload_timeline}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v: number) => formatBytes(v)} />
                <Tooltip formatter={(v) => formatBytes(Number(v ?? 0))} />
                <Area type="monotone" dataKey="size" stroke="#3b82f6" fill="#bfdbfe" />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <EmptyChart />
          )}
        </ChartCard>

        <ChartCard title="Accesses over time" subtitle="File downloads per bucket">
          {dashboard && dashboard.access_timeline.length > 0 ? (
            <ResponsiveContainer width="100%" height={220}>
              <LineChart data={dashboard.access_timeline}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} />
                <Tooltip />
                <Line type="monotone" dataKey="count" stroke="#8b5cf6" strokeWidth={2} />
              </LineChart>
            </ResponsiveContainer>
          ) : (
            <EmptyChart message="No accesses recorded in this period" />
          )}
        </ChartCard>
      </div>

      {/* Sync queue health — independent of period filter. Each card deep-links
          into Sync Tasks with the matching status filter (and the current
          project filter, if one is active) so the operator can drill from a
          number straight into the rows behind it. */}
      <div className="mt-6">
        <div className="mb-2 flex items-baseline justify-between">
          <h3 className="text-sm font-medium text-gray-700">Sync queue</h3>
          <span className="text-xs text-gray-400">
            Click a card to see the underlying tasks.
          </span>
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <StatCard
            icon={<RefreshCw className="h-5 w-5 text-orange-500" />}
            label="Pending"
            value={totals ? totals.pending_syncs.toLocaleString() : '—'}
            sub="awaiting worker"
            to={syncTasksHref('pending', projectId)}
          />
          <StatCard
            icon={<AlertTriangle className="h-5 w-5 text-red-500" />}
            label="Failed (all-time)"
            value={totals ? totals.failed_syncs_total.toLocaleString() : '—'}
            sub={totals ? `${totals.failed_syncs_in_period} in selected period` : undefined}
            to={syncTasksHref('failed', projectId)}
          />
          <StatCard
            icon={<Check className="h-5 w-5 text-green-600" />}
            label="Synced · 24h"
            value={totals ? totals.synced_in_24h.toLocaleString() : '—'}
            sub="completed in last 24 hours"
            to={syncTasksHref('completed', projectId)}
          />
        </div>
      </div>

      {/* Sync trend */}
      <div className="mt-6">
        <ChartCard title="Sync task status trend">
          {dashboard && dashboard.sync_status_trend.length > 0 ? (
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={dashboard.sync_status_trend}>
                <CartesianGrid strokeDasharray="3 3" />
                <XAxis dataKey="date" tick={{ fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} />
                <Tooltip />
                <Legend />
                <Bar dataKey="completed" stackId="s" fill="#10b981" />
                <Bar dataKey="pending"   stackId="s" fill="#f59e0b" />
                <Bar dataKey="failed"    stackId="s" fill="#ef4444" />
              </BarChart>
            </ResponsiveContainer>
          ) : (
            <EmptyChart message="No sync activity in this period" />
          )}
        </ChartCard>
      </div>

      {/* Breakdown tables */}
      <div className="mt-6 grid grid-cols-1 gap-4 lg:grid-cols-2">
        <BreakdownCard title="By content type">
          {dashboard && dashboard.by_content_type.length > 0 ? (
            <table className="min-w-full text-sm">
              <thead className="text-left text-xs uppercase text-gray-500">
                <tr><th className="py-1">Type</th><th className="py-1 text-right">Files</th><th className="py-1 text-right">Size</th></tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {dashboard.by_content_type.map(ct => (
                  <tr key={ct.content_type}>
                    <td className="py-1 text-gray-800">{ct.content_type || '—'}</td>
                    <td className="py-1 text-right text-gray-700">{ct.count.toLocaleString()}</td>
                    <td className="py-1 text-right text-gray-700">{formatBytes(ct.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyTable />
          )}
        </BreakdownCard>

        <BreakdownCard title="Top accessed files" subtitle={`Top 10 · ${period}`}>
          {dashboard && dashboard.top_accessed_files.length > 0 ? (
            <table className="min-w-full text-sm">
              <thead className="text-left text-xs uppercase text-gray-500">
                <tr>
                  <th className="py-1">Name</th>
                  <th className="py-1 text-right">Accesses</th>
                  <th className="py-1 text-right">Last access</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {dashboard.top_accessed_files.map(f => (
                  <tr key={f.file_id}>
                    <td className="py-1 text-gray-800">
                      <span className="font-medium">{f.original_name || f.file_id}</span>
                      <span className="ml-2 text-xs text-gray-400">{f.content_type}</span>
                    </td>
                    <td className="py-1 text-right text-gray-700">{f.access_count.toLocaleString()}</td>
                    <td className="py-1 text-right text-gray-500">
                      {new Date(f.last_accessed).toLocaleString()}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyTable message="No accesses yet — accesses are tracked from now on." />
          )}
        </BreakdownCard>
      </div>

      {/* Storage breakdown */}
      <div className="mt-6">
        <BreakdownCard title="Storage breakdown">
          {dashboard && dashboard.by_storage.length > 0 ? (
            <table className="min-w-full text-sm">
              <thead className="text-left text-xs uppercase text-gray-500">
                <tr>
                  <th className="py-1">Storage</th>
                  <th className="py-1 text-right">Files</th>
                  <th className="py-1 text-right">Size</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-100">
                {dashboard.by_storage.map(s => (
                  <tr key={s.storage_id}>
                    <td className="py-1 text-gray-800">{s.name}</td>
                    <td className="py-1 text-right text-gray-700">{s.count.toLocaleString()}</td>
                    <td className="py-1 text-right text-gray-700">{formatBytes(s.size)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          ) : (
            <EmptyTable />
          )}
        </BreakdownCard>
      </div>
    </div>
  );
}

/// Build a `/sync-tasks` URL preloaded with a status filter and (optionally)
/// the project filter currently active on the dashboard. The Sync Tasks page
/// reads `?status` and `?project_id` query params on mount.
function syncTasksHref(status: 'pending' | 'failed' | 'completed', projectId: string): string {
  const params = new URLSearchParams({ status });
  if (projectId) params.set('project_id', projectId);
  return `/sync-tasks?${params.toString()}`;
}

function StatCard({
  icon, label, value, sub, to,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  sub?: string;
  /// When set, the whole card becomes a router Link to this path. Used by the
  /// Sync queue block to deep-link into the Sync Tasks page with the matching
  /// status filter pre-applied.
  to?: string;
}) {
  const body = (
    <>
      <div className="flex items-center gap-2">
        {icon}
        <p className="text-xs text-gray-500">{label}</p>
      </div>
      <p className="mt-1 text-xl font-semibold text-gray-900">{value}</p>
      {sub && <p className="text-xs text-gray-500">{sub}</p>}
    </>
  );
  const base = 'rounded-lg border border-gray-200 bg-white p-3 shadow-sm';
  if (to) {
    return (
      <Link
        to={to}
        className={`${base} block transition hover:border-gray-300 hover:shadow focus:outline-none focus:ring-2 focus:ring-accent`}
      >
        {body}
      </Link>
    );
  }
  return <div className={base}>{body}</div>;
}

function ChartCard({
  title, subtitle, children,
}: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="flex items-baseline justify-between">
        <h3 className="text-sm font-medium text-gray-700">{title}</h3>
        {subtitle && <span className="text-xs text-gray-400">{subtitle}</span>}
      </div>
      <div className="mt-2">{children}</div>
    </div>
  );
}

function BreakdownCard({
  title, subtitle, children,
}: { title: string; subtitle?: string; children: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4">
      <div className="flex items-baseline justify-between">
        <h3 className="text-sm font-medium text-gray-700">{title}</h3>
        {subtitle && <span className="text-xs text-gray-400">{subtitle}</span>}
      </div>
      <div className="mt-2 overflow-x-auto">{children}</div>
    </div>
  );
}

function EmptyChart({ message }: { message?: string } = {}) {
  return (
    <div className="flex h-[220px] items-center justify-center text-sm text-gray-400">
      <AlertTriangle className="mr-2 h-4 w-4" /> {message || 'No data for this period'}
    </div>
  );
}

function EmptyTable({ message }: { message?: string } = {}) {
  return <p className="py-2 text-sm text-gray-400">{message || 'No data'}</p>;
}
