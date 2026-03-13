import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import StorageForm from './StorageForm';

describe('StorageForm', () => {
  const defaultProps = {
    onSubmit: vi.fn(),
    onCancel: vi.fn(),
    isLoading: false,
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders name field and storage type selector', () => {
    render(<StorageForm {...defaultProps} />);
    expect(screen.getByPlaceholderText('My Storage')).toBeInTheDocument();
    expect(screen.getByLabelText('Storage Type')).toBeInTheDocument();
  });

  it('shows local disk fields by default', () => {
    render(<StorageForm {...defaultProps} />);
    expect(screen.getByPlaceholderText('/data/storage')).toBeInTheDocument();
  });

  it('switches to S3 fields when storage type changed', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 's3' } });

    expect(screen.getByPlaceholderText('us-east-1')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('my-bucket')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('https://ams3.digitaloceanspaces.com')).toBeInTheDocument();
    expect(screen.queryByPlaceholderText('/data/storage')).not.toBeInTheDocument();
  });

  it('switches to Azure fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'azure' } });

    expect(screen.getByText('Account Name *')).toBeInTheDocument();
    expect(screen.getByText('Account Key *')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('my-container')).toBeInTheDocument();
  });

  it('switches to GCS fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'gcs' } });

    expect(screen.getByText('Client Email *')).toBeInTheDocument();
    expect(screen.getByText('Private Key (PEM) *')).toBeInTheDocument();
  });

  it('switches to FTP fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'ftp' } });

    expect(screen.getByPlaceholderText('ftp.example.com')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('21')).toBeInTheDocument();
  });

  it('switches to SFTP fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'sftp' } });

    expect(screen.getByPlaceholderText('sftp.example.com')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('22')).toBeInTheDocument();
  });

  it('switches to Samba fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'samba' } });

    expect(screen.getByPlaceholderText('192.168.1.100')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('files')).toBeInTheDocument();
  });

  it('switches to Hetzner fields', () => {
    render(<StorageForm {...defaultProps} />);
    fireEvent.change(screen.getByLabelText('Storage Type'), { target: { value: 'hetzner' } });

    expect(screen.getByPlaceholderText('uXXXXXX.your-storagebox.de')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('443')).toBeInTheDocument();
  });

  it('shows hot storage checkbox', () => {
    render(<StorageForm {...defaultProps} />);
    expect(screen.getByText('Hot storage (fast access tier)')).toBeInTheDocument();
  });

  it('shows Update button in edit mode', () => {
    render(<StorageForm {...defaultProps} isEdit initialValues={{
      name: 'Test', storage_type: 'local', config: { path: '/data' }, is_hot: true,
    }} />);
    expect(screen.getByText('Update')).toBeInTheDocument();
  });

  it('disables storage type selector in edit mode', () => {
    render(<StorageForm {...defaultProps} isEdit initialValues={{
      name: 'Test', storage_type: 's3', config: {}, is_hot: true,
    }} />);
    expect(screen.getByLabelText('Storage Type')).toBeDisabled();
  });

  it('calls onSubmit with form data', () => {
    const onSubmit = vi.fn();
    render(<StorageForm {...defaultProps} onSubmit={onSubmit} />);

    fireEvent.change(screen.getByPlaceholderText('My Storage'), { target: { value: 'Test Storage' } });
    fireEvent.change(screen.getByPlaceholderText('/data/storage'), { target: { value: '/mnt/test' } });
    fireEvent.click(screen.getByText('Create'));

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Test Storage',
      storage_type: 'local',
      config: { path: '/mnt/test' },
      is_hot: true,
    }));
  });

  it('calls onCancel when Cancel clicked', () => {
    const onCancel = vi.fn();
    render(<StorageForm {...defaultProps} onCancel={onCancel} />);

    fireEvent.click(screen.getByText('Cancel'));
    expect(onCancel).toHaveBeenCalled();
  });

  it('shows Saving... when isLoading is true', () => {
    render(<StorageForm {...defaultProps} isLoading={true} />);
    expect(screen.getByText('Saving...')).toBeInTheDocument();
  });
});
