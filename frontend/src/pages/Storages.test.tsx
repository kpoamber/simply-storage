import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import Storages from './Storages';

vi.mock('../api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
}));

vi.mock('../contexts/AuthContext', () => ({
  useAuth: () => ({
    user: {
      id: 'current-admin',
      username: 'admin',
      role: 'admin',
      created_at: '2026-01-01',
      updated_at: '2026-01-01',
    },
  }),
}));

import apiClient from '../api/client';
const mockGet = vi.mocked(apiClient.get);
const mockPost = vi.mocked(apiClient.post);

const mockStorages = [
  {
    id: 's1', name: 'Local Primary', storage_type: 'local',
    is_hot: true, enabled: true, file_count: 25, used_space: 1048576,
    config: { path: '/data' }, project_id: null, created_at: '', updated_at: '',
  },
  {
    id: 's2', name: 'S3 Backup', storage_type: 's3',
    is_hot: false, enabled: false, file_count: 10, used_space: 524288,
    config: { bucket: 'backup' }, project_id: null, created_at: '', updated_at: '',
  },
];

function renderStorages() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Storages />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('Storages', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders heading and subtitle', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderStorages();
    expect(screen.getByText('Storages')).toBeInTheDocument();
    expect(screen.getByText('Manage storage backends.')).toBeInTheDocument();
  });

  it('shows storages in a table', async () => {
    mockGet.mockResolvedValue({ data: mockStorages });
    renderStorages();

    await waitFor(() => {
      expect(screen.getByText('Local Primary')).toBeInTheDocument();
    });
    expect(screen.getByText('local')).toBeInTheDocument();
    expect(screen.getByText('Hot')).toBeInTheDocument();
    expect(screen.getByText('Enabled')).toBeInTheDocument();
    expect(screen.getByText('25')).toBeInTheDocument();

    expect(screen.getByText('S3 Backup')).toBeInTheDocument();
    expect(screen.getByText('s3')).toBeInTheDocument();
    expect(screen.getByText('Cold')).toBeInTheDocument();
    expect(screen.getByText('Disabled')).toBeInTheDocument();
  });

  it('shows empty state when no storages', async () => {
    mockGet.mockResolvedValue({ data: [] });
    renderStorages();

    await waitFor(() => {
      expect(screen.getByText('No storages configured.')).toBeInTheDocument();
    });
  });

  it('shows add storage form when button clicked', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderStorages();

    fireEvent.click(screen.getByText('Add Storage'));
    expect(screen.getByText('Add Storage', { selector: 'h3' })).toBeInTheDocument();
    expect(screen.getByPlaceholderText('My Storage')).toBeInTheDocument();
  });

  it('submits create form with correct data', async () => {
    mockGet.mockResolvedValue({ data: [] });
    mockPost.mockResolvedValue({ data: { id: 'new' } });

    renderStorages();

    await waitFor(() => {
      expect(screen.getByText('No storages configured.')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Add Storage'));
    fireEvent.change(screen.getByPlaceholderText('My Storage'), { target: { value: 'New Storage' } });
    fireEvent.change(screen.getByPlaceholderText('/data/storage'), { target: { value: '/mnt/data' } });
    fireEvent.click(screen.getByText('Create'));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/storages', expect.objectContaining({
        name: 'New Storage',
        storage_type: 'local',
        is_hot: true,
      }));
    });
  });

  it('has view and edit action buttons for each storage', async () => {
    mockGet.mockResolvedValue({ data: [mockStorages[0]] });
    renderStorages();

    await waitFor(() => {
      expect(screen.getByText('Local Primary')).toBeInTheDocument();
    });

    expect(screen.getByTitle('View')).toBeInTheDocument();
    expect(screen.getByTitle('Edit')).toBeInTheDocument();
    expect(screen.getByTitle('Disable')).toBeInTheDocument();
  });
});
