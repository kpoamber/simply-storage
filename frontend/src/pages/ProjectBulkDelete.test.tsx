import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import ProjectBulkDelete from './ProjectBulkDelete';

const mockBulkDeletePreview = vi.fn();
const mockBulkDeleteExecute = vi.fn();

vi.mock('../api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
  },
  bulkDeletePreview: (...args: unknown[]) => mockBulkDeletePreview(...args),
  bulkDeleteExecute: (...args: unknown[]) => mockBulkDeleteExecute(...args),
}));

function renderBulkDelete(projectId = 'proj-1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/projects/${projectId}/bulk-delete`]}>
        <Routes>
          <Route path="/projects/:id/bulk-delete" element={<ProjectBulkDelete />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

describe('ProjectBulkDelete', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders filter form with all fields', () => {
    renderBulkDelete();

    expect(screen.getByText('Bulk Delete Files')).toBeInTheDocument();
    expect(screen.getByText('Filters')).toBeInTheDocument();
    expect(screen.getByTestId('created-after')).toBeInTheDocument();
    expect(screen.getByTestId('created-before')).toBeInTheDocument();
    expect(screen.getByTestId('last-accessed-before')).toBeInTheDocument();
    expect(screen.getByTestId('size-min')).toBeInTheDocument();
    expect(screen.getByTestId('size-max')).toBeInTheDocument();
    expect(screen.getByTestId('preview-button')).toBeInTheDocument();
    expect(screen.getByTestId('delete-button')).toBeInTheDocument();
  });

  it('adds and removes metadata filter rows', () => {
    renderBulkDelete();

    // No metadata rows initially
    expect(screen.queryByTestId('metadata-filter-row')).not.toBeInTheDocument();

    // Add a row
    fireEvent.click(screen.getByTestId('add-metadata-filter'));
    expect(screen.getAllByTestId('metadata-filter-row')).toHaveLength(1);

    // Add another row
    fireEvent.click(screen.getByTestId('add-metadata-filter'));
    expect(screen.getAllByTestId('metadata-filter-row')).toHaveLength(2);

    // Remove first row
    fireEvent.click(screen.getAllByTestId('remove-metadata-filter')[0]);
    expect(screen.getAllByTestId('metadata-filter-row')).toHaveLength(1);
  });

  it('shows error when preview clicked without filters', () => {
    renderBulkDelete();

    fireEvent.click(screen.getByTestId('preview-button'));

    expect(screen.getByTestId('error-message')).toHaveTextContent('At least one filter is required');
  });

  it('preview calls API with size filter', async () => {
    mockBulkDeletePreview.mockResolvedValue({
      data: { matching_references: 5, total_size: 10240 },
    });
    renderBulkDelete();

    fireEvent.change(screen.getByTestId('size-min'), { target: { value: '1024' } });
    fireEvent.click(screen.getByTestId('preview-button'));

    await waitFor(() => {
      expect(mockBulkDeletePreview).toHaveBeenCalledWith('proj-1', expect.objectContaining({
        size_min: 1024,
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId('preview-result')).toBeInTheDocument();
    });
    expect(screen.getByText('5')).toBeInTheDocument();
    expect(screen.getByText('10 KB')).toBeInTheDocument();
  });

  it('shows confirmation dialog when delete clicked', () => {
    renderBulkDelete();

    fireEvent.change(screen.getByTestId('size-min'), { target: { value: '100' } });
    fireEvent.click(screen.getByTestId('delete-button'));

    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();
    expect(screen.getByText('Confirm Deletion')).toBeInTheDocument();
    expect(screen.getByTestId('confirm-delete')).toBeInTheDocument();
    expect(screen.getByTestId('cancel-delete')).toBeInTheDocument();
  });

  it('cancel button closes confirmation dialog', () => {
    renderBulkDelete();

    fireEvent.change(screen.getByTestId('size-min'), { target: { value: '100' } });
    fireEvent.click(screen.getByTestId('delete-button'));

    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('cancel-delete'));

    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });

  it('confirming delete calls API and shows result', async () => {
    mockBulkDeleteExecute.mockResolvedValue({
      data: { deleted_references: 3, orphaned_files_cleaned: 1, freed_bytes: 5120 },
    });
    renderBulkDelete();

    fireEvent.change(screen.getByTestId('size-max'), { target: { value: '9999' } });
    fireEvent.click(screen.getByTestId('delete-button'));

    expect(screen.getByTestId('confirm-dialog')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('confirm-delete'));

    await waitFor(() => {
      expect(mockBulkDeleteExecute).toHaveBeenCalledWith('proj-1', expect.objectContaining({
        size_max: 9999,
      }));
    });

    await waitFor(() => {
      expect(screen.getByTestId('delete-result')).toBeInTheDocument();
    });
    expect(screen.getByText('3')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
    expect(screen.getByText('5 KB')).toBeInTheDocument();
  });

  it('shows error when delete without filters', () => {
    renderBulkDelete();

    fireEvent.click(screen.getByTestId('delete-button'));

    expect(screen.getByTestId('error-message')).toHaveTextContent('At least one filter is required');
    expect(screen.queryByTestId('confirm-dialog')).not.toBeInTheDocument();
  });

  it('has back to project link', () => {
    renderBulkDelete();

    const link = screen.getByText(/Back to Project/);
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/projects/proj-1');
  });
});
