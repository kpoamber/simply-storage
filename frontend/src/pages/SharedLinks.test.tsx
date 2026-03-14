import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import SharedLinks from './SharedLinks';

const mockGet = vi.fn();
const mockPost = vi.fn();
const mockDelete = vi.fn();

vi.mock('../api/client', () => ({
  default: {
    get: (...args: unknown[]) => mockGet(...args),
    post: (...args: unknown[]) => mockPost(...args),
    delete: (...args: unknown[]) => mockDelete(...args),
  },
}));

function renderSharedLinks(projectId = 'proj-1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/projects/${projectId}/shared-links`]}>
        <Routes>
          <Route path="/projects/:id/shared-links" element={<SharedLinks />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockLinks = [
  {
    id: 'link-1',
    token: 'abc123token',
    file_id: 'file-1',
    project_id: 'proj-1',
    original_name: 'report.pdf',
    created_by: 'user-1',
    password_protected: false,
    expires_at: null,
    max_downloads: null,
    download_count: 5,
    last_accessed_at: '2026-03-14T10:00:00Z',
    is_active: true,
    created_at: '2026-03-14T08:00:00Z',
  },
  {
    id: 'link-2',
    token: 'xyz789token',
    file_id: 'file-2',
    project_id: 'proj-1',
    original_name: 'secret.docx',
    created_by: 'user-1',
    password_protected: true,
    expires_at: '2026-03-15T08:00:00Z',
    max_downloads: 10,
    download_count: 3,
    last_accessed_at: null,
    is_active: true,
    created_at: '2026-03-14T09:00:00Z',
  },
];

describe('SharedLinks', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders page title and create button', async () => {
    mockGet.mockResolvedValue({ data: [] });
    renderSharedLinks();

    expect(screen.getByText('Shared Links')).toBeInTheDocument();
    expect(screen.getByTestId('create-link-button')).toBeInTheDocument();
  });

  it('shows empty state when no links', async () => {
    mockGet.mockResolvedValue({ data: [] });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    });
    expect(screen.getByText('No shared links yet.')).toBeInTheDocument();
  });

  it('renders links table with data', async () => {
    mockGet.mockResolvedValue({ data: mockLinks });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getByTestId('links-table')).toBeInTheDocument();
    });
    expect(screen.getAllByTestId('link-row')).toHaveLength(2);
    expect(screen.getByText('report.pdf')).toBeInTheDocument();
    expect(screen.getByText('secret.docx')).toBeInTheDocument();
  });

  it('shows public/protected type indicators', async () => {
    mockGet.mockResolvedValue({ data: mockLinks });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getByText('Public')).toBeInTheDocument();
    });
    expect(screen.getByText('Protected')).toBeInTheDocument();
  });

  it('shows download counts', async () => {
    mockGet.mockResolvedValue({ data: mockLinks });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getByText('5')).toBeInTheDocument();
    });
    expect(screen.getByText('3')).toBeInTheDocument();
    expect(screen.getByText('/ 10')).toBeInTheDocument();
  });

  it('toggles create form', () => {
    mockGet.mockResolvedValue({ data: [] });
    renderSharedLinks();

    expect(screen.queryByTestId('create-form')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('create-link-button'));
    expect(screen.getByTestId('create-form')).toBeInTheDocument();
  });

  it('shows error when creating without selecting file', () => {
    mockGet.mockResolvedValue({ data: [] });
    renderSharedLinks();

    fireEvent.click(screen.getByTestId('create-link-button'));
    fireEvent.click(screen.getByTestId('submit-create'));

    expect(screen.getByTestId('error-message')).toHaveTextContent('Please select a file');
  });

  it('has back to project link', () => {
    mockGet.mockResolvedValue({ data: [] });
    renderSharedLinks();

    const link = screen.getByText(/Back to Project/);
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/projects/proj-1');
  });

  it('calls deactivate API when deactivate button clicked', async () => {
    mockGet.mockResolvedValue({ data: mockLinks });
    mockDelete.mockResolvedValue({ data: { ...mockLinks[0], is_active: false } });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getByTestId(`deactivate-${mockLinks[0].id}`)).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId(`deactivate-${mockLinks[0].id}`));

    await waitFor(() => {
      expect(mockDelete).toHaveBeenCalledWith(`/shared-links/${mockLinks[0].id}`);
    });
  });

  it('shows Active/Inactive status badges', async () => {
    const linksWithInactive = [
      ...mockLinks,
      { ...mockLinks[0], id: 'link-3', token: 'inactive-token', is_active: false },
    ];
    mockGet.mockResolvedValue({ data: linksWithInactive });
    renderSharedLinks();

    await waitFor(() => {
      expect(screen.getAllByText('Active')).toHaveLength(2);
    });
    expect(screen.getByText('Inactive')).toBeInTheDocument();
  });
});
