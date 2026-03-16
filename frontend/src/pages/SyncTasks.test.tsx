import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import SyncTasks from './SyncTasks';

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

const mockSyncTasks = [
  {
    id: 'st1', file_id: 'f1111111-aaaa-bbbb-cccc-dddddddddddd',
    source_storage_id: 's1', target_storage_id: 's2',
    status: 'pending', retries: 0, error_msg: null, project_id: null,
    created_at: '2026-01-15T10:00:00Z', updated_at: '2026-01-15T10:00:00Z',
  },
  {
    id: 'st2', file_id: 'f2222222-aaaa-bbbb-cccc-dddddddddddd',
    source_storage_id: 's1', target_storage_id: 's3',
    status: 'failed', retries: 3, error_msg: 'Connection timeout', project_id: null,
    created_at: '2026-01-14T10:00:00Z', updated_at: '2026-01-14T12:00:00Z',
  },
  {
    id: 'st3', file_id: 'f3333333-aaaa-bbbb-cccc-dddddddddddd',
    source_storage_id: 's2', target_storage_id: 's1',
    status: 'completed', retries: 0, error_msg: null, project_id: null,
    created_at: '2026-01-13T10:00:00Z', updated_at: '2026-01-13T11:00:00Z',
  },
];

const mockStorages = [
  { id: 's1', name: 'Local Primary', storage_type: 'local', is_hot: true, enabled: true, supports_direct_links: false, file_count: 0, used_space: 0, config: {}, project_id: null, created_at: '', updated_at: '' },
  { id: 's2', name: 'S3 Backup', storage_type: 's3', is_hot: false, enabled: true, supports_direct_links: false, file_count: 0, used_space: 0, config: {}, project_id: null, created_at: '', updated_at: '' },
  { id: 's3', name: 'Azure Cold', storage_type: 'azure', is_hot: false, enabled: true, supports_direct_links: false, file_count: 0, used_space: 0, config: {}, project_id: null, created_at: '', updated_at: '' },
];

function renderSyncTasks() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <SyncTasks />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('SyncTasks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders heading and subtitle', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderSyncTasks();
    expect(screen.getByText('Sync Tasks')).toBeInTheDocument();
    expect(screen.getByText('Monitor file synchronization tasks.')).toBeInTheDocument();
  });

  it('shows sync tasks in a table with storage names', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/sync-tasks') return Promise.resolve({ data: mockSyncTasks });
      if (url === '/storages') return Promise.resolve({ data: mockStorages });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderSyncTasks();

    await waitFor(() => {
      expect(screen.getByText('Connection timeout')).toBeInTheDocument();
    });
    expect(screen.getByText('3')).toBeInTheDocument();
  });

  it('resolves storage names from storages query', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/sync-tasks') return Promise.resolve({ data: [mockSyncTasks[0]] });
      if (url === '/storages') return Promise.resolve({ data: mockStorages });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderSyncTasks();

    await waitFor(() => {
      expect(screen.getByText('Local Primary')).toBeInTheDocument();
    });
    expect(screen.getByText('S3 Backup')).toBeInTheDocument();
  });

  it('shows empty state when no sync tasks', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/sync-tasks') return Promise.resolve({ data: [] });
      if (url === '/storages') return Promise.resolve({ data: [] });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderSyncTasks();

    await waitFor(() => {
      expect(screen.getByText('No sync tasks found.')).toBeInTheDocument();
    });
  });

  it('has status filter dropdown', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderSyncTasks();

    const select = screen.getByLabelText('Filter by status');
    expect(select).toBeInTheDocument();
    expect(select).toHaveValue('all');
  });

  it('changes filter and refetches', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/sync-tasks') return Promise.resolve({ data: mockSyncTasks });
      if (url === '/storages') return Promise.resolve({ data: mockStorages });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderSyncTasks();

    await waitFor(() => {
      expect(screen.getByText('Connection timeout')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Filter by status'), { target: { value: 'failed' } });

    await waitFor(() => {
      expect(mockGet).toHaveBeenCalledWith('/sync-tasks', expect.objectContaining({
        params: expect.objectContaining({ status: 'failed' }),
      }));
    });
  });

  it('shows pagination controls', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/sync-tasks') return Promise.resolve({ data: mockSyncTasks });
      if (url === '/storages') return Promise.resolve({ data: mockStorages });
      return Promise.reject(new Error('Unknown URL'));
    });

    renderSyncTasks();

    await waitFor(() => {
      expect(screen.getByText('Page 1')).toBeInTheDocument();
    });
    expect(screen.getByText('Previous')).toBeInTheDocument();
    expect(screen.getByText('Next')).toBeInTheDocument();
  });
});
