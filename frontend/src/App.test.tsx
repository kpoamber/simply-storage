import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from './App';

// Mock the API client
vi.mock('./api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
    interceptors: {
      request: { use: vi.fn() },
      response: { use: vi.fn() },
    },
    defaults: { baseURL: '/api', headers: { 'Content-Type': 'application/json' } },
  },
  setAuthInterceptors: vi.fn(),
}));

import apiClient from './api/client';
const mockGet = vi.mocked(apiClient.get);
const mockPost = vi.mocked(apiClient.post);

function renderWithProviders(initialRoute = '/') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  // Clear localStorage so AuthProvider doesn't try to restore session
  localStorage.removeItem('innovare_refresh_token');

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialRoute]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function renderAsLoggedIn(initialRoute = '/', role: 'admin' | 'user' = 'admin') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  // Set refresh token so AuthProvider tries to restore
  localStorage.setItem('innovare_refresh_token', 'test-refresh-token');

  // Mock refresh and me endpoints
  mockPost.mockImplementation((url: string) => {
    if (url === '/auth/refresh') {
      return Promise.resolve({
        data: { access_token: 'test-access-token', refresh_token: 'new-refresh-token' },
      });
    }
    return Promise.reject(new Error('Unknown POST URL'));
  });

  mockGet.mockImplementation((url: string) => {
    if (typeof url === 'string' && url.includes('/auth/me')) {
      return Promise.resolve({
        data: {
          id: 'user-1',
          username: 'testuser',
          role,
          created_at: '2026-01-01',
          updated_at: '2026-01-01',
        },
      });
    }
    return Promise.reject(new Error('Unknown GET URL'));
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[initialRoute]}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('App', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it('redirects to /login when not authenticated', async () => {
    renderWithProviders('/');
    await waitFor(() => {
      expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
    });
  });

  it('renders login page at /login route', async () => {
    renderWithProviders('/login');
    await waitFor(() => {
      expect(screen.getByText('Innovare Storage')).toBeInTheDocument();
    });
    expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
  });

  it('renders sidebar and content when authenticated', async () => {
    renderAsLoggedIn('/');
    await waitFor(() => {
      expect(screen.getByText('testuser')).toBeInTheDocument();
    });
    expect(screen.getByRole('link', { name: /Dashboard/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Projects/ })).toBeInTheDocument();
  });

  it('shows admin-only nav items for admin role', async () => {
    renderAsLoggedIn('/', 'admin');
    await waitFor(() => {
      expect(screen.getByText('testuser')).toBeInTheDocument();
    });
    expect(screen.getByRole('link', { name: /Storages/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Sync Tasks/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Nodes/ })).toBeInTheDocument();
  });

  it('hides admin-only nav items for user role', async () => {
    renderAsLoggedIn('/', 'user');
    await waitFor(() => {
      expect(screen.getByText('testuser')).toBeInTheDocument();
    });
    expect(screen.queryByRole('link', { name: /Storages/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Sync Tasks/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Nodes/ })).not.toBeInTheDocument();
  });

  it('shows 404 for unknown routes when authenticated', async () => {
    renderAsLoggedIn('/unknown-page');
    await waitFor(() => {
      expect(screen.getByText('404 - Page Not Found')).toBeInTheDocument();
    });
  });
});
