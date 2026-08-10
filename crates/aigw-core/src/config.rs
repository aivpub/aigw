//! Configuration types compatible with litellm proxy config.yaml format

use crate::body_archive::config::BodyArchiveConfig;
use crate::otel_tracing::OtelConfig;
use serde::{Deserialize, Serialize};

/// Budget reset configuration.
///
/// Controls periodic spend reset for entities (virtual_keys, teams, users,
/// organizations) that have `budget_duration` set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetResetConfig {
    /// Enable the periodic budget reset worker. Default: true.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for BudgetResetConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn default_true() -> bool {
    true
}

/// Top-level config (litellm-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AigwConfig {
    #[serde(rename = "general_settings", skip_serializing_if = "Option::is_none")]
    pub general_settings: Option<GeneralSettings>,

    #[serde(rename = "model_list")]
    pub model_list: Vec<ModelEntry>,

    #[serde(rename = "router_settings", skip_serializing_if = "Option::is_none")]
    pub router_settings: Option<RouterSettings>,

    #[serde(rename = "litellm_settings", skip_serializing_if = "Option::is_none")]
    pub litellm_settings: Option<serde_json::Value>,

    #[serde(
        rename = "environment_variables",
        skip_serializing_if = "Option::is_none"
    )]
    pub environment_variables: Option<serde_json::Value>,

    #[serde(rename = "body_archive", skip_serializing_if = "Option::is_none")]
    pub body_archive: Option<BodyArchiveConfig>,

    /// Budget reset config: controls periodic spend reset for entities
    /// with budget_duration set (virtual_keys, teams, users, organizations).
    #[serde(rename = "budget_reset", skip_serializing_if = "Option::is_none")]
    pub budget_reset: Option<BudgetResetConfig>,

    /// Exact-match response cache (Stage 119). Absent → enabled with defaults
    /// (memory LRU, ttl 60s, 10k entries).
    #[serde(rename = "cache", skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfig>,
}

/// Response cache configuration (Stage 119 §3.5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Master switch. `false` disables the cache layer entirely (zero cost).
    #[serde(default = "default_cache_enabled")]
    pub enabled: bool,
    /// Backend: `memory` now, `redis` reserved (M2 distributed layer).
    #[serde(default = "default_cache_backend")]
    pub backend: String,
    /// Default TTL in seconds for cached responses.
    #[serde(default = "default_cache_ttl")]
    pub ttl_seconds: u64,
    /// LRU capacity for the memory backend.
    #[serde(default = "default_cache_max_entries")]
    pub max_entries: usize,
}

fn default_cache_enabled() -> bool {
    true
}
fn default_cache_backend() -> String {
    "memory".into()
}
fn default_cache_ttl() -> u64 {
    60
}
fn default_cache_max_entries() -> usize {
    10_000
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: default_cache_enabled(),
            backend: default_cache_backend(),
            ttl_seconds: default_cache_ttl(),
            max_entries: default_cache_max_entries(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    #[serde(rename = "master_key", skip_serializing_if = "Option::is_none")]
    pub master_key: Option<String>,

    #[serde(rename = "database_url", skip_serializing_if = "Option::is_none")]
    pub database_url: Option<String>,

    #[serde(
        rename = "custom_key_generate_length",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_key_generate_length: Option<u32>,

    #[serde(rename = "disable_custom_api_keys", default)]
    pub disable_custom_api_keys: bool,

    /// Deployment mode: "saas" or "onprem"
    #[serde(rename = "deployment_mode", skip_serializing_if = "Option::is_none")]
    pub deployment_mode: Option<String>,

    /// Prometheus histogram bucket overrides — if set, these replace the defaults.
    /// Each key maps to a Vec<f64> of upper bounds. Example:
    /// ```yaml
    /// metrics_buckets:
    ///   latency: [0.5, 1, 2, 5, 10, 20, 30]
    ///   ttft: [0.25, 0.5, 1, 2.5, 5, 10]
    ///   queue_time: [0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1]
    /// ```
    #[serde(rename = "metrics_buckets", skip_serializing_if = "Option::is_none")]
    pub metrics_buckets: Option<MetricsBuckets>,

    /// Compression settings for HTTP response compression (Content-Encoding)
    #[serde(rename = "compression", skip_serializing_if = "Option::is_none")]
    pub compression: Option<CompressionConfig>,

    /// OpenTelemetry tracing configuration
    #[serde(rename = "otel", skip_serializing_if = "Option::is_none")]
    pub otel: Option<OtelConfig>,

    /// Maximum accepted HTTP request body size, in MiB.
    ///
    /// Applied as axum's `DefaultBodyLimit::max`. Defaults to 32 MiB when unset,
    /// which covers large LLM requests (long context, tool definitions, base64
    /// attachments) while still capping abuse. Set to 0 to restore axum's built-in
    /// 2 MiB default.
    #[serde(
        rename = "request_body_limit_mb",
        skip_serializing_if = "Option::is_none"
    )]
    pub request_body_limit_mb: Option<u32>,

    /// Optional outbound alert webhook URL (TD-007). When set, soft_budget
    /// exceedance is POSTed here (JSON, fire-and-forget) in addition to the
    /// existing tracing::warn log. Points at any HTTP endpoint (Slack/Feishu
    /// incoming webhook, custom alert service, ...). Unset → alerts stay
    /// log-only.
    #[serde(rename = "alert_webhook", skip_serializing_if = "Option::is_none")]
    pub alert_webhook: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Enable response compression (default: true)
    #[serde(default = "default_compression_enabled")]
    pub enabled: bool,

    /// Compression level 0-9 (default: 6). Applied to gzip and deflate.
    /// Brotli uses its own quality scale (0-11) mapped from this value.
    #[serde(default = "default_compression_level")]
    pub level: u32,

    /// Allowed algorithms in preference order. Supported: "gzip", "deflate", "brotli".
    /// Default: ["gzip", "deflate", "brotli"]
    #[serde(default = "default_compression_algorithms")]
    pub algorithms: Vec<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            level: 6,
            algorithms: default_compression_algorithms(),
        }
    }
}

fn default_compression_enabled() -> bool {
    true
}
fn default_compression_level() -> u32 {
    6
}
fn default_compression_algorithms() -> Vec<String> {
    vec!["gzip".into(), "deflate".into(), "brotli".into()]
}

/// Default request body limit when `general_settings.request_body_limit_mb` is unset.
/// 32 MiB — covers large LLM requests (long context, tool definitions, base64 attachments).
pub const DEFAULT_REQUEST_BODY_LIMIT_MB: u32 = 32;

/// Resolve the effective request body limit (in bytes) from the config option.
///
/// Rules:
/// - `None` or `Some(0)` → restore axum's built-in 2 MiB default (pass None upstream).
/// - `Some(n)` with `n > 0` → `n` MiB.
///
/// Returns `Some(bytes)` to apply via `DefaultBodyLimit::max`, or `None` to keep
/// axum's built-in default.
pub fn resolve_body_limit_bytes(mb: Option<u32>) -> Option<usize> {
    match mb.unwrap_or(DEFAULT_REQUEST_BODY_LIMIT_MB) {
        0 => None,
        n => Some(
            usize::try_from(n)
                .unwrap_or(usize::MAX)
                .saturating_mul(1024 * 1024),
        ),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsBuckets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_time: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    #[serde(rename = "model_name")]
    pub model_name: String,

    #[serde(rename = "litellm_params")]
    pub litellm_params: ModelParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelParams {
    pub model: String,
    #[serde(rename = "api_base", skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    #[serde(rename = "api_key", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(rename = "rpm", skip_serializing_if = "Option::is_none")]
    pub rpm: Option<i32>,
    #[serde(rename = "tpm", skip_serializing_if = "Option::is_none")]
    pub tpm: Option<i32>,
    #[serde(
        rename = "max_parallel_requests",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_requests: Option<i32>,
    #[serde(
        rename = "input_cost_per_token",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_cost_per_token: Option<f64>,
    #[serde(
        rename = "output_cost_per_token",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_cost_per_token: Option<f64>,
    #[serde(rename = "tpm_limit", skip_serializing_if = "Option::is_none")]
    pub tpm_limit: Option<i32>,
    #[serde(rename = "rpm_limit", skip_serializing_if = "Option::is_none")]
    pub rpm_limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterSettings {
    #[serde(rename = "routing_strategy", skip_serializing_if = "Option::is_none")]
    pub routing_strategy: Option<String>,

    #[serde(rename = "allowed_fails", default = "default_allowed_fails")]
    pub allowed_fails: i32,

    #[serde(rename = "num_retries", default = "default_num_retries")]
    pub num_retries: i32,

    #[serde(rename = "cooldown_time", default = "default_cooldown")]
    pub cooldown_time: f64,

    #[serde(rename = "fallbacks", skip_serializing_if = "Option::is_none")]
    pub fallbacks: Option<Vec<serde_json::Value>>,
}

fn default_allowed_fails() -> i32 {
    3
}
fn default_num_retries() -> i32 {
    2
}
fn default_cooldown() -> f64 {
    30.0
}

/// Re-export for backward compatibility
pub type ModelInfo = ModelEntry;
pub use ModelParams as LitellmParams;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_limit_unset_defaults_to_32mib() {
        assert_eq!(resolve_body_limit_bytes(None), Some(32 * 1024 * 1024));
    }

    #[test]
    fn body_limit_explicit_value() {
        assert_eq!(resolve_body_limit_bytes(Some(64)), Some(64 * 1024 * 1024));
        assert_eq!(resolve_body_limit_bytes(Some(1)), Some(1024 * 1024));
    }

    #[test]
    fn body_limit_zero_opts_out_to_axum_default() {
        assert_eq!(resolve_body_limit_bytes(Some(0)), None);
    }

    #[test]
    fn body_limit_parsed_from_yaml_default() {
        // Unset field → serde yields None → default 32 MiB.
        let yaml = "general_settings: {}\nmodel_list: []\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.general_settings.unwrap().request_body_limit_mb, None);
    }

    #[test]
    fn body_limit_parsed_from_yaml_explicit() {
        let yaml = "general_settings:\n  request_body_limit_mb: 50\nmodel_list: []\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            cfg.general_settings.unwrap().request_body_limit_mb,
            Some(50)
        );
    }

    #[test]
    fn alert_webhook_parsed_from_yaml() {
        // TD-007: general_settings.alert_webhook deserializes.
        let yaml =
            "general_settings:\n  alert_webhook: https://hooks.example.com/aigw\nmodel_list: []\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            cfg.general_settings.unwrap().alert_webhook.as_deref(),
            Some("https://hooks.example.com/aigw")
        );
    }

    #[test]
    fn alert_webhook_absent_is_none() {
        let yaml = "general_settings: {}\nmodel_list: []\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.general_settings.unwrap().alert_webhook.is_none());
    }

    #[test]
    fn cache_config_absent_defaults_to_enabled() {
        // Stage 119: absent `cache` block → CacheConfig::default() → enabled.
        let yaml = "model_list: []\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.cache.is_none(), "absent cache block → None");
        let cc = cfg.cache.clone().unwrap_or_default();
        assert!(cc.enabled);
        assert_eq!(cc.backend, "memory");
        assert_eq!(cc.ttl_seconds, 60);
        assert_eq!(cc.max_entries, 10_000);
    }

    #[test]
    fn cache_config_parsed_from_yaml() {
        // Stage 119: explicit `cache` block deserializes and overrides defaults.
        let yaml =
            "model_list: []\ncache:\n  enabled: false\n  ttl_seconds: 300\n  max_entries: 500\n";
        let cfg: AigwConfig = serde_yaml::from_str(yaml).unwrap();
        let cc = cfg.cache.expect("cache block present");
        assert!(!cc.enabled);
        assert_eq!(cc.backend, "memory");
        assert_eq!(cc.ttl_seconds, 300);
        assert_eq!(cc.max_entries, 500);
    }
}
