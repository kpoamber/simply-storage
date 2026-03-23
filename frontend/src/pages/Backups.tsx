import { useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, Pencil, X, Play } from 'lucide-react';
import apiClient from '../api/client';
import {
  BackupConfig,
  BackupRecord,
  CreateBackupConfigRequest,
  UpdateBackupConfigRequest,
  TriggerBackupRequest,
  StorageBackend,
  formatBytes,
} from '../api/types';

type Tab = 'configs' | 'history';

const CRON_PRESETS: { label: string; value: string }[] = [
  { label: 'Every hour', value: '0 0 * * * * *' },
  { label: 'Daily at 2:00 AM', value: '0 0 2 * * * *' },
  { label: 'Daily at midnight', value: '0 0 0 * * * *' },
  { label: 'Every 6 hours', value: '0 0 */6 * * * *' },
  { label: 'Weekly (Sunday 2 AM)', value: '0 0 2 * * 0 *' },
  { label: 'Custom', value: '' },
];

function statusBadge(status: string) {
  const map: Record<string, string> = {
    completed: 'bg-green-100 text-green-700',
    running: 'bg-blue-100 text-blue-700',
    pending: 'bg-yellow-100 text-yellow-700',
    failed: 'bg-red-100 text-red-700',
  };
  const cls = map[status] || 'bg-gray-100 text-gray-600';
  return (
    <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${cls}`}>
      {status}
    </span>
  );
}

function formatDuration(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt || !completedAt) return '-';
  const ms = new Date(completedAt).getTime() - new Date(startedAt).getTime();
  if (ms < 1000) return `${ms}ms`;
  const secs = Math.round(ms / 1000);
  if (secs < 60) return `${secs}s`;
  return `${Math.floor(secs / 60)}m ${secs % 60}s`;
}

export default function Backups() {
  const queryClient = useQueryClient();
  const [activeTab, setActiveTab] = useState<Tab>('configs');
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [showTriggerForm, setShowTriggerForm] = useState(false);
  const [error, setError] = useState('');

  // Queries
  const { data: configs, isLoading: configsLoading } = useQuery<BackupConfig[]>({
    queryKey: ['backup-configs'],
    queryFn: () => apiClient.get('/backup-configs').then(r => r.data),
  });

  const { data: backups, isLoading: backupsLoading } = useQuery<BackupRecord[]>({
    queryKey: ['backups'],
    queryFn: () => apiClient.get('/backups').then(r => r.data),
  });

  const { data: storages } = useQuery<StorageBackend[]>({
    queryKey: ['storages'],
    queryFn: () => apiClient.get('/storages').then(r => r.data),
  });

  // Config mutations
  const createConfigMutation = useMutation({
    mutationFn: (data: CreateBackupConfigRequest) =>
      apiClient.post('/backup-configs', data).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backup-configs'] });
      setShowCreateForm(false);
      setError('');
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to create config';
      setError(msg);
    },
  });

  const updateConfigMutation = useMutation({
    mutationFn: ({ id, data }: { id: string; data: UpdateBackupConfigRequest }) =>
      apiClient.put(`/backup-configs/${id}`, data).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backup-configs'] });
      setEditingId(null);
      setError('');
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to update config';
      setError(msg);
    },
  });

  const deleteConfigMutation = useMutation({
    mutationFn: (id: string) => apiClient.delete(`/backup-configs/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backup-configs'] });
    },
  });

  const toggleEnabledMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      apiClient.put(`/backup-configs/${id}`, { enabled }).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backup-configs'] });
    },
  });

  // Backup mutations
  const triggerBackupMutation = useMutation({
    mutationFn: (data: TriggerBackupRequest) =>
      apiClient.post('/backups/trigger', data).then(r => r.data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backups'] });
      setShowTriggerForm(false);
      setError('');
    },
    onError: (err: unknown) => {
      const msg = (err as { response?: { data?: { error?: string } } })?.response?.data?.error || 'Failed to trigger backup';
      setError(msg);
    },
  });

  const deleteBackupMutation = useMutation({
    mutationFn: (id: string) => apiClient.delete(`/backups/${id}`),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['backups'] });
    },
  });

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-semibold text-gray-800">Backups</h2>
          <p className="mt-1 text-gray-500">Manage database backup schedules and history.</p>
        </div>
      </div>

      {/* Tabs */}
      <div className="mt-4 flex border-b border-gray-200">
        <button
          onClick={() => setActiveTab('configs')}
          className={`px-4 py-2 text-sm font-medium border-b-2 -mb-px transition-colors ${
            activeTab === 'configs'
              ? 'border-blue-600 text-blue-600'
              : 'border-transparent text-gray-500 hover:text-gray-700'
          }`}
        >
          Configuration
        </button>
        <button
          onClick={() => setActiveTab('history')}
          className={`px-4 py-2 text-sm font-medium border-b-2 -mb-px transition-colors ${
            activeTab === 'history'
              ? 'border-blue-600 text-blue-600'
              : 'border-transparent text-gray-500 hover:text-gray-700'
          }`}
        >
          History
        </button>
      </div>

      {/* Configuration tab */}
      {activeTab === 'configs' && (
        <div className="mt-4">
          <div className="flex justify-end mb-4">
            <button
              onClick={() => { setShowCreateForm(true); setError(''); }}
              className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700"
            >
              <Plus className="h-4 w-4" /> Add Config
            </button>
          </div>

          {showCreateForm && (
            <ConfigForm
              storages={storages ?? []}
              error={error}
              isLoading={createConfigMutation.isPending}
              onSubmit={(data) => createConfigMutation.mutate(data)}
              onCancel={() => { setShowCreateForm(false); setError(''); }}
            />
          )}

          {configsLoading ? (
            <p className="text-gray-400">Loading configs...</p>
          ) : !configs?.length ? (
            <p className="text-gray-400">No backup configurations yet.</p>
          ) : (
            <div className="overflow-hidden rounded-lg border border-gray-200">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Name</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Storage</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Path</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Schedule</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Retention</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Enabled</th>
                    <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 bg-white">
                  {configs.map(config => (
                    <tr key={config.id}>
                      <td className="whitespace-nowrap px-4 py-2 text-sm font-medium text-gray-900">
                        {config.name}
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                        {config.storage_name || 'Unknown'}
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                        {config.storage_path || '/'}
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                        <code className="text-xs bg-gray-100 px-1.5 py-0.5 rounded">{config.schedule_cron}</code>
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                        {config.retention_count}
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-sm">
                        <button
                          onClick={() => toggleEnabledMutation.mutate({ id: config.id, enabled: !config.enabled })}
                          className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                            config.enabled ? 'bg-blue-600' : 'bg-gray-300'
                          }`}
                        >
                          <span
                            className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                              config.enabled ? 'translate-x-4.5' : 'translate-x-0.5'
                            }`}
                          />
                        </button>
                      </td>
                      <td className="whitespace-nowrap px-4 py-2 text-center">
                        <div className="flex items-center justify-center gap-2">
                          <button
                            onClick={() => triggerBackupMutation.mutate({ config_id: config.id })}
                            className="text-green-600 hover:text-green-800"
                            title="Run now"
                          >
                            <Play className="h-4 w-4" />
                          </button>
                          <button
                            onClick={() => { setEditingId(config.id); setError(''); }}
                            className="text-gray-500 hover:text-gray-700"
                            title="Edit"
                          >
                            <Pencil className="h-4 w-4" />
                          </button>
                          <button
                            onClick={() => {
                              if (window.confirm(`Delete backup config "${config.name}"?`))
                                deleteConfigMutation.mutate(config.id);
                            }}
                            className="text-red-400 hover:text-red-600"
                            title="Delete"
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

          {editingId && configs && (
            <EditConfigModal
              config={configs.find(c => c.id === editingId)!}
              storages={storages ?? []}
              error={error}
              isLoading={updateConfigMutation.isPending}
              onSubmit={(data) => updateConfigMutation.mutate({ id: editingId, data })}
              onCancel={() => { setEditingId(null); setError(''); }}
            />
          )}
        </div>
      )}

      {/* History tab */}
      {activeTab === 'history' && (
        <div className="mt-4">
          <div className="flex justify-end mb-4">
            <button
              onClick={() => { setShowTriggerForm(!showTriggerForm); setError(''); }}
              className="flex items-center gap-1 rounded bg-blue-600 px-3 py-1.5 text-sm text-white hover:bg-blue-700"
            >
              <Play className="h-4 w-4" /> Trigger Backup
            </button>
          </div>

          {showTriggerForm && (
            <TriggerForm
              configs={configs ?? []}
              storages={storages ?? []}
              error={error}
              isLoading={triggerBackupMutation.isPending}
              onSubmit={(data) => triggerBackupMutation.mutate(data)}
              onCancel={() => { setShowTriggerForm(false); setError(''); }}
            />
          )}

          {backupsLoading ? (
            <p className="text-gray-400">Loading backup history...</p>
          ) : !backups?.length ? (
            <p className="text-gray-400">No backups yet.</p>
          ) : (
            <div className="overflow-hidden rounded-lg border border-gray-200">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Date</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Config</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">File Path</th>
                    <th className="px-4 py-2 text-right text-xs font-medium uppercase text-gray-500">Size</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Status</th>
                    <th className="px-4 py-2 text-left text-xs font-medium uppercase text-gray-500">Duration</th>
                    <th className="px-4 py-2 text-center text-xs font-medium uppercase text-gray-500">Actions</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-gray-200 bg-white">
                  {backups.map(backup => {
                    const configName = configs?.find(c => c.id === backup.config_id)?.name;
                    return (
                      <tr key={backup.id}>
                        <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                          {new Date(backup.created_at).toLocaleString()}
                        </td>
                        <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                          {configName || (backup.config_id ? 'Deleted config' : 'Manual')}
                        </td>
                        <td className="px-4 py-2 text-sm text-gray-500 max-w-xs truncate" title={backup.file_path}>
                          {backup.file_path || '-'}
                        </td>
                        <td className="whitespace-nowrap px-4 py-2 text-right text-sm text-gray-900">
                          {backup.file_size_bytes > 0 ? formatBytes(backup.file_size_bytes) : '-'}
                        </td>
                        <td className="whitespace-nowrap px-4 py-2 text-sm">
                          {statusBadge(backup.status)}
                          {backup.error_message && (
                            <span className="ml-1 text-xs text-red-500" title={backup.error_message}>
                              (error)
                            </span>
                          )}
                        </td>
                        <td className="whitespace-nowrap px-4 py-2 text-sm text-gray-500">
                          {formatDuration(backup.started_at, backup.completed_at)}
                        </td>
                        <td className="whitespace-nowrap px-4 py-2 text-center">
                          <button
                            onClick={() => {
                              if (window.confirm('Delete this backup record and its file?'))
                                deleteBackupMutation.mutate(backup.id);
                            }}
                            className="text-red-400 hover:text-red-600"
                            title="Delete"
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Config Form (Create) ────────────────────────────────────────────────────

function ConfigForm({
  storages,
  error,
  isLoading,
  onSubmit,
  onCancel,
}: {
  storages: StorageBackend[];
  error: string;
  isLoading: boolean;
  onSubmit: (data: CreateBackupConfigRequest) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState('');
  const [storageId, setStorageId] = useState('');
  const [storagePath, setStoragePath] = useState('backups');
  const [cronPreset, setCronPreset] = useState('0 0 2 * * * *');
  const [customCron, setCustomCron] = useState('');
  const [retentionCount, setRetentionCount] = useState('7');
  const [enabled, setEnabled] = useState(true);

  function handleSubmit() {
    const cron = cronPreset || customCron;
    if (!name || !storageId || !cron) return;
    onSubmit({
      name,
      storage_id: storageId,
      storage_path: storagePath,
      schedule_cron: cron,
      retention_count: parseInt(retentionCount, 10) || 7,
      enabled,
    });
  }

  return (
    <div className="mb-4 rounded-lg border border-gray-200 bg-white p-4">
      <h3 className="font-medium text-gray-800 mb-3">New Backup Config</h3>
      <div className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-xs text-gray-500 mb-1">Name</label>
            <input
              type="text"
              value={name}
              onChange={e => setName(e.target.value)}
              placeholder="e.g. Daily Production Backup"
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500 mb-1">Storage</label>
            <select
              value={storageId}
              onChange={e => setStorageId(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              <option value="">Select storage...</option>
              {storages.filter(s => s.enabled).map(s => (
                <option key={s.id} value={s.id}>{s.name} ({s.storage_type})</option>
              ))}
            </select>
          </div>
        </div>
        <div className="grid grid-cols-3 gap-3">
          <div>
            <label className="block text-xs text-gray-500 mb-1">Storage Path</label>
            <input
              type="text"
              value={storagePath}
              onChange={e => setStoragePath(e.target.value)}
              placeholder="backups"
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500 mb-1">Schedule</label>
            <select
              value={cronPreset}
              onChange={e => setCronPreset(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              {CRON_PRESETS.map(p => (
                <option key={p.value} value={p.value}>{p.label}</option>
              ))}
            </select>
            {cronPreset === '' && (
              <input
                type="text"
                value={customCron}
                onChange={e => setCustomCron(e.target.value)}
                placeholder="0 0 2 * * * *"
                className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              />
            )}
          </div>
          <div>
            <label className="block text-xs text-gray-500 mb-1">Retention Count</label>
            <input
              type="number"
              value={retentionCount}
              onChange={e => setRetentionCount(e.target.value)}
              min="1"
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
        </div>
        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id="enabled"
            checked={enabled}
            onChange={e => setEnabled(e.target.checked)}
            className="rounded border-gray-300"
          />
          <label htmlFor="enabled" className="text-sm text-gray-700">Enabled</label>
        </div>
        {error && <p className="text-sm text-red-600">{error}</p>}
        <div className="flex gap-2">
          <button
            onClick={handleSubmit}
            disabled={isLoading || !name || !storageId}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {isLoading ? 'Creating...' : 'Create'}
          </button>
          <button
            onClick={onCancel}
            className="rounded bg-gray-200 px-4 py-1.5 text-sm text-gray-700 hover:bg-gray-300"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Edit Config Modal ───────────────────────────────────────────────────────

function EditConfigModal({
  config,
  storages,
  error,
  isLoading,
  onSubmit,
  onCancel,
}: {
  config: BackupConfig;
  storages: StorageBackend[];
  error: string;
  isLoading: boolean;
  onSubmit: (data: UpdateBackupConfigRequest) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(config.name);
  const [storageId, setStorageId] = useState(config.storage_id);
  const [storagePath, setStoragePath] = useState(config.storage_path);
  const [scheduleCron, setScheduleCron] = useState(config.schedule_cron);
  const [retentionCount, setRetentionCount] = useState(String(config.retention_count));
  const [enabled, setEnabled] = useState(config.enabled);

  function handleSubmit() {
    if (!name) return;
    onSubmit({
      name,
      storage_id: storageId,
      storage_path: storagePath,
      schedule_cron: scheduleCron,
      retention_count: parseInt(retentionCount, 10) || 7,
      enabled,
    });
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
      <div className="w-full max-w-lg rounded-lg border border-gray-200 bg-white p-6 shadow-lg">
        <div className="mb-4 flex items-center justify-between">
          <h3 className="text-lg font-medium text-gray-800">Edit: {config.name}</h3>
          <button onClick={onCancel} className="text-gray-400 hover:text-gray-600">
            <X className="h-5 w-5" />
          </button>
        </div>
        <div className="space-y-3">
          <div>
            <label className="block text-xs text-gray-500 mb-1">Name</label>
            <input
              type="text"
              value={name}
              onChange={e => setName(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            />
          </div>
          <div>
            <label className="block text-xs text-gray-500 mb-1">Storage</label>
            <select
              value={storageId}
              onChange={e => setStorageId(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              {storages.filter(s => s.enabled).map(s => (
                <option key={s.id} value={s.id}>{s.name} ({s.storage_type})</option>
              ))}
            </select>
          </div>
          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs text-gray-500 mb-1">Storage Path</label>
              <input
                type="text"
                value={storagePath}
                onChange={e => setStoragePath(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 mb-1">Cron Expression</label>
              <input
                type="text"
                value={scheduleCron}
                onChange={e => setScheduleCron(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              />
            </div>
            <div>
              <label className="block text-xs text-gray-500 mb-1">Retention</label>
              <input
                type="number"
                value={retentionCount}
                onChange={e => setRetentionCount(e.target.value)}
                min="1"
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              />
            </div>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="checkbox"
              id="edit-enabled"
              checked={enabled}
              onChange={e => setEnabled(e.target.checked)}
              className="rounded border-gray-300"
            />
            <label htmlFor="edit-enabled" className="text-sm text-gray-700">Enabled</label>
          </div>
          {error && <p className="text-sm text-red-600">{error}</p>}
          <div className="flex gap-2">
            <button
              onClick={handleSubmit}
              disabled={isLoading || !name}
              className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
            >
              {isLoading ? 'Saving...' : 'Save'}
            </button>
            <button
              onClick={onCancel}
              className="rounded bg-gray-200 px-4 py-1.5 text-sm text-gray-700 hover:bg-gray-300"
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Trigger Form ────────────────────────────────────────────────────────────

function TriggerForm({
  configs,
  storages,
  error,
  isLoading,
  onSubmit,
  onCancel,
}: {
  configs: BackupConfig[];
  storages: StorageBackend[];
  error: string;
  isLoading: boolean;
  onSubmit: (data: TriggerBackupRequest) => void;
  onCancel: () => void;
}) {
  const [mode, setMode] = useState<'config' | 'manual'>('config');
  const [configId, setConfigId] = useState('');
  const [storageId, setStorageId] = useState('');
  const [storagePath, setStoragePath] = useState('backups');

  function handleSubmit() {
    if (mode === 'config') {
      if (!configId) return;
      onSubmit({ config_id: configId });
    } else {
      if (!storageId) return;
      onSubmit({ storage_id: storageId, storage_path: storagePath });
    }
  }

  return (
    <div className="mb-4 rounded-lg border border-gray-200 bg-white p-4">
      <h3 className="font-medium text-gray-800 mb-3">Trigger Manual Backup</h3>
      <div className="space-y-3">
        <div className="flex gap-4">
          <label className="flex items-center gap-1.5 text-sm">
            <input
              type="radio"
              name="trigger-mode"
              checked={mode === 'config'}
              onChange={() => setMode('config')}
            />
            From config
          </label>
          <label className="flex items-center gap-1.5 text-sm">
            <input
              type="radio"
              name="trigger-mode"
              checked={mode === 'manual'}
              onChange={() => setMode('manual')}
            />
            Manual
          </label>
        </div>

        {mode === 'config' ? (
          <div>
            <label className="block text-xs text-gray-500 mb-1">Backup Config</label>
            <select
              value={configId}
              onChange={e => setConfigId(e.target.value)}
              className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            >
              <option value="">Select config...</option>
              {configs.map(c => (
                <option key={c.id} value={c.id}>{c.name}</option>
              ))}
            </select>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-gray-500 mb-1">Storage</label>
              <select
                value={storageId}
                onChange={e => setStorageId(e.target.value)}
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              >
                <option value="">Select storage...</option>
                {storages.filter(s => s.enabled).map(s => (
                  <option key={s.id} value={s.id}>{s.name} ({s.storage_type})</option>
                ))}
              </select>
            </div>
            <div>
              <label className="block text-xs text-gray-500 mb-1">Storage Path</label>
              <input
                type="text"
                value={storagePath}
                onChange={e => setStoragePath(e.target.value)}
                placeholder="backups"
                className="w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
              />
            </div>
          </div>
        )}

        {error && <p className="text-sm text-red-600">{error}</p>}

        <div className="flex gap-2">
          <button
            onClick={handleSubmit}
            disabled={isLoading || (mode === 'config' ? !configId : !storageId)}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {isLoading ? 'Triggering...' : 'Trigger'}
          </button>
          <button
            onClick={onCancel}
            className="rounded bg-gray-200 px-4 py-1.5 text-sm text-gray-700 hover:bg-gray-300"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
