import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import ProjectSearch from './ProjectSearch';

const mockSearchFiles = vi.fn();
const mockSearchSummary = vi.fn();

vi.mock('../api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
  searchFiles: (...args: unknown[]) => mockSearchFiles(...args),
  searchSummary: (...args: unknown[]) => mockSearchSummary(...args),
}));

vi.mock('../contexts/AuthContext', () => ({
  useAuth: () => ({
    user: {
      id: 'user-1',
      username: 'admin',
      role: 'admin',
      created_at: '2026-01-01',
      updated_at: '2026-01-01',
    },
    isLoading: false,
  }),
}));

// Mock recharts to avoid rendering issues in jsdom
vi.mock('recharts', () => ({
  LineChart: ({ children }: { children: React.ReactNode }) => <div data-testid="line-chart">{children}</div>,
  Line: () => <div />,
  AreaChart: ({ children }: { children: React.ReactNode }) => <div data-testid="area-chart">{children}</div>,
  Area: () => <div />,
  XAxis: () => <div />,
  YAxis: () => <div />,
  CartesianGrid: () => <div />,
  Tooltip: () => <div />,
  ResponsiveContainer: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

function renderSearch(projectId = 'proj-1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[`/projects/${projectId}/search`]}>
        <Routes>
          <Route path="/projects/:id/search" element={<ProjectSearch />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const mockSearchResult = {
  results: [
    {
      id: 'fr-1', file_id: 'f-1', project_id: 'proj-1',
      original_name: 'document.pdf', created_at: '2026-01-15T00:00:00Z',
      metadata: { env: 'prod', version: '1.0' },
      sync_status: 'synced', synced_storages: 2, total_storages: 2,
    },
    {
      id: 'fr-2', file_id: 'f-2', project_id: 'proj-1',
      original_name: 'image.png', created_at: '2026-02-10T00:00:00Z',
      metadata: {},
      sync_status: 'pending', synced_storages: 0, total_storages: 1,
    },
  ],
  total: 2,
  page: 1,
  per_page: 50,
};

const mockSummaryResult = {
  total_files: 2,
  total_size: 10240,
  earliest_upload: '2026-01-15T00:00:00Z',
  latest_upload: '2026-02-10T00:00:00Z',
  timeline: [
    { date: '2026-01-15', count: 1, size: 5120 },
    { date: '2026-02-10', count: 1, size: 5120 },
  ],
};

describe('ProjectSearch', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders query builder with initial filter row', () => {
    renderSearch();
    expect(screen.getByText('Search Files')).toBeInTheDocument();
    expect(screen.getByText('Filters')).toBeInTheDocument();
    expect(screen.getAllByTestId('filter-row')).toHaveLength(1);
    expect(screen.getByTestId('search-button')).toBeInTheDocument();
  });

  it('adds and removes filter rows', () => {
    renderSearch();

    // Initially 1 row
    expect(screen.getAllByTestId('filter-row')).toHaveLength(1);

    // Add a filter
    fireEvent.click(screen.getByTestId('add-filter'));
    expect(screen.getAllByTestId('filter-row')).toHaveLength(2);

    // Add another
    fireEvent.click(screen.getByTestId('add-filter'));
    expect(screen.getAllByTestId('filter-row')).toHaveLength(3);

    // Remove one
    fireEvent.click(screen.getAllByTestId('remove-filter')[0]);
    expect(screen.getAllByTestId('filter-row')).toHaveLength(2);
  });

  it('allows changing filter mode (AND/OR/NOT)', () => {
    renderSearch();

    const modeSelect = screen.getByTestId('filter-mode');
    expect(modeSelect).toHaveValue('and');

    fireEvent.change(modeSelect, { target: { value: 'not' } });
    expect(modeSelect).toHaveValue('not');
  });

  it('triggers search API call on search button click', async () => {
    mockSearchFiles.mockResolvedValue({ data: mockSearchResult });
    mockSearchSummary.mockResolvedValue({ data: mockSummaryResult });

    renderSearch();

    // Fill in a filter
    fireEvent.change(screen.getByTestId('filter-key'), { target: { value: 'env' } });
    fireEvent.change(screen.getByTestId('filter-value'), { target: { value: 'prod' } });

    // Click search
    fireEvent.click(screen.getByTestId('search-button'));

    await waitFor(() => {
      expect(mockSearchFiles).toHaveBeenCalledWith(
        'proj-1',
        expect.objectContaining({
          filters: { key: 'env', value: 'prod' },
          page: 1,
          per_page: 50,
        }),
      );
    });
  });

  it('renders search results table', async () => {
    mockSearchFiles.mockResolvedValue({ data: mockSearchResult });
    mockSearchSummary.mockResolvedValue({ data: mockSummaryResult });

    renderSearch();

    fireEvent.change(screen.getByTestId('filter-key'), { target: { value: 'env' } });
    fireEvent.change(screen.getByTestId('filter-value'), { target: { value: 'prod' } });
    fireEvent.click(screen.getByTestId('search-button'));

    await waitFor(() => {
      expect(screen.getByText('document.pdf')).toBeInTheDocument();
    });
    expect(screen.getByText('image.png')).toBeInTheDocument();
    expect(screen.getByText('Results (2 files)')).toBeInTheDocument();
  });

  it('renders summary section with totals', async () => {
    mockSearchFiles.mockResolvedValue({ data: mockSearchResult });
    mockSearchSummary.mockResolvedValue({ data: mockSummaryResult });

    renderSearch();
    fireEvent.click(screen.getByTestId('search-button'));

    await waitFor(() => {
      expect(screen.getByTestId('search-summary')).toBeInTheDocument();
    });
    expect(screen.getByText('2')).toBeInTheDocument(); // total_files
    expect(screen.getByText('10 KB')).toBeInTheDocument(); // total_size
  });

  it('renders charts with timeline data', async () => {
    mockSearchFiles.mockResolvedValue({ data: mockSearchResult });
    mockSearchSummary.mockResolvedValue({ data: mockSummaryResult });

    renderSearch();
    fireEvent.click(screen.getByTestId('search-button'));

    await waitFor(() => {
      expect(screen.getByTestId('count-chart')).toBeInTheDocument();
    });
    expect(screen.getByTestId('size-chart')).toBeInTheDocument();
    expect(screen.getByTestId('line-chart')).toBeInTheDocument();
    expect(screen.getByTestId('area-chart')).toBeInTheDocument();
  });

  it('displays metadata tags in results', async () => {
    mockSearchFiles.mockResolvedValue({ data: mockSearchResult });
    mockSearchSummary.mockResolvedValue({ data: mockSummaryResult });

    renderSearch();
    fireEvent.click(screen.getByTestId('search-button'));

    await waitFor(() => {
      expect(screen.getByText('env=prod')).toBeInTheDocument();
    });
    expect(screen.getByText('version=1.0')).toBeInTheDocument();
  });
});
