import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MemoryRouter } from 'react-router-dom';
import Dashboard from './Dashboard';

const mockGetDashboard = vi.fn();

vi.mock('../api/client', () => ({
  default: {
    get: vi.fn(),
    post: vi.fn(),
    put: vi.fn(),
    delete: vi.fn(),
  },
  setAuthInterceptors: vi.fn(),
  getDashboard: (...args: unknown[]) => mockGetDashboard(...args),
}));

// recharts uses canvas APIs not available in jsdom; stub to bare DOM nodes
// so the assertions can still find chart titles/labels rendered around them.
vi.mock('recharts', () => ({
  AreaChart: ({ children }: { children: React.ReactNode }) => <div data-testid="area-chart">{children}</div>,
  Area: () => <div />,
  LineChart: ({ children }: { children: React.ReactNode }) => <div data-testid="line-chart">{children}</div>,
  Line: () => <div />,
  BarChart: ({ children }: { children: React.ReactNode }) => <div data-testid="bar-chart">{children}</div>,
  Bar: () => <div />,
  XAxis: () => <div />,
  YAxis: () => <div />,
  CartesianGrid: () => <div />,
  Tooltip: () => <div />,
  Legend: () => <div />,
  ResponsiveContainer: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

const mockUseAuth = vi.fn();
vi.mock('../contexts/AuthContext', () => ({
  useAuth: () => mockUseAuth(),
}));

import apiClient from '../api/client';
const mockApiGet = vi.mocked(apiClient.get);

function renderDashboard() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <Dashboard />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const emptyDashboard = {
  period: '30d',
  start: '2026-05-01T00:00:00Z',
  bucket: 'day',
  totals: {
    files: 0, bytes: 0,
    uploads_in_period: 0, bytes_uploaded_in_period: 0,
    accesses_in_period: 0, bytes_accessed_in_period: 0,
    pending_syncs: 0, failed_syncs_in_period: 0,
  },
  upload_timeline: [],
  access_timeline: [],
  by_content_type: [],
  by_storage: [],
  sync_status_trend: [],
  top_accessed_files: [],
};

function adminUser() {
  return {
    user: { id: '1', username: 'admin', role: 'admin', created_at: '', updated_at: '' },
    isLoading: false,
    login: vi.fn(),
    logout: vi.fn(),
  };
}

describe('Dashboard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseAuth.mockReturnValue(adminUser());
    mockApiGet.mockImplementation((url: string) => {
      if (url === '/projects') return Promise.resolve({ data: [] });
      if (url === '/storages') return Promise.resolve({ data: [] });
      if (url === '/system/nodes') return Promise.resolve({ data: [] });
      return Promise.reject(new Error(`Unmocked URL: ${url}`));
    });
    mockGetDashboard.mockResolvedValue({ data: emptyDashboard });
  });

  it('renders heading and subtitle', () => {
    renderDashboard();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
    expect(screen.getByText(/System metrics/)).toBeInTheDocument();
  });

  it('renders stat card labels for admin', () => {
    renderDashboard();
    expect(screen.getByText('Total Files')).toBeInTheDocument();
    expect(screen.getByText('Storage Used')).toBeInTheDocument();
    expect(screen.getByText('Pending Syncs')).toBeInTheDocument();
    expect(screen.getByText('Active Nodes')).toBeInTheDocument();
    expect(screen.getByText(/Uploads · /)).toBeInTheDocument();
    expect(screen.getByText(/Accesses · /)).toBeInTheDocument();
  });

  it('shows user dashboard for non-admin', () => {
    mockUseAuth.mockReturnValue({
      ...adminUser(),
      user: { id: '2', username: 'user1', role: 'user', created_at: '', updated_at: '' },
    });
    renderDashboard();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
    expect(screen.getByText(/Welcome to Innovare Storage/)).toBeInTheDocument();
    expect(screen.queryByText('Total Files')).not.toBeInTheDocument();
  });

  it('shows stat values after dashboard data loads', async () => {
    mockGetDashboard.mockResolvedValue({
      data: {
        ...emptyDashboard,
        totals: {
          ...emptyDashboard.totals,
          files: 42,
          bytes: 1_073_741_824,
          pending_syncs: 5,
        },
      },
    });

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('42')).toBeInTheDocument();
    });
    expect(screen.getByText('1 GB')).toBeInTheDocument();
    expect(screen.getByText('5')).toBeInTheDocument();
  });

  it('renders top accessed files when present', async () => {
    mockGetDashboard.mockResolvedValue({
      data: {
        ...emptyDashboard,
        top_accessed_files: [
          {
            file_id: 'f1', original_name: 'report.pdf', content_type: 'application/pdf',
            access_count: 7, last_accessed: '2026-05-26T10:00:00Z',
          },
        ],
        by_content_type: [
          { content_type: 'application/pdf', count: 1, size: 1024 },
        ],
      },
    });

    renderDashboard();

    await waitFor(() => {
      expect(screen.getByText('report.pdf')).toBeInTheDocument();
    });
    expect(screen.getAllByText('application/pdf').length).toBeGreaterThan(0);
  });

  it('exposes period preset buttons', () => {
    renderDashboard();
    expect(screen.getByTestId('period-7d')).toBeInTheDocument();
    expect(screen.getByTestId('period-30d')).toBeInTheDocument();
    expect(screen.getByTestId('period-90d')).toBeInTheDocument();
    expect(screen.getByTestId('period-1y')).toBeInTheDocument();
  });
});
