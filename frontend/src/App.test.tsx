import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import App from './App';

function renderWithProviders(initialRoute = '/') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
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
  it('renders the sidebar with navigation links', () => {
    renderWithProviders();
    expect(screen.getByText('Innovare Storage')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Dashboard/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Projects/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Storages/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Sync Tasks/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Nodes/ })).toBeInTheDocument();
  });

  it('renders the Dashboard page at root route', () => {
    renderWithProviders('/');
    expect(
      screen.getByText('System overview and statistics.'),
    ).toBeInTheDocument();
  });

  it('navigates to Projects page', () => {
    renderWithProviders('/projects');
    expect(screen.getByText('Manage storage projects.')).toBeInTheDocument();
  });

  it('navigates to Storages page', () => {
    renderWithProviders('/storages');
    expect(screen.getByText('Manage storage backends.')).toBeInTheDocument();
  });

  it('navigates to Sync Tasks page', () => {
    renderWithProviders('/sync-tasks');
    expect(
      screen.getByText('Monitor file synchronization tasks.'),
    ).toBeInTheDocument();
  });

  it('navigates to Nodes page', () => {
    renderWithProviders('/nodes');
    expect(
      screen.getByText('Active service nodes in the cluster.'),
    ).toBeInTheDocument();
  });

  it('navigates to Project Detail page', () => {
    renderWithProviders('/projects/123e4567-e89b-12d3-a456-426614174000');
    expect(screen.getByText('Loading project...')).toBeInTheDocument();
  });

  it('navigates to Storage Detail page', () => {
    renderWithProviders('/storages/abc-def-123');
    expect(screen.getByText('Storage ID: abc-def-123')).toBeInTheDocument();
  });

  it('shows 404 for unknown routes', () => {
    renderWithProviders('/unknown-page');
    expect(screen.getByText('404 - Page Not Found')).toBeInTheDocument();
  });
});
