import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import { Download, Lock, FileText, AlertCircle } from 'lucide-react';
import axios from 'axios';
import { SharedLinkInfo, formatBytes } from '../api/types';

export default function SharedLinkAccess() {
  const { token } = useParams<{ token: string }>();
  const [password, setPassword] = useState('');
  const [passwordError, setPasswordError] = useState('');
  const [isDownloading, setIsDownloading] = useState(false);
  const [isVerifying, setIsVerifying] = useState(false);

  const { data: linkInfo, isLoading, error } = useQuery<SharedLinkInfo>({
    queryKey: ['shared-link-info', token],
    queryFn: () => axios.get(`/s/${token}`).then(r => r.data),
    enabled: !!token,
    retry: false,
  });

  async function handlePublicDownload() {
    if (!token) return;
    setIsDownloading(true);
    try {
      const resp = await axios.get(`/s/${token}/download`, { responseType: 'blob' });
      triggerDownload(resp.data, linkInfo?.file_name || 'download');
    } catch {
      setPasswordError('Download failed. Please try again.');
    } finally {
      setIsDownloading(false);
    }
  }

  async function handlePasswordSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!token || !password) return;
    setPasswordError('');
    setIsVerifying(true);

    try {
      const verifyResp = await axios.post(`/s/${token}/verify`, { password });
      const dlToken = verifyResp.data.dl_token;

      setIsDownloading(true);
      const resp = await axios.get(`/s/${token}/download`, {
        params: { dl_token: dlToken },
        responseType: 'blob',
      });
      triggerDownload(resp.data, linkInfo?.file_name || 'download');
    } catch (err: unknown) {
      const status = (err as { response?: { status?: number } })?.response?.status;
      if (status === 403) {
        setPasswordError('Wrong password. Please try again.');
      } else {
        setPasswordError('Download failed. Please try again.');
      }
    } finally {
      setIsVerifying(false);
      setIsDownloading(false);
    }
  }

  function triggerDownload(blob: Blob, filename: string) {
    const url = window.URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    window.URL.revokeObjectURL(url);
    a.remove();
  }

  if (isLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-50">
        <p className="text-gray-500">Loading...</p>
      </div>
    );
  }

  if (error || !linkInfo) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-50">
        <div className="text-center" data-testid="link-unavailable">
          <AlertCircle className="mx-auto h-12 w-12 text-gray-400" />
          <h2 className="mt-4 text-xl font-semibold text-gray-700">Link Unavailable</h2>
          <p className="mt-2 text-gray-500">
            This shared link has expired, been deactivated, or does not exist.
          </p>
        </div>
      </div>
    );
  }

  const isExpired = linkInfo.expires_at && new Date(linkInfo.expires_at) < new Date();

  if (isExpired) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-gray-50">
        <div className="text-center" data-testid="link-expired">
          <AlertCircle className="mx-auto h-12 w-12 text-orange-400" />
          <h2 className="mt-4 text-xl font-semibold text-gray-700">Link Expired</h2>
          <p className="mt-2 text-gray-500">This shared link has expired and is no longer available.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50">
      <div className="w-full max-w-md rounded-lg border border-gray-200 bg-white p-6 shadow-sm" data-testid="link-access-card">
        <div className="text-center">
          <FileText className="mx-auto h-12 w-12 text-blue-500" />
          <h2 className="mt-3 text-lg font-semibold text-gray-800" data-testid="file-name">
            {linkInfo.file_name}
          </h2>
          <div className="mt-1 flex items-center justify-center gap-3 text-sm text-gray-500">
            <span data-testid="file-size">{formatBytes(linkInfo.file_size)}</span>
            <span>&middot;</span>
            <span data-testid="content-type">{linkInfo.content_type}</span>
          </div>
          {linkInfo.expires_at && (
            <p className="mt-1 text-xs text-gray-400">
              Expires: {new Date(linkInfo.expires_at).toLocaleString()}
            </p>
          )}
        </div>

        <div className="mt-6">
          {linkInfo.password_required ? (
            <form onSubmit={handlePasswordSubmit} data-testid="password-form">
              <div className="flex items-center gap-2 mb-3 text-sm text-orange-600">
                <Lock className="h-4 w-4" />
                <span>This file is password-protected</span>
              </div>
              <input
                type="password"
                value={password}
                onChange={e => { setPassword(e.target.value); setPasswordError(''); }}
                placeholder="Enter password"
                className="w-full rounded border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
                data-testid="password-field"
                autoFocus
              />
              {passwordError && (
                <p className="mt-2 text-sm text-red-600" data-testid="password-error">
                  {passwordError}
                </p>
              )}
              <button
                type="submit"
                disabled={!password || isVerifying || isDownloading}
                className="mt-3 flex w-full items-center justify-center gap-2 rounded bg-blue-600 px-4 py-2 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
                data-testid="download-button"
              >
                <Download className="h-4 w-4" />
                {isVerifying ? 'Verifying...' : isDownloading ? 'Downloading...' : 'Download'}
              </button>
            </form>
          ) : (
            <div>
              {passwordError && (
                <p className="mb-3 text-sm text-red-600" data-testid="download-error">
                  {passwordError}
                </p>
              )}
              <button
                onClick={handlePublicDownload}
                disabled={isDownloading}
                className="flex w-full items-center justify-center gap-2 rounded bg-blue-600 px-4 py-2 text-sm text-white hover:bg-blue-700 disabled:opacity-50"
                data-testid="download-button"
              >
                <Download className="h-4 w-4" />
                {isDownloading ? 'Downloading...' : 'Download'}
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
