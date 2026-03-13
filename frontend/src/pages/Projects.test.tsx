import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import Projects from './Projects';

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

function renderProjects() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Projects />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockProjectsList = [
  {
    id: 'p1', name: 'Project Alpha', slug: 'alpha',
    hot_to_cold_days: 7, created_at: '2026-01-01', updated_at: '2026-01-01',
  },
  {
    id: 'p2', name: 'Project Beta', slug: 'beta',
    hot_to_cold_days: null, created_at: '2026-02-01', updated_at: '2026-02-01',
  },
];

describe('Projects', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders heading and subtitle', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderProjects();
    expect(screen.getByText('Projects')).toBeInTheDocument();
    expect(screen.getByText('Manage storage projects.')).toBeInTheDocument();
  });

  it('shows projects in a table with stats', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/projects') {
        return Promise.resolve({ data: mockProjectsList });
      }
      if (url === '/projects/p1') {
        return Promise.resolve({
          data: { project: mockProjectsList[0], stats: { file_count: 5, total_size: 10240 } },
        });
      }
      if (url === '/projects/p2') {
        return Promise.resolve({
          data: { project: mockProjectsList[1], stats: { file_count: 0, total_size: 0 } },
        });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderProjects();

    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });
    expect(screen.getByText('alpha')).toBeInTheDocument();
    expect(screen.getByText('7d')).toBeInTheDocument();
    expect(screen.getByText('Project Beta')).toBeInTheDocument();
    expect(screen.getByText('beta')).toBeInTheDocument();
  });

  it('shows empty state when no projects', async () => {
    mockGet.mockResolvedValue({ data: [] });
    renderProjects();

    await waitFor(() => {
      expect(screen.getByText('No projects yet.')).toBeInTheDocument();
    });
  });

  it('shows create project form when New Project button clicked', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderProjects();

    fireEvent.click(screen.getByText('New Project'));
    expect(screen.getByText('Create Project')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Project name')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('slug')).toBeInTheDocument();
  });

  it('submits create form with correct data', async () => {
    mockGet.mockResolvedValue({ data: [] });
    mockPost.mockResolvedValue({ data: { id: 'new-id', name: 'Test', slug: 'test' } });

    renderProjects();
    fireEvent.click(screen.getByText('New Project'));

    fireEvent.change(screen.getByPlaceholderText('Project name'), { target: { value: 'Test' } });
    fireEvent.change(screen.getByPlaceholderText('slug'), { target: { value: 'test' } });
    fireEvent.click(screen.getByText('Create'));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/projects', {
        name: 'Test',
        slug: 'test',
        hot_to_cold_days: null,
      });
    });
  });

  it('shows edit mode when Edit button clicked', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url === '/projects') {
        return Promise.resolve({ data: [mockProjectsList[0]] });
      }
      if (url === '/projects/p1') {
        return Promise.resolve({
          data: { project: mockProjectsList[0], stats: { file_count: 0, total_size: 0 } },
        });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderProjects();

    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTitle('Edit'));
    expect(screen.getByText('Save')).toBeInTheDocument();
    expect(screen.getByText('Cancel')).toBeInTheDocument();
  });
});
