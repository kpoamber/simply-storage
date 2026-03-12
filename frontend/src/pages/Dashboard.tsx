import { useQuery } from '@tanstack/react-query';
import { Files, HardDrive, RefreshCw, Server } from 'lucide-react';
import apiClient from '../api/client';
import { SystemStats, StorageBackend, formatBytes } from '../api/types';

export default function Dashboard() {
  const { data: stats, isLoading: statsLoading } = useQuery<SystemStats>({
    queryKey: ['system-stats'],
    queryFn: () => apiClient.get('/system/stats').then(r => r.data),
  });

  const { data: storages, isLoading: storagesLoading } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  return (
    <div>
      <h2 className="text-2xl font-semibold text-gray-800">Dashboard</h2>
      <p className="mt-1 text-gray-500">System overview and statistics.</p>

      <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          icon={<Files className="h-6 w-6 text-blue-600" />}
          label="Total Files"
          value={statsLoading ? '...' : String(stats?.total_files ?? 0)}
        />
        <StatCard
          icon={<HardDrive className="h-6 w-6 text-green-600" />}
          label="Storage Used"
          value={statsLoading ? '...' : formatBytes(stats?.total_storage_used ?? 0)}
        />
        <StatCard
          icon={<RefreshCw className="h-6 w-6 text-orange-600" />}
          label="Pending Sync Tasks"
          value={statsLoading ? '...' : String(stats?.pending_sync_tasks ?? 0)}
        />
        <StatCard
          icon={<Server className="h-6 w-6 text-purple-600" />}
          label="Active Nodes"
          value="0"
        />
      </div>

      <div className="mt-8">
        <h3 className="text-lg font-medium text-gray-700">Storage Health</h3>
        {storagesLoading ? (
          <p className="mt-2 text-gray-400">Loading storages...</p>
        ) : !storages?.length ? (
          <p className="mt-2 text-gray-400">No storages configured.</p>
        ) : (
          <div className="mt-3 overflow-hidden rounded-lg border border-gray-200">
            <table className="min-w-full divide-y divide-gray-200">
              <thead className="bg-gray-50">
                <tr>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Type</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Tier</th>
                  <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                  <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Files</th>
                  <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Used</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 bg-white">
                {storages.map(s => (
                  <tr key={s.id}>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-900">{s.name}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">{s.storage_type}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm">
                      <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${s.is_hot ? 'bg-red-100 text-red-700' : 'bg-blue-100 text-blue-700'}`}>
                        {s.is_hot ? 'Hot' : 'Cold'}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-sm">
                      <span className={`inline-block rounded px-2 py-0.5 text-xs font-medium ${s.enabled ? 'bg-green-100 text-green-700' : 'bg-gray-100 text-gray-500'}`}>
                        {s.enabled ? 'Enabled' : 'Disabled'}
                      </span>
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">{s.file_count}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">{formatBytes(s.used_space)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

function StatCard({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
      <div className="flex items-center gap-3">
        {icon}
        <div>
          <p className="text-sm text-gray-500">{label}</p>
          <p className="text-2xl font-semibold text-gray-900">{value}</p>
        </div>
      </div>
    </div>
  );
}
