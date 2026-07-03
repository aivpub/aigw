//! Tenant-aware data isolation layer for multi-tenant SaaS deployment.
//!
//! Provides `TenantContext` for org-level scoping and `TenantDb` wrapper
//! around the `Database` type that optionally enforces organization filtering.
//!
//! In onprem mode, `tenant` is `None` — queries run without org filters
//! (full access). In SaaS mode, `tenant` is `Some` — all data operations
//! are scoped to the tenant's `organization_id`.

use crate::db::Database;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TenantContext
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Tenant context — used for data isolation in multi-tenant SaaS mode.
///
/// Each request carries the tenant's `organization_id`. The `TenantDb`
/// wrapper uses this to filter queries so that one organization cannot
/// access another organization's data.
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub organization_id: String,
}

impl TenantContext {
    pub fn new(org_id: String) -> Self {
        Self {
            organization_id: org_id,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TenantDb
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Tenant-aware database wrapper.
///
/// Wraps a `Database` reference with an optional `TenantContext`.
/// Use `TenantDb::new(db, Some(&tenant))` for SaaS mode and
/// `TenantDb::new(db, None)` for onprem mode.
pub struct TenantDb<'a> {
    db: &'a Database,
    tenant: Option<&'a TenantContext>,
}

impl<'a> TenantDb<'a> {
    /// Create a new tenant-aware database handle.
    ///
    /// Pass `None` for `tenant` to disable org filtering (onprem mode).
    /// Pass `Some(&tenant)` for SaaS mode with organization isolation.
    pub fn new(db: &'a Database, tenant: Option<&'a TenantContext>) -> Self {
        Self { db, tenant }
    }

    /// Return a clone of the underlying tenant context, if any.
    pub fn with_tenant(&self) -> Option<&TenantContext> {
        self.tenant
    }

    /// Check whether the given `organization_id` is authorized under
    /// the current tenant context.
    ///
    /// - `None` tenant context (onprem mode) → always returns `true`.
    /// - `Some` tenant context (SaaS mode):
    ///   - `org_id` is `None` → returns `false`.
    ///   - `org_id` matches the tenant's `organization_id` → `true`.
    ///   - `org_id` differs → `false`.
    pub fn is_authorized(&self, org_id: Option<&str>) -> bool {
        match self.tenant {
            None => true, // No tenant context = full access (onprem mode)
            Some(tenant) => org_id.is_some_and(|id| id == tenant.organization_id),
        }
    }

    /// Access the underlying database directly (e.g. for admin operations
    /// that skip tenant isolation).
    pub fn db(&self) -> &Database {
        self.db
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    async fn init_test_db() -> Database {
        Database::init("sqlite::memory:")
            .await
            .expect("init in-memory database")
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // TenantDb::is_authorized tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_authorized_same_org() {
        let db = init_test_db().await;
        let tenant = TenantContext::new("org-abc".to_string());
        let tenant_db = TenantDb::new(&db, Some(&tenant));

        // Same org should be authorized
        assert!(tenant_db.is_authorized(Some("org-abc")));
    }

    #[tokio::test]
    async fn test_tenant_unauthorized_different_org() {
        let db = init_test_db().await;
        let tenant = TenantContext::new("org-abc".to_string());
        let tenant_db = TenantDb::new(&db, Some(&tenant));

        // Different org should NOT be authorized
        assert!(!tenant_db.is_authorized(Some("org-xyz")));
    }

    #[tokio::test]
    async fn test_tenant_unauthorized_null_org_id() {
        let db = init_test_db().await;
        let tenant = TenantContext::new("org-abc".to_string());
        let tenant_db = TenantDb::new(&db, Some(&tenant));

        // None org_id in SaaS mode should NOT be authorized
        assert!(!tenant_db.is_authorized(None));
    }

    #[tokio::test]
    async fn test_no_tenant_full_access() {
        let db = init_test_db().await;
        let tenant_db = TenantDb::new(&db, None);

        // No tenant context = full access regardless of org_id
        assert!(tenant_db.is_authorized(Some("org-abc")));
        assert!(tenant_db.is_authorized(Some("org-xyz")));
        assert!(tenant_db.is_authorized(None));
    }

    #[tokio::test]
    async fn test_with_tenant_returns_context() {
        let db = init_test_db().await;
        let tenant = TenantContext::new("my-org".to_string());

        let with_tenant = TenantDb::new(&db, Some(&tenant));
        assert!(with_tenant.with_tenant().is_some());
        assert_eq!(with_tenant.with_tenant().unwrap().organization_id, "my-org");

        let without_tenant = TenantDb::new(&db, None);
        assert!(without_tenant.with_tenant().is_none());
    }

    #[tokio::test]
    async fn test_db_accessor_returns_underlying_db() {
        let db = init_test_db().await;
        let tenant = TenantContext::new("org-1".to_string());
        let tenant_db = TenantDb::new(&db, Some(&tenant));

        // The db accessor should let us use the underlying Database
        let _underlying: &Database = tenant_db.db();
        // If the pointer identity matters in real usage, we can compare
        // but for now just verify it compiles and doesn't panic.
    }

    #[tokio::test]
    async fn test_multiple_tenants_independent() {
        let db = init_test_db().await;
        let tenant_a = TenantContext::new("org-a".to_string());
        let tenant_b = TenantContext::new("org-b".to_string());

        let td_a = TenantDb::new(&db, Some(&tenant_a));
        let td_b = TenantDb::new(&db, Some(&tenant_b));

        // Each tenant sees only its own org
        assert!(td_a.is_authorized(Some("org-a")));
        assert!(!td_a.is_authorized(Some("org-b")));
        assert!(td_b.is_authorized(Some("org-b")));
        assert!(!td_b.is_authorized(Some("org-a")));
    }
}
