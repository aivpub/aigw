//! OpenTelemetry tracing — W3C traceparent propagation
//!
//! Provides:
//! - extract_traceparent — extract W3C trace context from incoming HTTP headers
//! - inject_traceparent — inject W3C trace context into outgoing HTTP headers
//! - OtelTracer — manages the tracer provider lifecycle
//! - OtelConfig — configuration from config.yaml
//!
//! When disabled (config.enabled = false), all functions are no-ops with zero overhead.
//!
//! Usage in handler (chat.rs / v1_messages.rs):
//! ```ignore
//! // Extract upstream traceparent before resolving model
//! let otel_ctx = otel_tracing::extract_traceparent(req.headers());
//!
//! // Before sending upstream request
//! otel_tracing::inject_traceparent(&mut upstream_headers);
//! ```

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;

/// OTEL configuration, read from config.yaml or deserialized from DB config.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OtelConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_service_name")]
    pub service_name: String,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: f64,
    #[serde(default = "default_exporter")]
    pub exporter: String,
}

fn default_service_name() -> String { "aigw".into() }
fn default_sample_rate() -> f64 { 1.0 }
fn default_exporter() -> String { "otlp_grpc".into() }

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: None,
            service_name: "aigw".into(),
            sample_rate: 1.0,
            exporter: "otlp_grpc".into(),
        }
    }
}

/// Wrapper around the OTEL tracer provider.
/// When `config.enabled` is false, `init` is a no-op.
pub struct OtelTracer {
    provider: Option<SdkTracerProvider>,
    _tracer_name: String,
}

impl OtelTracer {
    /// Initialize the OTEL tracer provider.
    /// Returns None when OTEL is disabled (zero overhead).
    pub fn init(config: &OtelConfig) -> Option<Self> {
        if !config.enabled {
            tracing::info!("OTEL tracing disabled");
            return None;
        }

        let endpoint = match &config.endpoint {
            Some(e) if !e.is_empty() => e.clone(),
            _ => {
                tracing::warn!("OTEL enabled but no endpoint configured, disabling");
                return None;
            }
        };

        tracing::info!(
            "Initializing OTEL tracing: endpoint={}, service={}",
            endpoint,
            config.service_name
        );

        let exporter = match config.exporter.as_str() {
            "otlp_http" => opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .with_endpoint(&endpoint)
                .build(),
            _ => opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(&endpoint)
                .build(),
        };

        let exporter = match exporter {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to create OTEL exporter: {}, disabling", e);
                return None;
            }
        };

        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();

        global::set_tracer_provider(provider.clone());

        Some(Self {
            provider: Some(provider),
            _tracer_name: config.service_name.clone(),
        })
    }

    /// Returns true when OTEL tracing is active.
    pub fn is_active(&self) -> bool {
        self.provider.is_some()
    }

    /// Shutdown the tracer provider, flushing pending spans.
    pub fn shutdown(&self) {
        if let Some(ref provider) = self.provider {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for OtelTracer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Extract W3C traceparent from HTTP headers, returning the parent context.
/// Returns Context::current() if no traceparent header is present.
pub fn extract_traceparent(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    let extractor = HeaderExtractor(headers);
    global::get_text_map_propagator(|propagator| propagator.extract(&extractor))
}

/// Inject the current span context as traceparent + tracestate headers
/// into the given HeaderMap for downstream propagation.
pub fn inject_traceparent(headers: &mut axum::http::HeaderMap) {
    let mut injector = HeaderInjector(headers);
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&opentelemetry::Context::current(), &mut injector)
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// W3C propagator helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);

impl<'a> opentelemetry::propagation::Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

struct HeaderInjector<'a>(&'a mut axum::http::HeaderMap);

impl<'a> opentelemetry::propagation::Injector for HeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(k) = axum::http::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(v) = axum::http::HeaderValue::from_str(&value) {
                self.0.insert(k, v);
            }
        }
    }
}
