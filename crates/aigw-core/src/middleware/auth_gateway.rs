//! Auth Gateway — enhanced authentication with tenant scoping for SaaS multi-tenant deployment
//!
//! Provides `TenantIdentity` which wraps `KeyIdentity` with deployment mode awareness
//! and organization-level access control. In SaaS mode, keys are scoped to their
//! organization; in OnPrem mode, there is no tenant isolation.
//!
//! # Usage
//!
//! ```rust,ignore
//! use aigw_core::middleware::auth_gateway::{TenantIdentity, DeploymentMode};
//!
//! let mode = DeploymentMode::from_str("saas");
//! let identity = TenantIdentity::new(key_identity, mode);
//! assert!(identity.can_access_org("org-123"));
//! ```

use super::KeyIdentity;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DeploymentMode
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Deployment mode determining tenant isolation behavior.
///
/// - `OnPrem`: no tenant isolation; all keys can access all data.
/// - `SaaS`: keys are scoped to their organization; cross-org access is denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    OnPrem,
    SaaS,
}

impl std::str::FromStr for DeploymentMode {
    type Err = std::convert::Infallible;

    /// Parse from a string. Recognizes "saas" (case-insensitive);
    /// anything else defaults to `OnPrem`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.to_lowercase().as_str() {
            "saas" => Self::SaaS,
            _ => Self::OnPrem,
        })
    }
}

impl DeploymentMode {
    /// Returns `true` if this is SaaS mode.
    pub fn is_saas(&self) -> bool {
        matches!(self, Self::SaaS)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TenantIdentity
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Extended identity with tenant/org context.
///
/// Wraps a [`KeyIdentity`] and adds deployment mode awareness.
/// The `organization_id` is read from the inner identity (which comes from
/// the virtual_keys database row).
#[derive(Debug, Clone)]
pub struct TenantIdentity {
    pub identity: KeyIdentity,
    pub organization_id: Option<String>,
    pub deployment_mode: DeploymentMode,
}

impl TenantIdentity {
    /// Create from a key identity and deployment mode.
    ///
    /// The `organization_id` is copied from the inner identity (from the DB row).
    pub fn new(identity: KeyIdentity, mode: DeploymentMode) -> Self {
        Self {
            organization_id: identity.organization_id.clone(),
            identity,
            deployment_mode: mode,
        }
    }

    /// Check whether this identity can access the given organization's data.
    ///
    /// - Master keys always have full access.
    /// - In OnPrem mode, all keys have unrestricted access.
    /// - In SaaS mode, a key can only access its own organization.
    pub fn can_access_org(&self, org_id: &str) -> bool {
        if self.identity.is_master_key {
            return true;
        }
        if !self.deployment_mode.is_saas() {
            return true;
        }
        self.organization_id.as_deref() == Some(org_id)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn make_identity(is_master: bool, org_id: Option<&str>) -> KeyIdentity {
        KeyIdentity {
            token_hash: "test-hash".to_string(),
            key_alias: Some("test-key".to_string()),
            user_id: Some("user-1".to_string()),
            team_id: Some("team-1".to_string()),
            organization_id: org_id.map(|s| s.to_string()),
            is_master_key: is_master,
            user_role: if is_master {
                Some("proxy_admin".to_string())
            } else {
                None
            },
        }
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // DeploymentMode tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn test_deployment_mode_parsing() {
        assert_eq!(
            DeploymentMode::from_str("saas").unwrap(),
            DeploymentMode::SaaS
        );
        assert_eq!(
            DeploymentMode::from_str("SaaS").unwrap(),
            DeploymentMode::SaaS
        );
        assert_eq!(
            DeploymentMode::from_str("SAAS").unwrap(),
            DeploymentMode::SaaS
        );
        assert_eq!(
            DeploymentMode::from_str("onprem").unwrap(),
            DeploymentMode::OnPrem
        );
        assert_eq!(
            DeploymentMode::from_str("onprem").unwrap(),
            DeploymentMode::OnPrem
        );
        assert_eq!(
            DeploymentMode::from_str("").unwrap(),
            DeploymentMode::OnPrem
        );
        assert_eq!(
            DeploymentMode::from_str("unknown").unwrap(),
            DeploymentMode::OnPrem
        );
    }

    #[test]
    fn test_deployment_mode_is_saas() {
        assert!(DeploymentMode::SaaS.is_saas());
        assert!(!DeploymentMode::OnPrem.is_saas());
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // TenantIdentity tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn test_master_key_can_access_any_org() {
        let identity = make_identity(true, None);
        let tenant = TenantIdentity::new(identity, DeploymentMode::SaaS);

        // Master key can access any org, even in SaaS mode
        assert!(tenant.can_access_org("org-1"));
        assert!(tenant.can_access_org("org-2"));
        assert!(tenant.can_access_org("any-random-org"));
    }

    #[test]
    fn test_key_can_access_own_org() {
        let identity = make_identity(false, Some("my-org"));
        let tenant = TenantIdentity::new(identity, DeploymentMode::SaaS);

        assert!(tenant.can_access_org("my-org"));
    }

    #[test]
    fn test_key_cannot_access_other_org_saas() {
        let identity = make_identity(false, Some("my-org"));
        let tenant = TenantIdentity::new(identity, DeploymentMode::SaaS);

        assert!(!tenant.can_access_org("other-org"));
        assert!(!tenant.can_access_org("different-org"));
    }

    #[test]
    fn test_key_can_access_any_org_onprem() {
        let identity = make_identity(false, Some("my-org"));
        let tenant = TenantIdentity::new(identity, DeploymentMode::OnPrem);

        // In OnPrem mode, any key can access any org
        assert!(tenant.can_access_org("my-org"));
        assert!(tenant.can_access_org("other-org"));
        assert!(tenant.can_access_org("any-org"));
    }

    #[test]
    fn test_key_without_org_in_saas() {
        let identity = make_identity(false, None);
        let tenant = TenantIdentity::new(identity, DeploymentMode::SaaS);

        // A key without organization_id cannot access any org in SaaS mode
        assert!(!tenant.can_access_org("some-org"));
    }

    #[test]
    fn test_key_without_org_in_onprem() {
        let identity = make_identity(false, None);
        let tenant = TenantIdentity::new(identity, DeploymentMode::OnPrem);

        // In OnPrem mode, no org means full access
        assert!(tenant.can_access_org("some-org"));
    }
}
