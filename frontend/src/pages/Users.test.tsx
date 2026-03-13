import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import Users from './Users';

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

function renderUsers() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Users />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockUsersList = [
  {
    id: 'current-admin',
    username: 'admin',
    role: 'admin',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
  },
  {
    id: 'u2',
    username: 'johndoe',
    role: 'user',
    created_at: '2026-02-15T00:00:00Z',
    updated_at: '2026-02-15T00:00:00Z',
  },
];

describe('Users', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders heading and subtitle', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderUsers();
    expect(screen.getByText('Users')).toBeInTheDocument();
    expect(screen.getByText('Manage user accounts.')).toBeInTheDocument();
  });

  it('shows users in a table', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });
    // admin appears as both username link and role badge
    const adminElements = screen.getAllByText('admin');
    expect(adminElements.length).toBeGreaterThanOrEqual(2);
  });

  it('shows role badges', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });
    // Role badges rendered as <span> elements
    const adminBadges = screen.getAllByText('admin');
    expect(adminBadges.length).toBeGreaterThanOrEqual(2); // username link + role badge
    expect(screen.getByText('user')).toBeInTheDocument();
  });

  it('shows empty state when no users', async () => {
    mockGet.mockResolvedValue({ data: [] });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('No users found.')).toBeInTheDocument();
    });
  });

  it('shows create user form when New User button clicked', () => {
    mockGet.mockRejectedValue(new Error('Network error'));
    renderUsers();

    fireEvent.click(screen.getByText('New User'));
    expect(screen.getByText('Create User')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Username')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Password')).toBeInTheDocument();
  });

  it('submits create form with correct data', async () => {
    mockGet.mockResolvedValue({ data: [] });
    mockPost.mockResolvedValue({
      data: { id: 'new-id', username: 'newuser', role: 'user' },
    });

    renderUsers();
    fireEvent.click(screen.getByText('New User'));

    fireEvent.change(screen.getByPlaceholderText('Username'), {
      target: { value: 'newuser' },
    });
    fireEvent.change(screen.getByPlaceholderText('Password'), {
      target: { value: 'password123' },
    });
    fireEvent.click(screen.getByText('Create'));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/auth/users', {
        username: 'newuser',
        password: 'password123',
        role: 'user',
      });
    });
  });

  it('shows delete confirmation dialog', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });

    // Find the delete button for johndoe (not disabled one for current user)
    const deleteButtons = screen.getAllByTitle('Delete user');
    fireEvent.click(deleteButtons[0]);

    expect(screen.getByText('Delete?')).toBeInTheDocument();
    expect(screen.getByText('Yes')).toBeInTheDocument();
    expect(screen.getByText('No')).toBeInTheDocument();
  });

  it('confirms deletion and calls API', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    mockDelete.mockResolvedValue({ data: {} });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByTitle('Delete user');
    fireEvent.click(deleteButtons[0]);
    fireEvent.click(screen.getByText('Yes'));

    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith('/auth/users/u2');
    });
  });

  it('cancels deletion', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });

    const deleteButtons = screen.getAllByTitle('Delete user');
    fireEvent.click(deleteButtons[0]);
    fireEvent.click(screen.getByText('No'));

    expect(screen.queryByText('Delete?')).not.toBeInTheDocument();
  });

  it('disables delete button for current user', async () => {
    mockGet.mockResolvedValue({ data: mockUsersList });
    renderUsers();

    await waitFor(() => {
      expect(screen.getByText('johndoe')).toBeInTheDocument();
    });

    const selfDeleteButton = screen.getByTitle('Cannot delete yourself');
    expect(selfDeleteButton).toBeDisabled();
  });
});
