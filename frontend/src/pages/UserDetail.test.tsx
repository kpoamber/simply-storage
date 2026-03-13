import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import UserDetail from './UserDetail';

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
const mockPut = vi.mocked(apiClient.put);
const mockDelete = vi.mocked(apiClient.delete);

function renderUserDetail(userId = 'u2') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/users/${userId}`]}>
        <Routes>
          <Route path="/users/:id" element={<UserDetail />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockUserDetail = {
  user: {
    id: 'u2',
    username: 'johndoe',
    role: 'user',
    created_at: '2026-02-15T00:00:00Z',
    updated_at: '2026-02-15T00:00:00Z',
  },
  projects: [
    {
      id: 'p1',
      name: 'Project Alpha',
      slug: 'alpha',
      hot_to_cold_days: null,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    },
  ],
  storages: [
    {
      id: 's1',
      name: 'Local Storage',
      storage_type: 'local',
      config: {},
      is_hot: true,
      project_id: null,
      enabled: true,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      file_count: 10,
      used_space: 1024,
    },
  ],
};

describe('UserDetail', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows loading state', () => {
    mockGet.mockReturnValue(new Promise(() => {}));
    renderUserDetail();
    expect(screen.getByText('Loading user...')).toBeInTheDocument();
  });

  it('shows user not found when data is null', async () => {
    mockGet.mockRejectedValue(new Error('Not found'));
    renderUserDetail();
    await waitFor(() => {
      expect(screen.getByText('User not found.')).toBeInTheDocument();
    });
  });

  it('renders user info header', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });
    // Role badge and select option both say "user", so use getAllByText
    const userTexts = screen.getAllByText('user');
    expect(userTexts.length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText(/Back to Users/)).toBeInTheDocument();
  });

  it('renders assigned projects', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });
    expect(screen.getByText('Assigned Projects (1)')).toBeInTheDocument();
    expect(screen.getByText('alpha')).toBeInTheDocument();
  });

  it('renders assigned storages', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Local Storage')).toBeInTheDocument();
    });
    expect(screen.getByText('Assigned Storages (1)')).toBeInTheDocument();
    expect(screen.getByText('local')).toBeInTheDocument();
    expect(screen.getByText('hot')).toBeInTheDocument();
  });

  it('shows empty state when no projects assigned', async () => {
    mockGet.mockResolvedValue({
      data: { ...mockUserDetail, projects: [] },
    });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('No projects assigned.')).toBeInTheDocument();
    });
  });

  it('shows empty state when no storages assigned', async () => {
    mockGet.mockResolvedValue({
      data: { ...mockUserDetail, storages: [] },
    });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('No storages assigned.')).toBeInTheDocument();
    });
  });

  it('shows edit user section with role dropdown', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Edit User')).toBeInTheDocument();
    });
    expect(screen.getByText('Update Role')).toBeInTheDocument();
    expect(screen.getByText('Reset Password')).toBeInTheDocument();
  });

  it('calls PUT when updating role', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    mockPut.mockResolvedValue({ data: { ...mockUserDetail.user, role: 'admin' } });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Edit User')).toBeInTheDocument();
    });

    const roleSelect = screen.getByDisplayValue('user');
    fireEvent.change(roleSelect, { target: { value: 'admin' } });
    fireEvent.click(screen.getByText('Update Role'));

    await waitFor(() => {
      expect(mockPut).toHaveBeenCalledWith('/auth/users/u2', { role: 'admin' });
    });
  });

  it('shows password form when Reset Password clicked', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Reset Password')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Reset Password'));
    expect(screen.getByPlaceholderText('Min 6 characters')).toBeInTheDocument();
    expect(screen.getByText('Confirm Reset')).toBeInTheDocument();
  });

  it('calls PUT when resetting password', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    mockPut.mockResolvedValue({ data: mockUserDetail.user });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Reset Password')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Reset Password'));
    fireEvent.change(screen.getByPlaceholderText('Min 6 characters'), {
      target: { value: 'newpass123' },
    });
    fireEvent.click(screen.getByText('Confirm Reset'));

    await waitFor(() => {
      expect(mockPut).toHaveBeenCalledWith('/auth/users/u2', {
        password: 'newpass123',
      });
    });
  });

  it('removes project assignment', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    mockDelete.mockResolvedValue({ data: {} });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    const removeButtons = screen.getAllByTitle('Remove assignment');
    fireEvent.click(removeButtons[0]);

    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith('/projects/p1/members/u2');
    });
  });

  it('removes storage assignment', async () => {
    mockGet.mockResolvedValue({ data: mockUserDetail });
    mockDelete.mockResolvedValue({ data: {} });
    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Local Storage')).toBeInTheDocument();
    });

    const removeButtons = screen.getAllByTitle('Remove assignment');
    // Second remove button is for storages (first is for projects)
    fireEvent.click(removeButtons[1]);

    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith('/storages/s1/members/u2');
    });
  });

  it('opens add project modal', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/auth/users/')) {
        return Promise.resolve({ data: mockUserDetail });
      }
      if (url === '/projects') {
        return Promise.resolve({
          data: [
            { project: mockUserDetail.projects[0] },
            {
              project: {
                id: 'p2',
                name: 'Project Beta',
                slug: 'beta',
                hot_to_cold_days: null,
                created_at: '2026-01-01T00:00:00Z',
                updated_at: '2026-01-01T00:00:00Z',
              },
            },
          ],
        });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Project Alpha')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Add Project'));

    await waitFor(() => {
      expect(
        screen.getByText('Add Project Assignment'),
      ).toBeInTheDocument();
    });
  });

  it('opens add storage modal', async () => {
    mockGet.mockImplementation((url: string) => {
      if (url.includes('/auth/users/')) {
        return Promise.resolve({ data: mockUserDetail });
      }
      if (url === '/storages') {
        return Promise.resolve({
          data: [
            mockUserDetail.storages[0],
            {
              id: 's2',
              name: 'S3 Storage',
              storage_type: 's3',
              config: {},
              is_hot: false,
              project_id: null,
              enabled: true,
              created_at: '2026-01-01T00:00:00Z',
              updated_at: '2026-01-01T00:00:00Z',
              file_count: 0,
              used_space: 0,
            },
          ],
        });
      }
      return Promise.reject(new Error('Unknown URL'));
    });

    renderUserDetail();

    await waitFor(() => {
      expect(screen.getByText('Local Storage')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText('Add Storage'));

    await waitFor(() => {
      expect(
        screen.getByText('Add Storage Assignment'),
      ).toBeInTheDocument();
    });
  });

  it('disables role change for self', async () => {
    const selfUserDetail = {
      ...mockUserDetail,
      user: {
        ...mockUserDetail.user,
        id: 'current-admin',
        username: 'admin',
        role: 'admin',
      },
    };
    mockGet.mockResolvedValue({ data: selfUserDetail });
    renderUserDetail('current-admin');

    await waitFor(() => {
      expect(screen.getByText('Edit User')).toBeInTheDocument();
    });

    const roleSelect = screen.getByDisplayValue('admin');
    expect(roleSelect).toBeDisabled();
  });
});
