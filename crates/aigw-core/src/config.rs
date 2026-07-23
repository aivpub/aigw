//! Configuration types compatible with litellm proxy config.yaml format

use serde::{Deserialize, Serialize};
use crate::otel_tracing::OtelConfig;

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

fn default_compression_enabled() -> bool { true }
fn default_compression_level() -> u32 { 6 }
fn default_compression_algorithms() -> Vec<String> {
    vec!["gzip".into(), "deflate".into(), "brotli".into()]
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
