pub mod local;
pub mod registry;
pub mod traits;

pub use local::LocalDiskBackend;
pub use registry::StorageRegistry;
pub use traits::StorageBackend;
