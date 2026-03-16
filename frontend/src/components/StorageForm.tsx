import { useState } from 'react';

const STORAGE_TYPES = [
  { value: 'local', label: 'Local Disk' },
  { value: 's3', label: 'S3 / DigitalOcean Spaces' },
  { value: 'azure', label: 'Azure Blob Storage' },
  { value: 'gcs', label: 'Google Cloud Storage' },
  { value: 'ftp', label: 'FTP' },
  { value: 'sftp', label: 'SFTP' },
  { value: 'samba', label: 'Samba / SMB' },
  { value: 'hetzner', label: 'Hetzner StorageBox' },
];

interface StorageFormField {
  key: string;
  label: string;
  type?: string;
  placeholder?: string;
  required?: boolean;
  sensitive?: boolean;
}

const STORAGE_TYPE_FIELDS: Record<string, StorageFormField[]> = {
  local: [
    { key: 'path', label: 'Storage Path', placeholder: '/data/storage', required: true },
  ],
  s3: [
    { key: 'region', label: 'Region', placeholder: 'us-east-1', required: true },
    { key: 'endpoint_url', label: 'Endpoint URL (for DO Spaces)', placeholder: 'https://ams3.digitaloceanspaces.com' },
    { key: 'access_key_id', label: 'Access Key ID', required: true, sensitive: true },
    { key: 'secret_access_key', label: 'Secret Access Key', type: 'password', required: true, sensitive: true },
  ],
  azure: [
    { key: 'account_name', label: 'Account Name', required: true },
    { key: 'account_key', label: 'Account Key', type: 'password', required: true, sensitive: true },
    { key: 'endpoint', label: 'Endpoint (optional)', placeholder: 'https://account.blob.core.windows.net' },
  ],
  gcs: [
    { key: 'client_email', label: 'Client Email', placeholder: 'sa@project.iam.gserviceaccount.com', required: true, sensitive: true },
    { key: 'private_key_pem', label: 'Private Key (PEM)', type: 'textarea', required: true, sensitive: true },
    { key: 'token_uri', label: 'Token URI (optional)', placeholder: 'https://oauth2.googleapis.com/token' },
  ],
  ftp: [
    { key: 'host', label: 'Host', placeholder: 'ftp.example.com', required: true },
    { key: 'port', label: 'Port', placeholder: '21', type: 'number' },
    { key: 'username', label: 'Username', required: true },
    { key: 'password', label: 'Password', type: 'password', required: true, sensitive: true },
    { key: 'base_path', label: 'Base Path', placeholder: '/files' },
  ],
  sftp: [
    { key: 'host', label: 'Host', placeholder: 'sftp.example.com', required: true },
    { key: 'port', label: 'Port', placeholder: '22', type: 'number' },
    { key: 'username', label: 'Username', required: true },
    { key: 'password', label: 'Password', type: 'password', required: true, sensitive: true },
    { key: 'base_path', label: 'Base Path', placeholder: '/files' },
  ],
  samba: [
    { key: 'host', label: 'Host', placeholder: '192.168.1.100', required: true },
    { key: 'share', label: 'Share Name', placeholder: 'files', required: true },
    { key: 'username', label: 'Username', required: true },
    { key: 'password', label: 'Password', type: 'password', required: true, sensitive: true },
  ],
  hetzner: [
    { key: 'host', label: 'Host', placeholder: 'uXXXXXX.your-storagebox.de', required: true },
    { key: 'username', label: 'Username', required: true },
    { key: 'password', label: 'Password', type: 'password', required: true, sensitive: true },
    { key: 'port', label: 'Port', placeholder: '443', type: 'number' },
    { key: 'base_path', label: 'Base Path', placeholder: '/files' },
  ],
};

interface StorageFormProps {
  initialValues?: {
    name: string;
    storage_type: string;
    config: Record<string, unknown>;
    is_hot: boolean;
    supports_direct_links: boolean;
  };
  onSubmit: (data: { name: string; storage_type: string; config: Record<string, unknown>; is_hot: boolean; supports_direct_links: boolean }) => void;
  onCancel: () => void;
  isLoading: boolean;
  isEdit?: boolean;
}

export default function StorageForm({ initialValues, onSubmit, onCancel, isLoading, isEdit }: StorageFormProps) {
  const [name, setName] = useState(initialValues?.name ?? '');
  const [storageType, setStorageType] = useState(initialValues?.storage_type ?? 'local');
  const [isHot, setIsHot] = useState(initialValues?.is_hot ?? true);
  const [supportsDirectLinks, setSupportsDirectLinks] = useState(initialValues?.supports_direct_links ?? false);
  const sensitiveKeys = new Set(
    (STORAGE_TYPE_FIELDS[initialValues?.storage_type ?? storageType] ?? [])
      .filter(f => f.sensitive)
      .map(f => f.key)
  );

  const [configValues, setConfigValues] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {};
    if (initialValues?.config) {
      for (const [k, v] of Object.entries(initialValues.config)) {
        // In edit mode, don't pre-fill sensitive fields (they come as "***")
        if (isEdit && sensitiveKeys.has(k)) continue;
        initial[k] = String(v ?? '');
      }
    }
    return initial;
  });

  const fields = STORAGE_TYPE_FIELDS[storageType] ?? [];

  const handleTypeChange = (newType: string) => {
    setStorageType(newType);
    if (!isEdit) {
      setConfigValues({});
    }
  };

  const handleConfigChange = (key: string, value: string) => {
    setConfigValues(prev => ({ ...prev, [key]: value }));
  };

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    const config: Record<string, unknown> = {};
    for (const field of fields) {
      const val = configValues[field.key] ?? '';
      // In edit mode, skip empty sensitive fields — server preserves the original
      if (isEdit && field.sensitive && !val) continue;
      if (val) {
        config[field.key] = field.type === 'number' ? parseInt(val, 10) : val;
      }
    }
    onSubmit({ name, storage_type: storageType, config, is_hot: isHot, supports_direct_links: supportsDirectLinks });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <div className="grid grid-cols-2 gap-4">
        <div>
          <label className="block text-xs font-medium text-gray-500">Name</label>
          <input
            value={name}
            onChange={e => setName(e.target.value)}
            required
            placeholder="My Storage"
            className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-500">Storage Type</label>
          <select
            value={storageType}
            onChange={e => handleTypeChange(e.target.value)}
            disabled={isEdit}
            className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
            aria-label="Storage Type"
          >
            {STORAGE_TYPES.map(t => (
              <option key={t.value} value={t.value}>{t.label}</option>
            ))}
          </select>
        </div>
      </div>

      <div className="space-y-2">
        <label className="flex items-center gap-2 text-sm text-gray-700">
          <input
            type="checkbox"
            checked={isHot}
            onChange={e => setIsHot(e.target.checked)}
            className="rounded border-gray-300"
          />
          Hot storage (fast access tier)
        </label>
        <label className="flex items-center gap-2 text-sm text-gray-700">
          <input
            type="checkbox"
            checked={supportsDirectLinks}
            onChange={e => setSupportsDirectLinks(e.target.checked)}
            className="rounded border-gray-300"
          />
          Supports direct links (generate presigned URLs)
        </label>
      </div>

      {fields.length > 0 && (
        <div>
          <h4 className="mb-2 text-sm font-medium text-gray-700">Configuration</h4>
          <div className="grid grid-cols-2 gap-3">
            {fields.map(field => {
              const isSensitiveEdit = isEdit && field.sensitive;
              const placeholder = isSensitiveEdit
                ? 'Leave empty to keep current value'
                : field.placeholder;
              const required = isSensitiveEdit ? false : field.required;

              return (
                <div key={field.key} className={field.type === 'textarea' ? 'col-span-2' : ''}>
                  <label className="block text-xs text-gray-500">
                    {field.label}{field.required && !isSensitiveEdit && ' *'}
                    {isSensitiveEdit && <span className="ml-1 text-gray-400">(saved)</span>}
                  </label>
                  {field.type === 'textarea' ? (
                    <textarea
                      value={configValues[field.key] ?? ''}
                      onChange={e => handleConfigChange(field.key, e.target.value)}
                      placeholder={placeholder}
                      required={required}
                      rows={4}
                      autoComplete="off"
                      className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm font-mono"
                    />
                  ) : (
                    <input
                      type={isSensitiveEdit && !configValues[field.key] ? 'text' : (field.type ?? 'text')}
                      value={configValues[field.key] ?? ''}
                      onChange={e => handleConfigChange(field.key, e.target.value)}
                      placeholder={placeholder}
                      required={required}
                      autoComplete="new-password"
                      data-1p-ignore
                      data-lpignore="true"
                      readOnly={isSensitiveEdit && !configValues[field.key]}
                      onFocus={e => { if (e.target.readOnly) e.target.readOnly = false; }}
                      className="mt-1 w-full rounded border border-gray-300 px-3 py-1.5 text-sm"
                    />
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      <div className="flex gap-2">
        <button
          type="submit"
          disabled={isLoading}
          className="rounded bg-blue-600 px-4 py-1.5 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
        >
          {isLoading ? 'Saving...' : isEdit ? 'Update' : 'Create'}
        </button>
        <button type="button" onClick={onCancel} className="rounded bg-gray-200 px-4 py-1.5 text-sm hover:bg-gray-300">
          Cancel
        </button>
      </div>
    </form>
  );
}

export { STORAGE_TYPES, STORAGE_TYPE_FIELDS };
