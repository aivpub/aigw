//! OpenTelemetry tracing — W3C traceparent propagation
//!
//! Provides:
//! - extract_traceparent — extract W3C trace context from incoming HTTP headers
//! - inject_traceparent — inject W3C trace context into outgoing HTTP headers
//! - OtelTracer — manages the tracer provider lifecycle
//! - OtelConfig — configuration from config.yaml
//! - is_enabled — returns true when OTEL tracing is active (for handler gating)
//! - build_otel_layer — create tracing-opentelemetry bridge layer for subscriber
//!
//! When disabled (config.enabled = false), all functions are no-ops with zero overhead.
//!
//! Usage in handler (chat.rs / v1_messages.rs):
//! ```ignore
//! // Extract upstream traceparent before resolving model
//! let otel_ctx = otel_tracing::extract_traceparent(&headers);
//!
//! // Before sending upstream request
//! otel_tracing::inject_traceparent(&mut upstream_headers);
//! ```

use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag tracking whether OTEL has been initialized.
/// Set by OtelTracer::init(), read by is_enabled().
static OTEL_ENABLED: AtomicBool = AtomicBool::new(false);

/// OTEL configuration, read from config.yaml or deserialized from DB config.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
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
        // Also register W3C TraceContext propagator for extract/inject
        global::set_text_map_propagator(TraceContextPropagator::new());

        // Mark OTEL as enabled so handler code can conditionally extract/inject
        OTEL_ENABLED.store(true, Ordering::Relaxed);

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

/// Returns true when OTEL tracing is active (global tracer provider has been set).
/// Handler code uses this to conditionally extract/inject traceparent.
pub fn is_enabled() -> bool {
    OTEL_ENABLED.load(Ordering::Relaxed)
}

/// Build a tracing-opentelemetry layer that maps tracing::Span to OTEL spans.
/// Returns the layer — compose with `tracing_subscriber::registry().with(layer)`.
/// Caller must ensure the layer is only added when OTEL is active;
/// the global tracer resolver determines whether spans are actually exported.
pub fn build_otel_layer<S>(
) -> tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry::global::BoxedTracer>
where
    S: tracing_subscriber::layer::SubscriberExt + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    let tracer = global::tracer("aigw");
    tracing_opentelemetry::layer().with_tracer(tracer)
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use opentelemetry::trace::TraceContextExt;

    #[test]
    fn test_otel_config_default_disabled() {
        let config = OtelConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.service_name, "aigw");
        assert_eq!(config.sample_rate, 1.0);
        assert_eq!(config.exporter, "otlp_grpc");
        assert!(config.endpoint.is_none());
    }

    #[test]
    fn test_otel_disabled_init_returns_none() {
        let config = OtelConfig::default();
        let tracer = OtelTracer::init(&config);
        assert!(tracer.is_none());
    }

    #[test]
    fn test_otel_enabled_no_endpoint_returns_none() {
        let config = OtelConfig {
            enabled: true,
            endpoint: None,
            ..Default::default()
        };
        let tracer = OtelTracer::init(&config);
        assert!(tracer.is_none());
    }

    #[test]
    fn test_otel_enabled_empty_endpoint_returns_none() {
        let config = OtelConfig {
            enabled: true,
            endpoint: Some("".into()),
            ..Default::default()
        };
        let tracer = OtelTracer::init(&config);
        assert!(tracer.is_none());
    }

    #[test]
    fn test_extract_traceparent_missing_header() {
        let headers = HeaderMap::new();
        let ctx = extract_traceparent(&headers);
        // With no propagator set, extract creates a context — just verify no panic
        let _ = ctx.span();
    }

    #[test]
    fn test_extract_traceparent_empty_value() {
        let mut headers = HeaderMap::new();
        headers.insert("traceparent", HeaderValue::from_static(""));
        let ctx = extract_traceparent(&headers);
        // Empty value — just verify no panic
        let _ = ctx.span();
    }

    #[test]
    fn test_extract_traceparent_valid_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            HeaderValue::from_static(
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            ),
        );
        let ctx = extract_traceparent(&headers);
        // With a noop propagator (OTEL not initialized), the context won't be valid.
        // The test merely verifies that extract doesn't panic.
        let _ = ctx.span().span_context().is_valid();
    }

    #[test]
    fn test_inject_traceparent_no_init_no_panic() {
        // When no tracer provider is set up, inject_traceparent should not panic
        // The global propagator is a NoopTextMapPropagator by default
        let mut headers = HeaderMap::new();
        inject_traceparent(&mut headers);
        // No headers added by noop propagator — no panic either way
    }

    #[test]
    fn test_deserialize_otel_config_from_yaml() {
        let yaml = r#"
enabled: true
endpoint: "http://jaeger:4317"
service_name: "aigw-test"
sample_rate: 0.5
exporter: "otlp_http"
"#;
        let config: OtelConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.endpoint.unwrap(), "http://jaeger:4317");
        assert_eq!(config.service_name, "aigw-test");
        assert_eq!(config.sample_rate, 0.5);
        assert_eq!(config.exporter, "otlp_http");
    }

    #[test]
    fn test_deserialize_otel_config_minimal_disabled() {
        let yaml = r#"
enabled: false
"#;
        let config: OtelConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.service_name, "aigw");
    }
}
