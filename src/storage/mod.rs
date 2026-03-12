pub mod azure;
pub mod ftp;
pub mod gcs;
pub mod hetzner;
pub mod local;
pub mod registry;
pub mod s3;
#[cfg(feature = "samba")]
pub mod samba;
pub mod sftp;
pub mod traits;

pub use azure::{AzureBlobBackend, AzureBlobConfig};
pub use ftp::{FtpBackend, FtpConfig};
pub use gcs::{GcsBackend, GcsConfig};
pub use hetzner::{HetznerStorageBoxBackend, HetznerStorageBoxConfig};
pub use local::LocalDiskBackend;
pub use registry::StorageRegistry;
pub use s3::{S3Config, S3StorageBackend};
#[cfg(feature = "samba")]
pub use samba::{SambaBackend, SambaConfig};
pub use sftp::{SftpBackend, SftpConfig};
pub use traits::StorageBackend;
