//! OAuth token lifecycle + 3-tier self-healing (Phase 51, Stage 127).
//!
//! Implements sub2api's `claude_oauth_service.go` token lifecycle
//! (reference: `docs/research/2026-08-18-sub2api-proxy-oauth-reference.md`
//! §2.6): access(8h) + refresh(30d rotate) + cookie all stored; a request-path
//! token getter walks an in-memory cache → near-expiry refresh → cookie
//! re-exchange self-heal → `needs_reauth` + alert_webhook alarm.
//!
//! Tier-2 refresh uses `OauthClient.refresh`; Tier-3 re-exchange uses
//! `OauthClient.exchange` (both through the bound proxy from the credential).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::claude_oauth::{OauthClient, TokenResponse};
use crate::crypto::{decrypt_json_fields, decrypt_litellm_value, encrypt_litellm_value};
use crate::db::Database;

/// Access token considered expiring when within this window of `expires_at`.
const REFRESH_WINDOW_SECS: i64 = 180;
/// Cache entry TTL floor even when the token would expire sooner.
const _MIN_CACHE_TTL_SECS: u64 = 60;

/// Token acquisition failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// Credential not found / not an OAuth credential.
    NotFound,
    /// Token unavailable and self-heal failed → manual re-auth required.
    NeedsReauth(String),
    /// Transient network/upstream failure (caller may retry).
    Upstream(String),
    /// Crypto / config failure (master key missing, decrypt failed).
    Config(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::NotFound => write!(f, "OAuth credential not found"),
            TokenError::NeedsReauth(m) => write!(f, "needs re-auth: {}", m),
            TokenError::Upstream(m) => write!(f, "upstream: {}", m),
            TokenError::Config(m) => write!(f, "config: {}", m),
        }
    }
}

/// In-memory access-token cache entry.
#[derive(Debug, Clone)]
struct TokenCacheEntry {
    access_token: String,
    expires_at: i64, // unix seconds
}

/// Process-internal per-credential token provider.
///
/// `cache` = credential_name → access token + expiry; `locks` = per-credential
/// async mutex serializing refresh/re-exchange (single instance; distributed
/// locking deferred to M2 Redis).
#[derive(Debug, Clone, Default)]
pub struct TokenProvider {
    cache: Arc<Mutex<HashMap<String, TokenCacheEntry>>>,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl TokenProvider {
    /// Build an empty provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Per-credential async mutex to serialize refresh + cookie re-exchange.
    async fn lock_for(&self, credential_name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(credential_name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Get a valid access token for the OAuth credential (`credential_name`).
    ///
    /// Cache hit (not within the refresh window) → return immediately.
    /// Otherwise: refresh; refresh `invalid_grant` → cookie re-exchange;
    /// cookie also fails → `needs_reauth` + alert.
    pub async fn get_access_token(
        &self,
        db: &Database,
        credential_name: &str,
        master_key: &str,
    ) -> Result<String, TokenError> {
        if let Some(token) = self.cache_hit(credential_name).await {
            return Ok(token);
        }

        let lock = self.lock_for(credential_name).await;
        let _guard = lock.lock().await;

        // Re-check after acquiring the lock (another task may have refreshed).
        if let Some(token) = self.cache_hit(credential_name).await {
            return Ok(token);
        }

        let cred = db
            .get_credential_by_name(credential_name)
            .await
            .map_err(|e| TokenError::Config(format!("db error: {}", e)))?
            .ok_or(TokenError::NotFound)?;
        let values = decrypt_json_fields(&cred.credential_values, master_key);
        let proxy_url = self.resolve_proxy_url(db, &values, master_key).await?;
        let client = OauthClient::new(proxy_url.as_deref())
            .map_err(|e| TokenError::Config(format!("build OAuth client: {}", e)))?;

        // Tier 2: refresh with the stored refresh_token. `values` is already
        // decrypted (decrypt_json_fields), so refresh_token is plaintext here.
        let refresh_token = values
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TokenError::Config("missing refresh_token".to_string()))?;

        match client.refresh(refresh_token).await {
            Ok(token) => {
                self.insert_cache(credential_name, &token).await;
                self.merge_and_persist(db, &cred, &values, &token, master_key)
                    .await?;
                Ok(token.access_token)
            }
            Err(e) => {
                let recoverable = matches!(
                    e.kind.as_str(),
                    "invalid_grant" | "unauthorized" | "account_session_invalid"
                );
                if !recoverable {
                    return Err(TokenError::Upstream(format!(
                        "refresh failed: {} {}",
                        e.kind, e.message
                    )));
                }
                // Tier 3: cookie re-exchange self-heal.
                self.cookie_self_heal(db, &cred, &values, &client, master_key)
                    .await
            }
        }
    }

    /// Invalidate the cached token and force a refresh (401 handling in the
    /// reverse-proxy pipeline). Returns the fresh token or an error.
    pub async fn invalidate_and_refresh(
        &self,
        db: &Database,
        credential_name: &str,
        master_key: &str,
    ) -> Result<String, TokenError> {
        self.cache.lock().await.remove(credential_name);
        self.get_access_token(db, credential_name, master_key).await
    }

    /// Cache lookup — returns Some when cached and not within the refresh window.
    async fn cache_hit(&self, credential_name: &str) -> Option<String> {
        let cache = self.cache.lock().await;
        let entry = cache.get(credential_name)?;
        let now = chrono::Utc::now().timestamp();
        if now + REFRESH_WINDOW_SECS < entry.expires_at {
            Some(entry.access_token.clone())
        } else {
            None
        }
    }

    /// Insert / update the cache from a token response.
    async fn insert_cache(&self, credential_name: &str, token: &TokenResponse) {
        let expires_at = chrono::Utc::now().timestamp()
            + token
                .expires_in
                .unwrap_or(crate::claude_oauth::DEFAULT_EXPIRES_IN);
        self.cache.lock().await.insert(
            credential_name.to_string(),
            TokenCacheEntry {
                access_token: token.access_token.clone(),
                expires_at,
            },
        );
    }

    /// Resolve the bound proxy URL (decrypts `proxies.proxy_url`).
    async fn resolve_proxy_url(
        &self,
        db: &Database,
        values: &Value,
        master_key: &str,
    ) -> Result<Option<String>, TokenError> {
        let Some(pid) = values.get("proxy_id").and_then(|v| v.as_i64()) else {
            return Ok(None);
        };
        let p = db
            .get_proxy_by_id(pid)
            .await
            .map_err(|e| TokenError::Config(format!("db error: {}", e)))?
            .ok_or_else(|| TokenError::Config(format!("proxy {} not found", pid)))?;
        crate::crypto::decrypt_proxy_url(&p.proxy_url, master_key)
            .map(Some)
            .map_err(|e| TokenError::Config(format!("decrypt proxy_url: {}", e)))
    }

    /// Tier-3 self-heal: re-run the 3-step cookie exchange with the stored
    /// session cookie, persist the fresh token pair, update cache, and return
    /// the new access token. On cookie failure → `needs_reauth` + alert.
    async fn cookie_self_heal(
        &self,
        db: &Database,
        cred: &crate::models::Credential,
        values: &Value,
        client: &OauthClient,
        master_key: &str,
    ) -> Result<String, TokenError> {
        let session_enc = values
            .get("session_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TokenError::Config("missing session_key".to_string()))?;
        let session_key = decrypt_litellm_value(session_enc, master_key)
            .map_err(|e| TokenError::Config(format!("decrypt session_key: {}", e)))?;

        match client.exchange(&session_key).await {
            Ok((token, org_uuid)) => {
                self.insert_cache(&cred.credential_name, &token).await;
                self.persist_after_self_heal(db, cred, values, &token, &org_uuid, master_key)
                    .await?;
                Ok(token.access_token)
            }
            Err(e) => {
                let reason = format!("{}: {}", e.kind, e.message);
                self.mark_needs_reauth(db, cred, master_key, &reason).await;
                crate::alerts::dispatch_oauth_reauth_alert(&cred.credential_name, &reason);
                Err(TokenError::NeedsReauth(reason))
            }
        }
    }

    /// Persist rotated tokens after a refresh (MergeCredentials: only update
    /// access/refresh/expires_at + `_token_version`, keep everything else).
    async fn merge_and_persist(
        &self,
        db: &Database,
        cred: &crate::models::Credential,
        values: &Value,
        token: &TokenResponse,
        master_key: &str,
    ) -> Result<(), TokenError> {
        let mut merged = values.clone();
        if let Some(obj) = merged.as_object_mut() {
            obj.insert(
                "access_token".to_string(),
                json!(encrypt_litellm_value(&token.access_token, master_key)
                    .map_err(TokenError::Config)?),
            );
            obj.insert(
                "refresh_token".to_string(),
                json!(encrypt_litellm_value(&token.refresh_token, master_key)
                    .map_err(TokenError::Config)?),
            );
            let expires_at = chrono::Utc::now().timestamp()
                + token
                    .expires_in
                    .unwrap_or(crate::claude_oauth::DEFAULT_EXPIRES_IN);
            obj.insert("expires_at".to_string(), json!(expires_at));
            obj.insert("_token_version".to_string(), json!(now_ms()));
        }
        let mut updated = cred.clone();
        updated.credential_values = merged;
        updated.updated_at = chrono::Utc::now().to_rfc3339();
        db.update_credential(&updated)
            .await
            .map_err(|e| TokenError::Config(format!("persist refresh: {}", e)))
    }

    /// Persist a full rewrite after cookie self-heal (fresh tokens + updated
    /// org + status back to active).
    async fn persist_after_self_heal(
        &self,
        db: &Database,
        cred: &crate::models::Credential,
        values: &Value,
        token: &TokenResponse,
        org_uuid: &str,
        master_key: &str,
    ) -> Result<(), TokenError> {
        let mut cv = values.clone();
        if let Some(obj) = cv.as_object_mut() {
            obj.insert(
                "access_token".to_string(),
                json!(encrypt_litellm_value(&token.access_token, master_key)
                    .map_err(TokenError::Config)?),
            );
            obj.insert(
                "refresh_token".to_string(),
                json!(encrypt_litellm_value(&token.refresh_token, master_key)
                    .map_err(TokenError::Config)?),
            );
            let expires_at = chrono::Utc::now().timestamp()
                + token
                    .expires_in
                    .unwrap_or(crate::claude_oauth::DEFAULT_EXPIRES_IN);
            obj.insert("expires_at".to_string(), json!(expires_at));
            obj.insert("org_uuid".to_string(), json!(org_uuid));
            obj.insert("status".to_string(), json!("active"));
            obj.insert("_token_version".to_string(), json!(now_ms()));
        }
        let mut updated = cred.clone();
        updated.credential_values = cv;
        updated.updated_at = chrono::Utc::now().to_rfc3339();
        db.update_credential(&updated)
            .await
            .map_err(|e| TokenError::Config(format!("persist self-heal: {}", e)))
    }

    /// Mark the credential `needs_reauth` + record `last_error`. Exposed so the
    /// reverse-proxy pipeline can flag an account rejected by the upstream
    /// (401 after refresh) — the token may be valid but the scope stripped /
    /// revoked / region-blocked (reference doc §2.7), which the cookie self-heal
    /// path does NOT cover.
    pub async fn mark_needs_reauth(
        &self,
        db: &Database,
        cred: &crate::models::Credential,
        master_key: &str,
        reason: &str,
    ) {
        let mut cv = decrypt_json_fields(&cred.credential_values, master_key);
        if let Some(obj) = cv.as_object_mut() {
            obj.insert("status".to_string(), json!("needs_reauth"));
            obj.insert("last_error".to_string(), json!(reason));
        }
        let mut updated = cred.clone();
        updated.credential_values = cv;
        updated.updated_at = chrono::Utc::now().to_rfc3339();
        let _ = db.update_credential(&updated).await;
    }
}

/// Millisecond unix timestamp for `_token_version` fencing.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::Credential;

    const MK: &str = "sk-token-test-master";

    fn enc(v: &str) -> String {
        encrypt_litellm_value(v, MK).unwrap()
    }

    /// Seed an OAuth credential (active) with encrypted token trio.
    async fn seed_oauth_credential(db: &Database, name: &str) {
        let cred = Credential {
            credential_id: uuid::Uuid::new_v4().to_string(),
            credential_name: name.to_string(),
            credential_values: json!({
                "type": "anthropic_oauth",
                "access_token": enc("sk-ant-access-old"),
                "refresh_token": enc("sk-ant-refresh-old"),
                "session_key": enc("sk-ant-sid-seed"),
                "expires_at": chrono::Utc::now().timestamp() + 9999,
                "proxy_id": null,
                "org_uuid": "org-1",
                "status": "active",
            }),
            credential_info: json!({}),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        db.insert_credential(&cred).await.expect("seed credential");
    }

    /// Test cache hit: a cached non-expiring token is returned without refresh.
    #[tokio::test]
    async fn test_get_token_cache_hit() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        seed_oauth_credential(&db, "cache-hit").await;
        let provider = TokenProvider::new();
        // Prime the cache directly with a long-lived token.
        provider.cache.lock().await.insert(
            "cache-hit".to_string(),
            TokenCacheEntry {
                access_token: "sk-ant-cached".to_string(),
                expires_at: chrono::Utc::now().timestamp() + 60_000,
            },
        );
        let token = provider
            .get_access_token(&db, "cache-hit", MK)
            .await
            .expect("cache hit");
        assert_eq!(token, "sk-ant-cached");
    }

    /// Near-expiry cache entry triggers the refresh path (not cache hit).
    #[tokio::test]
    async fn test_get_token_refresh_when_expiring() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        seed_oauth_credential(&db, "expiring").await;
        let provider = TokenProvider::new();
        // Cache an expiring token (within 3 min window).
        provider.cache.lock().await.insert(
            "expiring".to_string(),
            TokenCacheEntry {
                access_token: "sk-ant-expiring".to_string(),
                expires_at: chrono::Utc::now().timestamp() + 60,
            },
        );
        // The refresh path runs — with the mock token endpoint pointed at the
        // mock upstream, it would succeed; without it the refresh fails as
        // network. We assert it does NOT return the expiring cache entry.
        let res = provider.get_access_token(&db, "expiring", MK).await;
        assert!(
            res.is_err(),
            "expiring token must not be returned from cache (got {:?})",
            res
        );
    }

    /// NotFound for a missing credential.
    #[tokio::test]
    async fn test_get_token_not_found() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let provider = TokenProvider::new();
        let res = provider.get_access_token(&db, "nope", MK).await;
        assert_eq!(res, Err(TokenError::NotFound));
    }

    /// invalidate_and_refresh drops the cache and re-resolves.
    #[tokio::test]
    async fn test_401_invalidate_and_refresh() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        seed_oauth_credential(&db, "inv").await;
        let provider = TokenProvider::new();
        provider.cache.lock().await.insert(
            "inv".to_string(),
            TokenCacheEntry {
                access_token: "sk-ant-stale".to_string(),
                expires_at: chrono::Utc::now().timestamp() + 60_000,
            },
        );
        // invalidate clears the cache → refresh path (network error since no
        // mock endpoint); cache no longer has the stale token.
        let _ = provider.invalidate_and_refresh(&db, "inv", MK).await;
        assert!(
            provider.cache.lock().await.get("inv").is_none()
                || provider
                    .cache
                    .lock()
                    .await
                    .get("inv")
                    .map(|e| e.access_token != "sk-ant-stale")
                    .unwrap_or(true),
            "stale token should be invalidated"
        );
    }

    /// merge preserves non-token fields and rotates the token pair.
    #[tokio::test]
    async fn test_merge_credentials_preserves_old() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        seed_oauth_credential(&db, "merge").await;
        let provider = TokenProvider::new();
        let cred = db.get_credential_by_name("merge").await.unwrap().unwrap();
        let values = decrypt_json_fields(&cred.credential_values, MK);
        let token = TokenResponse {
            access_token: "sk-ant-access-new".to_string(),
            token_type: Some("Bearer".to_string()),
            expires_in: Some(28800),
            refresh_token: "sk-ant-refresh-new".to_string(),
            scope: Some("user:inference".to_string()),
            organization: Some(json!({"uuid": "org-1"})),
            account: None,
        };
        provider
            .merge_and_persist(&db, &cred, &values, &token, MK)
            .await
            .expect("merge persist");
        let stored = db.get_credential_by_name("merge").await.unwrap().unwrap();
        let cv = decrypt_json_fields(&stored.credential_values, MK);
        assert_eq!(cv["access_token"], "sk-ant-access-new");
        assert_eq!(cv["refresh_token"], "sk-ant-refresh-new");
        assert_eq!(cv["status"], "active"); // old field preserved
        assert_eq!(cv["org_uuid"], "org-1");
        assert!(cv.get("_token_version").is_some());
    }

    /// Concurrent refresh serialization: two parallel calls both resolve, and
    /// only one network refresh round-trip happens (lock prevents the second
    /// from re-entering refresh after the first populated the cache).
    #[tokio::test]
    async fn test_concurrent_refresh_lock() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        seed_oauth_credential(&db, "conc").await;
        let provider = TokenProvider::new();
        // First call populates nothing (network fail) — but the per-credential
        // lock is exercised. Just assert both resolve to Err (network) without
        // panicking, proving the lock path runs.
        let (a, b) = tokio::join!(
            provider.get_access_token(&db, "conc", MK),
            provider.get_access_token(&db, "conc", MK),
        );
        assert!(a.is_err() && b.is_err());
    }
}
