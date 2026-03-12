import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import StorageDetail from './StorageDetail';

vi.mock('../api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}));

import apiClient from '../api/client';
const mockGet = vi.mocked(apiClient.get);
const mockPost = vi.mocked(apiClient.post);

const mockStorage = {
  id: 's1', name: 'Local Primary', storage_type: 'local',
  is_hot: true, enabled: true, file_count: 25, used_space: 1048576,
  config: { path: '/data' }, project_id: null, created_at: '', updated_at: '',
};

const mockFileLocations = [
  {
    id: 'fl1', file_id: 'f1111111-aaaa-bbbb-cccc-dddddddddddd', storage_id: 's1',
    storage_path: 'f1/11/f1111111', status: 'synced',
    synced_at: '2026-01-15T10:00:00Z', last_accessed_at: '2026-01-20T12:00:00Z', created_at: '2026-01-15T10:00:00Z',
  },
  {
    id: 'fl2', file_id: 'f2222222-aaaa-bbbb-cccc-dddddddddddd', storage_id: 's1',
    storage_path: 'f2/22/f2222222', status: 'pending',
    synced_at: null, last_accessed_at: null, created_at: '2026-01-16T10:00:00Z',
  },
];

function renderStorageDetail(storageId = 's1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/storages/${storageId}`]}>
        <Routes>
          <Route path="/storages/:id" element={<StorageDetail />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('StorageDetail', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows loading state', () => {
    mockGet.mockReturnValue(new Promise(() => {}));
    renderStorageDetail();
    expect(screen.getByText('Loading storage...')).toBeInTheDocument();
  });

  it('shows storage not found when data is null', async () => {
    mockGet.mockRejectedValue(new Error('Not found'));
    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Storage not found.')).toBeInTheDocument();
    });
  });

  it('renders storage details and stats', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: mockFileLocations });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'none', percentage: 0, file_count: 0 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Local Primary')).toBeInTheDocument();
    });
    expect(screen.getByText('25')).toBeInTheDocument();
    expect(screen.getByText('1 MB')).toBeInTheDocument();
  });

  it('renders file locations table', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: mockFileLocations });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'none', percentage: 0, file_count: 0 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('f1/11/f1111111')).toBeInTheDocument();
    });
    expect(screen.getByText('synced')).toBeInTheDocument();
    expect(screen.getByText('f2/22/f2222222')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
  });

  it('has sync-all and export buttons', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: [] });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'none', percentage: 0, file_count: 0 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Sync All')).toBeInTheDocument();
    });
    expect(screen.getByText('Export')).toBeInTheDocument();
  });

  it('clicking Sync All triggers API call', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: [] });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'none', percentage: 0, file_count: 0 } });
      return Promise.reject(new Error('Unknown URL'));
    });
    mockPost.mockResolvedValue({ data: { created: 5 } });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Sync All')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Sync All'));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/storages/s1/sync-all');
    });
  });

  it('shows empty state when no files', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: [] });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'none', percentage: 0, file_count: 0 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('No files on this storage.')).toBeInTheDocument();
    });
  });

  it('shows export progress when in progress', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: [] });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'in_progress', percentage: 45, file_count: 12 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Export Status')).toBeInTheDocument();
    });
    expect(screen.getByText('12 files')).toBeInTheDocument();
  });

  it('shows download link when export completed', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/storages/s1') return Promise.resolve({ data: mockStorage });
      if (url.includes('/files')) return Promise.resolve({ data: [] });
      if (url.includes('/export/status')) return Promise.resolve({ data: { status: 'completed', percentage: 100, file_count: 25 } });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderStorageDetail();

    await waitFor(() => {
      expect(screen.getByText('Download archive')).toBeInTheDocument();
    });
  });
});
