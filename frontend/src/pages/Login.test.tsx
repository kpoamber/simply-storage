import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from '../App';

vi.mock('../api/client', () => ({
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

import apiClient from '../api/client';
const mockPost = vi.mocked(apiClient.post);
const mockGet = vi.mocked(apiClient.get);

function renderLogin() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  localStorage.removeItem('innovare_refresh_token');

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={['/login']}>
        <App />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('Login', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it('renders login form by default', async () => {
    renderLogin();
    await waitFor(() => {
      expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
    });
    expect(screen.getByLabelText('Username')).toBeInTheDocument();
    expect(screen.getByLabelText('Password')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign In' })).toBeInTheDocument();
  });

  it('switches to register form', async () => {
    renderLogin();
    await waitFor(() => {
      expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("Don't have an account? Register"));
    expect(screen.getByText('Create a new account')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Register' })).toBeInTheDocument();
  });

  it('switches back to login form', async () => {
    renderLogin();
    await waitFor(() => {
      expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("Don't have an account? Register"));
    fireEvent.click(screen.getByText('Already have an account? Sign in'));
    expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
  });

  it('submits login form', async () => {
    mockPost.mockImplementation((url: string) => {
      if (url === '/auth/login') {
        return Promise.resolve({
          data: { access_token: 'tok', refresh_token: 'ref' },
        });
      }
      return Promise.reject(new Error('Unknown'));
    });
    mockGet.mockImplementation((url: string) => {
      if (typeof url === 'string' && url.includes('/auth/me')) {
        return Promise.resolve({
          data: { id: '1', username: 'alice', role: 'admin', created_at: '', updated_at: '' },
        });
      }
      return Promise.reject(new Error('Unknown'));
    });

    renderLogin();
    await waitFor(() => {
      expect(screen.getByLabelText('Username')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'alice' } });
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'secret123' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/auth/login', {
        username: 'alice',
        password: 'secret123',
      });
    });
  });

  it('shows error on login failure', async () => {
    mockPost.mockRejectedValue({
      response: { data: { error: 'Invalid username or password' } },
    });

    renderLogin();
    await waitFor(() => {
      expect(screen.getByLabelText('Username')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'bob' } });
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'wrongpass' } });
    fireEvent.click(screen.getByRole('button', { name: 'Sign In' }));

    await waitFor(() => {
      expect(screen.getByText('Invalid username or password')).toBeInTheDocument();
    });
  });

  it('submits register form', async () => {
    mockPost.mockImplementation((url: string) => {
      if (url === '/auth/register') {
        return Promise.resolve({
          data: {
            user: { id: '1', username: 'newuser', role: 'admin', created_at: '', updated_at: '' },
            access_token: 'tok',
            refresh_token: 'ref',
          },
        });
      }
      return Promise.reject(new Error('Unknown'));
    });

    renderLogin();
    await waitFor(() => {
      expect(screen.getByText('Sign in to your account')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("Don't have an account? Register"));

    fireEvent.change(screen.getByLabelText('Username'), { target: { value: 'newuser' } });
    fireEvent.change(screen.getByLabelText('Password'), { target: { value: 'password123' } });
    fireEvent.click(screen.getByRole('button', { name: 'Register' }));

    await waitFor(() => {
      expect(mockPost).toHaveBeenCalledWith('/auth/register', {
        username: 'newuser',
        password: 'password123',
      });
    });
  });
});
