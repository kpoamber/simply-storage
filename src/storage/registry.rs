use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::storage::traits::StorageBackend;

/// Thread-safe registry for looking up storage backends by their storage ID.
///
/// Supports dynamic registration and removal of backends at runtime,
/// allowing hot-reload of storage configurations.
pub struct StorageRegistry {
    backends: RwLock<HashMap<Uuid, Arc<dyn StorageBackend>>>,
}

impl StorageRegistry {
    pub fn new() -> Self {
        Self {
            backends: RwLock::new(HashMap::new()),
        }
    }

    /// Register a storage backend under the given ID.
    /// Replaces any existing backend with the same ID.
    pub async fn register(&self, id: Uuid, backend: Arc<dyn StorageBackend>) {
        let mut backends = self.backends.write().await;
        backends.insert(id, backend);
    }

    /// Remove a storage backend by ID.
    /// Returns the removed backend if it existed.
    pub async fn unregister(&self, id: &Uuid) -> Option<Arc<dyn StorageBackend>> {
        let mut backends = self.backends.write().await;
        backends.remove(id)
    }

    /// Look up a storage backend by ID.
    pub async fn get(&self, id: &Uuid) -> AppResult<Arc<dyn StorageBackend>> {
        let backends = self.backends.read().await;
        backends
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("Storage backend not found: {}", id)))
    }

    /// Check whether a backend with the given ID is registered.
    pub async fn contains(&self, id: &Uuid) -> bool {
        let backends = self.backends.read().await;
        backends.contains_key(id)
    }

    /// Return the number of registered backends.
    pub async fn len(&self) -> usize {
        let backends = self.backends.read().await;
        backends.len()
    }

    /// Return whether the registry is empty.
    pub async fn is_empty(&self) -> bool {
        let backends = self.backends.read().await;
        backends.is_empty()
    }

    /// Return all registered storage IDs.
    pub async fn list_ids(&self) -> Vec<Uuid> {
        let backends = self.backends.read().await;
        backends.keys().copied().collect()
    }
}

impl Default for StorageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::local::LocalDiskBackend;

    fn make_backend(path: &str) -> Arc<dyn StorageBackend> {
        Arc::new(LocalDiskBackend::new(path, "test-secret"))
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        let backend = make_backend("/tmp/test1");

        registry.register(id, backend).await;
        assert!(registry.get(&id).await.is_ok());
    }

    #[tokio::test]
    async fn test_get_nonexistent_returns_not_found() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();

        let result = registry.get(&id).await;
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        let backend = make_backend("/tmp/test2");

        registry.register(id, backend).await;
        assert!(registry.contains(&id).await);

        let removed = registry.unregister(&id).await;
        assert!(removed.is_some());
        assert!(!registry.contains(&id).await);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_returns_none() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();
        assert!(registry.unregister(&id).await.is_none());
    }

    #[tokio::test]
    async fn test_replace_existing() {
        let registry = StorageRegistry::new();
        let id = Uuid::new_v4();

        registry.register(id, make_backend("/tmp/old")).await;
        registry.register(id, make_backend("/tmp/new")).await;

        // Should still have exactly one entry
        assert_eq!(registry.len().await, 1);
        assert!(registry.get(&id).await.is_ok());
    }

    #[tokio::test]
    async fn test_len_and_is_empty() {
        let registry = StorageRegistry::new();
        assert!(registry.is_empty().await);
        assert_eq!(registry.len().await, 0);

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        registry.register(id1, make_backend("/tmp/a")).await;
        registry.register(id2, make_backend("/tmp/b")).await;

        assert!(!registry.is_empty().await);
        assert_eq!(registry.len().await, 2);
    }

    #[tokio::test]
    async fn test_list_ids() {
        let registry = StorageRegistry::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        registry.register(id1, make_backend("/tmp/a")).await;
        registry.register(id2, make_backend("/tmp/b")).await;

        let mut ids = registry.list_ids().await;
        ids.sort();
        let mut expected = vec![id1, id2];
        expected.sort();
        assert_eq!(ids, expected);
    }
}
