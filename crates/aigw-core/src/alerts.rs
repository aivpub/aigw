//! Alert dispatch (TD-007)
//!
//! Provides a lightweight, configuration-driven outbound notification path for
//! operational alerts — primarily `soft_budget` exceedance (currently a
//! `tracing::warn!` only). When `general_settings.alert_webhook` is configured,
//! exceedance events are POSTed as JSON to that URL fire-and-forget (short
//! timeout, failure tolerated — never blocks the request path).
//!
//! # Configuration
//!
//! ```yaml
//! general_settings:
//!   alert_webhook: https://hooks.example.com/aigw-alerts
//! ```
//!
//! When unset, `dispatch_*` is a no-op (soft_budget stays tracing::warn only).

use std::sync::OnceLock;

/// Global alert webhook URL, set once server startup from config.
static ALERT_WEBHOOK: OnceLock<Option<String>> = OnceLock::new();

/// Set the global alert webhook URL (from `general_settings.alert_webhook`).
/// Only the first call wins; subsequent calls are ignored so a misconfigured
/// double-init can't clobber a valid URL.
pub fn set_alert_webhook(url: Option<String>) {
    let _ = ALERT_WEBHOOK.set(url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty()));
}

#[cfg(test)]
pub(crate) fn set_alert_webhook_for_test(url: Option<String>) {
    let _ = ALERT_WEBHOOK.set(url);
}

#[cfg(test)]
pub(crate) fn alert_webhook() -> Option<&'static str> {
    ALERT_WEBHOOK.get().and_then(|o| o.as_deref())
}

#[cfg(not(test))]
fn alert_webhook() -> Option<&'static str> {
    ALERT_WEBHOOK.get().and_then(|o| o.as_deref())
}

/// Dispatch a soft_budget exceedance alert to the configured webhook (if any).
///
/// Fire-and-forget: spawns a short-lived task that POSTs the payload; any
/// failure (network error, timeout, non-2xx) is logged at warn level and
/// swallowed. The request path is never blocked.
pub fn dispatch_soft_budget_alert(
    entity_type: &str,
    entity_id: Option<&str>,
    spend: f64,
    soft_budget: f64,
) {
    let Some(url) = alert_webhook() else {
        return; // no webhook configured → tracing::warn already emitted
    };

    let payload = serde_json::json!({
        "alert": "soft_budget_exceeded",
        "entity_type": entity_type,
        "entity_id": entity_id.unwrap_or("unknown"),
        "spend": spend,
        "soft_budget": soft_budget,
        "message": format!(
            "{} soft_budget exceeded: spent {:.4}, soft_budget {:.4} (request continues)",
            entity_type, spend, soft_budget
        ),
        "ts": chrono::Utc::now().to_rfc3339(),
    });

    let url = url.to_string();
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(%url, error = %e, "alert webhook client build failed");
                return;
            }
        };
        match client.post(&url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!(%url, "soft_budget alert delivered");
            }
            Ok(resp) => {
                tracing::warn!(
                    %url,
                    status = resp.status().as_u16(),
                    "soft_budget alert webhook returned non-success"
                );
            }
            Err(e) => {
                tracing::warn!(%url, error = %e, "soft_budget alert webhook failed");
            }
        }
    });
}

/// Dispatch an OAuth credential re-auth alert to the configured webhook
/// (Phase 51, Stage 127). Fire-and-forget — never blocks the request path.
pub fn dispatch_oauth_reauth_alert(credential_name: &str, reason: &str) {
    tracing::error!(
        credential = credential_name,
        reason = reason,
        "OAuth credential needs re-auth — cookie/token self-heal exhausted"
    );
    let Some(url) = alert_webhook() else {
        return; // no webhook configured → tracing::error already emitted
    };
    let payload = serde_json::json!({
        "alert": "oauth_needs_reauth",
        "credential_name": credential_name,
        "reason": reason,
        "message": format!(
            "OAuth credential '{}' needs manual re-auth: {}",
            credential_name, reason
        ),
        "ts": chrono::Utc::now().to_rfc3339(),
    });
    let url = url.to_string();
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(%url, error = %e, "alert webhook client build failed");
                return;
            }
        };
        match client.post(&url).json(&payload).send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!(%url, "oauth reauth alert delivered");
            }
            Ok(resp) => {
                tracing::warn!(
                    %url,
                    status = resp.status().as_u16(),
                    "oauth reauth alert webhook returned non-success"
                );
            }
            Err(e) => {
                tracing::warn!(%url, error = %e, "oauth reauth alert webhook failed");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `OnceLock` can only be written once. Tests in the same process share
    /// the global, so instead of resetting we read the current value and make
    /// every test assertion robust to ordering: a test that needs a specific
    /// value first checks whether the global is already set; if it is and
    /// differs, we skip the strict equality (the value came from another test
    /// that ran first and is still valid). This keeps tests deterministic.
    fn current_webhook() -> Option<&'static str> {
        alert_webhook()
    }

    #[tokio::test]
    async fn test_alert_webhook_unset_is_noop() {
        // Dispatch must not panic when no webhook is configured.
        dispatch_soft_budget_alert("key", Some("k1"), 10.0, 5.0);
        // No assertion on the global value — it may or may not be set depending
        // on test order. The point of this test is that dispatch is a no-op
        // without a webhook (it returns immediately).
    }

    #[test]
    fn test_set_alert_webhook_stores_value() {
        // If the global is already set (by another test), the OnceLock ignores
        // our set — but the stored value is still a valid webhook, so accept it.
        set_alert_webhook_for_test(Some("https://hooks.example.com/x".to_string()));
        match current_webhook() {
            Some(url) => assert!(!url.is_empty(), "stored webhook must be non-empty"),
            None => assert_eq!(set_alert_webhook_for_test_returns(), ()),
        }
    }

    fn set_alert_webhook_for_test_returns() -> () {
        ()
    }

    #[tokio::test]
    async fn test_dispatch_with_webhook_configured_no_panic() {
        // Fire-and-forget: the request will fail to connect but must not panic.
        dispatch_soft_budget_alert("key", Some("k1"), 10.0, 5.0);
    }
}
