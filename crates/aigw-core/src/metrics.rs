//! Prometheus metrics — aligned with litellm PrometheusLogger
//!
//! 14 core metrics (Counter/Histogram/Gauge) with configurable namespace.

use prometheus::{
    register_counter_vec_with_registry, register_histogram_vec_with_registry,
    register_gauge_vec_with_registry, register_int_counter_vec_with_registry,
    CounterVec, GaugeVec, HistogramVec, IntCounterVec, Registry,
};

/// Central metrics recorder holding all 14 prometheus metric families.
#[derive(Debug, Clone)]
pub struct MetricsRecorder {
    pub registry: Registry,

    // Request-level Counters
    pub total_requests: CounterVec,
    pub failed_requests: CounterVec,

    // Latency Histograms (seconds)
    pub request_latency_seconds: HistogramVec,
    pub llm_api_latency_seconds: HistogramVec,
    pub llm_api_ttft_seconds: HistogramVec,
    pub request_queue_time_seconds: HistogramVec,

    // Usage / Cost Counters
    pub spend_metric: CounterVec,
    pub tokens_metric: IntCounterVec,

    // Deployment Gauges / Counters
    pub deployment_state: GaugeVec,
    pub deployment_tpm_limit: GaugeVec,
    pub deployment_rpm_limit: GaugeVec,
    pub deployment_cooled_down: IntCounterVec,
    pub deployment_success_responses: IntCounterVec,
    pub deployment_failure_responses: IntCounterVec,
}

impl MetricsRecorder {
    /// Initialize all 14 metrics under the given namespace (e.g. "aigw").
    pub fn init(namespace: &str) -> Result<Self, prometheus::Error> {
        let registry = Registry::new_custom(Some(namespace.to_string()), None)?;

        let total_requests = register_counter_vec_with_registry!(
            format!("{}_total_requests", namespace),
            "Total proxy requests",
            &["model", "user", "status_code"],
            registry
        )?;

        let failed_requests = register_counter_vec_with_registry!(
            format!("{}_failed_requests", namespace),
            "Failed proxy requests",
            &["model", "user", "error_type"],
            registry
        )?;

        let request_latency_seconds = register_histogram_vec_with_registry!(
            format!("{}_request_latency_seconds", namespace),
            "End-to-end request latency in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.01, 2.0, 12)?,
            registry
        )?;

        let llm_api_latency_seconds = register_histogram_vec_with_registry!(
            format!("{}_llm_api_latency_seconds", namespace),
            "Upstream API latency in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.01, 2.0, 12)?,
            registry
        )?;

        let llm_api_ttft_seconds = register_histogram_vec_with_registry!(
            format!("{}_llm_api_ttft_seconds", namespace),
            "Time to first token in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.05, 2.0, 10)?,
            registry
        )?;

        let request_queue_time_seconds = register_histogram_vec_with_registry!(
            format!("{}_request_queue_time_seconds", namespace),
            "Queue time before processing in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.001, 2.0, 8)?,
            registry
        )?;

        let spend_metric = register_counter_vec_with_registry!(
            format!("{}_spend_metric", namespace),
            "Total spend in USD",
            &["model", "user"],
            registry
        )?;

        let tokens_metric = register_int_counter_vec_with_registry!(
            format!("{}_tokens_metric", namespace),
            "Total tokens processed",
            &["model", "user", "token_type"],
            registry
        )?;

        let deployment_state = register_gauge_vec_with_registry!(
            format!("{}_deployment_state", namespace),
            "Deployment health state (1=healthy, 0=unhealthy)",
            &["model", "api_base"],
            registry
        )?;

        let deployment_tpm_limit = register_gauge_vec_with_registry!(
            format!("{}_deployment_tpm_limit", namespace),
            "Deployment TPM limit",
            &["model", "api_base"],
            registry
        )?;

        let deployment_rpm_limit = register_gauge_vec_with_registry!(
            format!("{}_deployment_rpm_limit", namespace),
            "Deployment RPM limit",
            &["model", "api_base"],
            registry
        )?;

        let deployment_cooled_down = register_int_counter_vec_with_registry!(
            format!("{}_deployment_cooled_down", namespace),
            "Deployment cooldown events",
            &["model", "api_base"],
            registry
        )?;

        let deployment_success_responses = register_int_counter_vec_with_registry!(
            format!("{}_deployment_success_responses", namespace),
            "Deployment successful responses",
            &["model", "api_base"],
            registry
        )?;

        let deployment_failure_responses = register_int_counter_vec_with_registry!(
            format!("{}_deployment_failure_responses", namespace),
            "Deployment failure responses",
            &["model", "api_base"],
            registry
        )?;

        Ok(Self {
            registry,
            total_requests,
            failed_requests,
            request_latency_seconds,
            llm_api_latency_seconds,
            llm_api_ttft_seconds,
            request_queue_time_seconds,
            spend_metric,
            tokens_metric,
            deployment_state,
            deployment_tpm_limit,
            deployment_rpm_limit,
            deployment_cooled_down,
            deployment_success_responses,
            deployment_failure_responses,
        })
    }

    /// Record a completed request (success or failure).
    pub fn record_request(&self, summary: &RequestSummary) {
        let model = &summary.model;
        let user = &summary.user;

        self.total_requests
            .with_label_values(&[model, user, &summary.status_code])
            .inc();

        if summary.success {
            self.request_latency_seconds
                .with_label_values(&[model, user])
                .observe(summary.latency_secs);

            if let Some(ttft) = summary.ttft_secs {
                self.llm_api_ttft_seconds
                    .with_label_values(&[model, user])
                    .observe(ttft);
            }

            self.llm_api_latency_seconds
                .with_label_values(&[model, user])
                .observe(summary.upstream_latency_secs);

            self.spend_metric
                .with_label_values(&[model, user])
                .inc_by(summary.spend);

            let prompt_label = "prompt".to_string();
            let completion_label = "completion".to_string();
            let total_label = "total".to_string();
            self.tokens_metric
                .with_label_values(&[model, user, &prompt_label])
                .inc_by(summary.prompt_tokens as u64);
            self.tokens_metric
                .with_label_values(&[model, user, &completion_label])
                .inc_by(summary.completion_tokens as u64);
            self.tokens_metric
                .with_label_values(&[model, user, &total_label])
                .inc_by(summary.total_tokens as u64);

            if let Some(queue) = summary.queue_time_secs {
                self.request_queue_time_seconds
                    .with_label_values(&[model, user])
                    .observe(queue);
            }

            if let Some(api_base) = &summary.api_base {
                self.deployment_state
                    .with_label_values(&[model, api_base])
                    .set(1.0);
                self.deployment_success_responses
                    .with_label_values(&[model, api_base])
                    .inc();
            }
        } else {
            self.failed_requests
                .with_label_values(&[model, user, &summary.error_type])
                .inc();

            if let Some(api_base) = &summary.api_base {
                self.deployment_state
                    .with_label_values(&[model, api_base])
                    .set(0.0);
                self.deployment_failure_responses
                    .with_label_values(&[model, api_base])
                    .inc();
            }
        }
    }
}

/// Summary of a completed proxy request for metrics recording.
pub struct RequestSummary {
    pub model: String,
    pub user: String,
    pub status_code: String,
    pub success: bool,
    pub latency_secs: f64,
    pub upstream_latency_secs: f64,
    pub ttft_secs: Option<f64>,
    pub queue_time_secs: Option<f64>,
    pub spend: f64,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub error_type: String,
    pub api_base: Option<String>,
}
