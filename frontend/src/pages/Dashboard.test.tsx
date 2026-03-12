import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import Dashboard from './Dashboard';

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

function renderDashboard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('Dashboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders heading and subtitle', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderDashboard();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
    expect(screen.getByText('System overview and statistics.')).toBeInTheDocument();
  });

  it('renders stat card labels', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderDashboard();
    expect(screen.getByText('Total Files')).toBeInTheDocument();
    expect(screen.getByText('Storage Used')).toBeInTheDocument();
    expect(screen.getByText('Pending Sync Tasks')).toBeInTheDocument();
    expect(screen.getByText('Active Nodes')).toBeInTheDocument();
  });

  it('shows stat values after data loads', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/system/stats') {
        return Promise.resolve({
          data: { total_files: 42, total_storage_used: 1073741824, pending_sync_tasks: 5 },
        });
      }
      if (url === '/storages') {
        return Promise.resolve({ data: [] });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('42')).toBeInTheDocument();
    });
    expect(screen.getByText('1 GB')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('shows storage health table with storage entries', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/system/stats') {
        return Promise.resolve({
          data: { total_files: 0, total_storage_used: 0, pending_sync_tasks: 0 },
        });
      }
      if (url === '/storages') {
        return Promise.resolve({
          data: [
            {
              id: '1', name: 'S3 Primary', storage_type: 's3',
              is_hot: true, enabled: true, file_count: 10, used_space: 2048,
              config: {}, project_id: null, created_at: '', updated_at: '',
            },
            {
              id: '2', name: 'Cold Archive', storage_type: 'azure',
              is_hot: false, enabled: false, file_count: 0, used_space: 0,
              config: {}, project_id: null, created_at: '', updated_at: '',
            },
          ],
        });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('S3 Primary')).toBeInTheDocument();
    });
    expect(screen.getByText('s3')).toBeInTheDocument();
    expect(screen.getByText('Hot')).toBeInTheDocument();
    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(screen.getByText('Cold Archive')).toBeInTheDocument();
    expect(screen.getByText('Cold')).toBeInTheDocument();
    expect(screen.getByText('Disabled')).toBeInTheDocument();
  });

  it('shows empty state when no storages', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/system/stats') {
        return Promise.resolve({
          data: { total_files: 0, total_storage_used: 0, pending_sync_tasks: 0 },
        });
      }
      if (url === '/storages') {
        return Promise.resolve({ data: [] });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('No storages configured.')).toBeInTheDocument();
    });
  });
});
