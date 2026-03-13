import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import ProjectDetail from './ProjectDetail';

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
    isLoading: false,
  }),
}));

import apiClient from '../api/client';
const mockGet = vi.mocked(apiClient.get);
const mockPost = vi.mocked(apiClient.post);
const mockDelete = vi.mocked(apiClient.delete);

function renderProjectDetail(projectId: string) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/projects/${projectId}`]}>
        <Routes>
          <Route path="/projects/:id" element={<ProjectDetail />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockProject = {
  id: 'proj-1',
  name: 'Test Project',
  slug: 'test-project',
  hot_to_cold_days: 14,
  owner_id: 'owner-1',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
};

const mockProjectDetail = {
  project: mockProject,
  stats: { file_count: 3, total_size: 5120 },
};

const mockFiles = [
  {
    id: 'fr-1', file_id: 'f-1', project_id: 'proj-1',
    original_name: 'document.pdf', created_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'fr-2', file_id: 'f-2', project_id: 'proj-1',
    original_name: 'image.png', created_at: '2026-01-15T00:00:00Z',
  },
];

const mockMembers = [
  { id: 'owner-1', username: 'projectowner', role: 'user', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', assigned_at: '2026-01-15T00:00:00Z' },
  { id: 'member-1', username: 'memberuser', role: 'user', created_at: '2026-02-01T00:00:00Z', updated_at: '2026-02-01T00:00:00Z', assigned_at: '2026-02-10T00:00:00Z' },
];

function setupMocks(files = mockFiles, members = mockMembers) {
  mockGet.mockImplementation((url: string) => {
    if (url === '/projects/proj-1') return Promise.resolve({ data: mockProjectDetail });
    if (url.startsWith('/projects/proj-1/files')) return Promise.resolve({ data: files });
    if (url === '/projects/proj-1/storages') return Promise.resolve({ data: [] });
    if (url === '/projects/proj-1/available-storages') return Promise.resolve({ data: [] });
    if (url === '/projects/proj-1/members') return Promise.resolve({ data: members });
    if (url === '/auth/users/owner-1') return Promise.resolve({ data: { user: mockMembers[0], projects: [], storages: [] } });
    if (url === '/auth/users') return Promise.resolve({ data: [
      ...members,
      { id: 'other-1', username: 'otheruser', role: 'user', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', assigned_at: '2026-01-01T00:00:00Z' },
    ] });
    return Promise.reject(new Error('Unknown URL'));
  });
}

describe('ProjectDetail', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows loading state initially', () => {
    mockGet.mockReturnValue(new Promise(() => {}));
    renderProjectDetail('proj-1');
    expect(screen.getByText('Loading project...')).toBeInTheDocument();
  });

  it('shows project name and stats after loading', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByRole('heading', { level: 2, name: 'Test Project' })).toBeInTheDocument();
    });
    expect(screen.getByText(/3 files/)).toBeInTheDocument();
  });

  it('shows file browser with file names', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('document.pdf')).toBeInTheDocument();
    });
    expect(screen.getByText('image.png')).toBeInTheDocument();
  });

  it('shows upload zone with drag-and-drop text', async () => {
    setupMocks([]);
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText(/Drag and drop files here/)).toBeInTheDocument();
    });
  });

  it('shows project settings section', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Settings')).toBeInTheDocument();
    });
    expect(screen.getByText('test-project')).toBeInTheDocument();
    expect(screen.getByText('14')).toBeInTheDocument();
  });

  it('toggles edit mode for settings', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Edit')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Edit'));
    expect(screen.getByText('Edit Settings')).toBeInTheDocument();
    expect(screen.getByText('Save')).toBeInTheDocument();

    fireEvent.click(screen.getByText('Cancel'));
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('filters files by search input', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('document.pdf')).toBeInTheDocument();
      expect(screen.getByText('image.png')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByPlaceholderText('Search files...'), {
      target: { value: 'document' },
    });

    expect(screen.getByText('document.pdf')).toBeInTheDocument();
    expect(screen.queryByText('image.png')).not.toBeInTheDocument();
  });

  it('shows file action buttons', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('document.pdf')).toBeInTheDocument();
    });

    expect(screen.getAllByTitle('Download').length).toBeGreaterThan(0);
    expect(screen.getAllByTitle('Copy temp link').length).toBeGreaterThan(0);
    expect(screen.getAllByTitle('Restore from cold').length).toBeGreaterThan(0);
    expect(screen.getAllByTitle('Delete').length).toBeGreaterThan(0);
  });

  it('shows pagination controls', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Page 1')).toBeInTheDocument();
    });
    expect(screen.getByText('Previous')).toBeInTheDocument();
    expect(screen.getByText('Next')).toBeInTheDocument();
  });

  it('shows assigned storages section', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/projects/proj-1') return Promise.resolve({ data: mockProjectDetail });
      if (url.startsWith('/projects/proj-1/files')) return Promise.resolve({ data: [] });
      if (url === '/projects/proj-1/storages') {
        return Promise.resolve({
          data: [{
            id: 'ps-1', project_id: 'proj-1', storage_id: 's1',
            container_override: null, prefix_override: null, is_active: true,
            created_at: '', updated_at: '',
            storage_name: 'Local Disk', storage_type: 'local', is_hot: true, enabled: true,
          }],
        });
      }
      if (url === '/projects/proj-1/available-storages') return Promise.resolve({ data: [] });
      if (url === '/projects/proj-1/members') return Promise.resolve({ data: [] });
      if (url.startsWith('/auth/users/')) return Promise.resolve({ data: { user: null, projects: [], storages: [] } });
      return Promise.resolve({ data: [] });
    });

    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Local Disk')).toBeInTheDocument();
    });
  });

  it('shows members section with owner badge', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('projectowner')).toBeInTheDocument();
    });
    expect(screen.getByText('Owner')).toBeInTheDocument();
    expect(screen.getByText('memberuser')).toBeInTheDocument();
  });

  it('shows empty members state when no members', async () => {
    setupMocks(mockFiles, []);
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('No members assigned.')).toBeInTheDocument();
    });
  });

  it('owner row has no remove button', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('projectowner')).toBeInTheDocument();
    });

    const removeButtons = screen.getAllByTitle('Remove member');
    expect(removeButtons).toHaveLength(1);
  });

  it('clicking remove member calls DELETE', async () => {
    setupMocks();
    mockDelete.mockResolvedValue({ data: {} });
    vi.spyOn(window, 'confirm').mockReturnValue(true);
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('memberuser')).toBeInTheDocument();
    });

    const removeButton = screen.getByTitle('Remove member');
    fireEvent.click(removeButton);

    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith('/projects/proj-1/members/member-1');
    });
  });

  it('opens add member modal', async () => {
    setupMocks();
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Members')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Add Member'));

    await waitFor(() => {
      expect(screen.getByText('Select a user...')).toBeInTheDocument();
    });
  });

  it('adds a member via the modal', async () => {
    setupMocks();
    mockPost.mockResolvedValue({ data: {} });
    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Members')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Add Member'));

    await waitFor(() => {
      expect(screen.getByText('Select a user...')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByDisplayValue('Select a user...'), {
      target: { value: 'other-1' },
    });
    fireEvent.click(screen.getByText('Add'));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/projects/proj-1/members', { user_id: 'other-1' });
    });
  });
});
