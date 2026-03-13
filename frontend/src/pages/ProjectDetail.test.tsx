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

import apiClient from '../api/client';
const mockGet = vi.mocked(apiClient.get);

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

function setupMocks(files = mockFiles) {
  mockGet.mockImplementation((url: string) => {
    if (url === '/projects/proj-1') return Promise.resolve({ data: mockProjectDetail });
    if (url.startsWith('/projects/proj-1/files')) return Promise.resolve({ data: files });
    if (url === '/projects/proj-1/storages') return Promise.resolve({ data: [] });
    if (url === '/projects/proj-1/available-storages') return Promise.resolve({ data: [] });
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
      return Promise.resolve({ data: [] });
    });

    renderProjectDetail('proj-1');

    await waitFor(() => {
      expect(screen.getByText('Local Disk')).toBeInTheDocument();
    });
  });
});
