import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import SharedLinkAccess from './SharedLinkAccess';

const mockAxiosGet = vi.fn();
const mockAxiosPost = vi.fn();

vi.mock('axios', () => ({
  default: {
    get: (...args: unknown[]) => mockAxiosGet(...args),
    post: (...args: unknown[]) => mockAxiosPost(...args),
  },
}));

function renderAccess(token = 'test-token') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/share/${token}`]}>
        <Routes>
          <Route path="/share/:token" element={<SharedLinkAccess />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('SharedLinkAccess', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('shows loading state initially', () => {
    mockAxiosGet.mockReturnValue(new Promise(() => {})); // never resolves
    renderAccess();

    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('shows unavailable message when link not found', async () => {
    mockAxiosGet.mockRejectedValue({ response: { status: 404 } });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('link-unavailable')).toBeInTheDocument();
    });
    expect(screen.getByText('Link Unavailable')).toBeInTheDocument();
  });

  it('shows file info for public link', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'report.pdf',
        file_size: 1048576,
        content_type: 'application/pdf',
        password_required: false,
        expires_at: null,
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('link-access-card')).toBeInTheDocument();
    });
    expect(screen.getByTestId('file-name')).toHaveTextContent('report.pdf');
    expect(screen.getByTestId('file-size')).toHaveTextContent('1 MB');
    expect(screen.getByTestId('content-type')).toHaveTextContent('application/pdf');
    expect(screen.getByTestId('download-button')).toBeInTheDocument();
    expect(screen.queryByTestId('password-form')).not.toBeInTheDocument();
  });

  it('shows password form for protected link', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'secret.docx',
        file_size: 2048,
        content_type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
        password_required: true,
        expires_at: null,
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('password-form')).toBeInTheDocument();
    });
    expect(screen.getByTestId('password-field')).toBeInTheDocument();
    expect(screen.getByText('This file is password-protected')).toBeInTheDocument();
  });

  it('shows expired message for expired link (client-side check)', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'old.txt',
        file_size: 100,
        content_type: 'text/plain',
        password_required: false,
        expires_at: '2020-01-01T00:00:00Z', // expired in the past
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('link-expired')).toBeInTheDocument();
    });
    expect(screen.getByText('Link Expired')).toBeInTheDocument();
  });

  it('initiates download for public link', async () => {
    mockAxiosGet
      .mockResolvedValueOnce({
        data: {
          file_name: 'file.txt',
          file_size: 100,
          content_type: 'text/plain',
          password_required: false,
          expires_at: null,
        },
      });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('download-button')).toBeInTheDocument();
    });

    // Mock the download response
    const blob = new Blob(['content'], { type: 'text/plain' });
    mockAxiosGet.mockResolvedValueOnce({ data: blob });

    // Mock URL.createObjectURL and URL.revokeObjectURL
    const createObjectURL = vi.fn().mockReturnValue('blob:test');
    const revokeObjectURL = vi.fn();
    Object.defineProperty(window, 'URL', {
      value: { createObjectURL, revokeObjectURL },
      writable: true,
    });

    fireEvent.click(screen.getByTestId('download-button'));

    await waitFor(() => {
      expect(mockAxiosGet).toHaveBeenCalledWith('/s/test-token/download', { responseType: 'blob' });
    });
  });

  it('shows wrong password error on 403', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'secret.pdf',
        file_size: 500,
        content_type: 'application/pdf',
        password_required: true,
        expires_at: null,
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('password-form')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('password-field'), { target: { value: 'wrong' } });
    mockAxiosPost.mockRejectedValue({ response: { status: 403 } });

    fireEvent.submit(screen.getByTestId('password-form'));

    await waitFor(() => {
      expect(screen.getByTestId('password-error')).toHaveTextContent('Wrong password');
    });
  });

  it('verifies password and downloads for protected link', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'secret.pdf',
        file_size: 500,
        content_type: 'application/pdf',
        password_required: true,
        expires_at: null,
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('password-form')).toBeInTheDocument();
    });

    fireEvent.change(screen.getByTestId('password-field'), { target: { value: 'correct' } });

    mockAxiosPost.mockResolvedValue({ data: { dl_token: 'jwt-download-token' } });
    const blob = new Blob(['content'], { type: 'application/pdf' });
    mockAxiosGet.mockResolvedValueOnce({ data: blob });

    const createObjectURL = vi.fn().mockReturnValue('blob:test');
    const revokeObjectURL = vi.fn();
    Object.defineProperty(window, 'URL', {
      value: { createObjectURL, revokeObjectURL },
      writable: true,
    });

    fireEvent.submit(screen.getByTestId('password-form'));

    await waitFor(() => {
      expect(mockAxiosPost).toHaveBeenCalledWith('/s/test-token/verify', { password: 'correct' });
    });
  });

  it('shows file size and content type', async () => {
    mockAxiosGet.mockResolvedValue({
      data: {
        file_name: 'image.png',
        file_size: 5242880,
        content_type: 'image/png',
        password_required: false,
        expires_at: null,
      },
    });
    renderAccess();

    await waitFor(() => {
      expect(screen.getByTestId('file-size')).toHaveTextContent('5 MB');
    });
    expect(screen.getByTestId('content-type')).toHaveTextContent('image/png');
  });
});
