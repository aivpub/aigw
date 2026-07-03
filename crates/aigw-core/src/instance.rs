//! Instance registry for multi-instance deployment coordination.
//!
//! Provides `InstanceRegistry` — a concurrent-safe registry that tracks
//! running aigw instances, their health status, and lifecycle transitions.
//! Uses `RwLock<HashMap>` for read-heavy access patterns typical of
//! heartbeat checks and health-based load balancing.

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use tokio::sync::RwLock;
use uuid::Uuid;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// InstanceInfo
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Registration info for a single aigw instance.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    pub instance_id: String,
    pub bind_address: String,
    pub status: InstanceStatus,
    pub started_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// InstanceStatus
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceStatus {
    /// Instance has just registered, still initializing.
    Starting,
    /// Instance is healthy and accepting traffic.
    Healthy,
    /// Instance has missed heartbeats; should not receive traffic.
    Unhealthy,
    /// Instance is preparing to shut down (no new requests, drain existing).
    Draining,
    /// Instance has been removed from the registry.
    Stopped,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// InstanceRegistry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Concurrent-safe registry for multi-instance coordination.
///
/// Uses `RwLock<HashMap>` to allow many concurrent readers (heartbeats,
/// health checks, load balancer queries) with infrequent writers (register,
/// drain, unregister).
pub struct InstanceRegistry {
    instances: RwLock<HashMap<String, InstanceInfo>>,
}

impl InstanceRegistry {
    /// Create a new empty instance registry.
    pub fn new() -> Self {
        Self {
            instances: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new instance and return its unique instance ID.
    ///
    /// The instance starts in `Starting` status and transitions to
    /// `Healthy` on its first successful heartbeat.
    pub async fn register(&self, bind_address: &str) -> String {
        let instance_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let mut instances = self.instances.write().await;
        instances.insert(
            instance_id.clone(),
            InstanceInfo {
                instance_id: instance_id.clone(),
                bind_address: bind_address.to_string(),
                status: InstanceStatus::Starting,
                started_at: now,
                last_heartbeat: now,
            },
        );
        instance_id
    }

    /// Update the heartbeat timestamp for an instance.
    ///
    /// If the instance is in `Starting` status, this transitions it to
    /// `Healthy`. Subsequent heartbeats keep it in `Healthy`.
    pub async fn heartbeat(&self, instance_id: &str) {
        let mut instances = self.instances.write().await;
        if let Some(info) = instances.get_mut(instance_id) {
            info.last_heartbeat = Utc::now();
            if info.status == InstanceStatus::Starting {
                info.status = InstanceStatus::Healthy;
            }
        }
    }

    /// Get all registered instances (snapshot).
    pub async fn list_instances(&self) -> Vec<InstanceInfo> {
        let instances = self.instances.read().await;
        instances.values().cloned().collect()
    }

    /// Mark an instance as draining (pre-shutdown).
    ///
    /// The load balancer should stop sending new requests to draining
    /// instances while allowing in-flight requests to complete.
    pub async fn drain(&self, instance_id: &str) {
        let mut instances = self.instances.write().await;
        if let Some(info) = instances.get_mut(instance_id) {
            info.status = InstanceStatus::Draining;
        }
    }

    /// Mark an instance as unhealthy (missed heartbeats).
    ///
    /// Called by the health checker when an instance stops sending
    /// heartbeats within the expected interval.
    pub async fn mark_unhealthy(&self, instance_id: &str) {
        let mut instances = self.instances.write().await;
        if let Some(info) = instances.get_mut(instance_id) {
            if info.status != InstanceStatus::Draining {
                info.status = InstanceStatus::Unhealthy;
            }
        }
    }

    /// Remove an instance from the registry.
    pub async fn unregister(&self, instance_id: &str) {
        let mut instances = self.instances.write().await;
        instances.remove(instance_id);
    }

    /// Get the count of healthy instances (for load balancer decisions).
    pub async fn healthy_count(&self) -> usize {
        let instances = self.instances.read().await;
        instances
            .values()
            .filter(|i| i.status == InstanceStatus::Healthy || i.status == InstanceStatus::Starting)
            .count()
    }
}

impl Default for InstanceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_instance() {
        let registry = InstanceRegistry::new();
        let id = registry.register("127.0.0.1:8000").await;

        // Should return a non-empty UUID
        assert!(!id.is_empty());
        assert_eq!(id.len(), 36); // UUID v4 string length

        let instances = registry.list_instances().await;
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].instance_id, id);
        assert_eq!(instances[0].bind_address, "127.0.0.1:8000");
        assert_eq!(instances[0].status, InstanceStatus::Starting);
    }

    #[tokio::test]
    async fn test_heartbeat_updates_status() {
        let registry = InstanceRegistry::new();
        let id = registry.register("10.0.0.1:8080").await;

        // Initially Starting
        let before = registry.list_instances().await;
        assert_eq!(before[0].status, InstanceStatus::Starting);

        // Heartbeat should transition Starting → Healthy
        registry.heartbeat(&id).await;
        let after = registry.list_instances().await;
        assert_eq!(after[0].status, InstanceStatus::Healthy);

        // Subsequent heartbeats keep it Healthy
        registry.heartbeat(&id).await;
        registry.heartbeat(&id).await;
        let still = registry.list_instances().await;
        assert_eq!(still[0].status, InstanceStatus::Healthy);
    }

    #[tokio::test]
    async fn test_list_instances() {
        let registry = InstanceRegistry::new();

        assert!(registry.list_instances().await.is_empty());

        let id1 = registry.register("10.0.0.1:8080").await;
        let id2 = registry.register("10.0.0.2:8080").await;
        let id3 = registry.register("10.0.0.3:8080").await;

        let all = registry.list_instances().await;
        assert_eq!(all.len(), 3);

        let ids: Vec<&str> = all.iter().map(|i| i.instance_id.as_str()).collect();
        assert!(ids.contains(&id1.as_str()));
        assert!(ids.contains(&id2.as_str()));
        assert!(ids.contains(&id3.as_str()));
    }

    #[tokio::test]
    async fn test_drain_and_unregister() {
        let registry = InstanceRegistry::new();
        let id = registry.register("10.0.0.1:8080").await;

        // Heartbeat to make it Healthy
        registry.heartbeat(&id).await;
        assert_eq!(
            registry.list_instances().await[0].status,
            InstanceStatus::Healthy
        );

        // Drain → status should become Draining
        registry.drain(&id).await;
        assert_eq!(
            registry.list_instances().await[0].status,
            InstanceStatus::Draining
        );

        // Optional: mark unhealthy should NOT override Draining
        registry.mark_unhealthy(&id).await;
        assert_eq!(
            registry.list_instances().await[0].status,
            InstanceStatus::Draining
        );

        // Unregister removes it
        registry.unregister(&id).await;
        assert!(registry.list_instances().await.is_empty());
    }

    #[tokio::test]
    async fn test_mark_unhealthy() {
        let registry = InstanceRegistry::new();
        let id = registry.register("10.0.0.1:8080").await;

        registry.heartbeat(&id).await;
        assert_eq!(
            registry.list_instances().await[0].status,
            InstanceStatus::Healthy
        );

        registry.mark_unhealthy(&id).await;
        assert_eq!(
            registry.list_instances().await[0].status,
            InstanceStatus::Unhealthy
        );
    }

    #[tokio::test]
    async fn test_healthy_count() {
        let registry = InstanceRegistry::new();

        assert_eq!(registry.healthy_count().await, 0);

        // Starting counts as healthy
        let id1 = registry.register("10.0.0.1:8080").await;
        assert_eq!(registry.healthy_count().await, 1);

        // Heartbeat → Healthy still counts
        registry.heartbeat(&id1).await;
        assert_eq!(registry.healthy_count().await, 1);

        // Add a second instance
        let id2 = registry.register("10.0.0.2:8080").await;
        assert_eq!(registry.healthy_count().await, 2);

        // Drain → no longer counts as healthy
        registry.drain(&id2).await;
        assert_eq!(registry.healthy_count().await, 1);
    }

    #[tokio::test]
    async fn test_unique_ids_sequential() {
        let registry = InstanceRegistry::new();
        let mut ids = Vec::new();
        for _ in 0..10 {
            let id = registry.register("127.0.0.1:9000").await;
            ids.push(id);
        }

        // All IDs should be unique
        use std::collections::HashSet;
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 10);
    }

    #[tokio::test]
    async fn test_heartbeat_nonexistent_is_noop() {
        let registry = InstanceRegistry::new();
        // Heartbeat on a non-existent ID should not panic
        registry.heartbeat("nonexistent-id").await;
        assert!(registry.list_instances().await.is_empty());
    }

    #[tokio::test]
    async fn test_drain_nonexistent_is_noop() {
        let registry = InstanceRegistry::new();
        registry.drain("nonexistent-id").await;
        assert!(registry.list_instances().await.is_empty());
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_is_noop() {
        let registry = InstanceRegistry::new();
        registry.unregister("nonexistent-id").await;
        assert!(registry.list_instances().await.is_empty());
    }

    #[tokio::test]
    async fn test_default_creates_empty_registry() {
        let registry = InstanceRegistry::default();
        assert!(registry.list_instances().await.is_empty());
    }
}
