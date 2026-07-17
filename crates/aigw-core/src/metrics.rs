//! Prometheus metrics — aligned with litellm PrometheusLogger
//!
//! 14 core metrics (Counter/Histogram/Gauge) with configurable namespace.

use prometheus::{
    register_counter_vec, register_histogram_vec,
    register_gauge_vec, register_int_counter_vec,
    CounterVec, GaugeVec, HistogramVec, IntCounterVec,
};

/// Central metrics recorder holding all 14 prometheus metric families.
/// All metrics are registered in the global default registry so `prometheus::gather()` works.
#[derive(Debug, Clone)]
pub struct MetricsRecorder {
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
        let total_requests = register_counter_vec!(
            format!("{}_total_requests", namespace),
            "Total proxy requests",
            &["model", "user", "status_code"]
        )?;

        let failed_requests = register_counter_vec!(
            format!("{}_failed_requests", namespace),
            "Failed proxy requests",
            &["model", "user", "error_type"]
        )?;

        let request_latency_seconds = register_histogram_vec!(
            format!("{}_request_latency_seconds", namespace),
            "End-to-end request latency in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.01, 2.0, 12)?
        )?;

        let llm_api_latency_seconds = register_histogram_vec!(
            format!("{}_llm_api_latency_seconds", namespace),
            "Upstream API latency in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.01, 2.0, 12)?
        )?;

        let llm_api_ttft_seconds = register_histogram_vec!(
            format!("{}_llm_api_ttft_seconds", namespace),
            "Time to first token in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.05, 2.0, 10)?
        )?;

        let request_queue_time_seconds = register_histogram_vec!(
            format!("{}_request_queue_time_seconds", namespace),
            "Queue time before processing in seconds",
            &["model", "user"],
            prometheus::exponential_buckets(0.001, 2.0, 8)?
        )?;

        let spend_metric = register_counter_vec!(
            format!("{}_spend_metric", namespace),
            "Total spend in USD",
            &["model", "user"]
        )?;

        let tokens_metric = register_int_counter_vec!(
            format!("{}_tokens_metric", namespace),
            "Total tokens processed",
            &["model", "user", "token_type"]
        )?;

        let deployment_state = register_gauge_vec!(
            format!("{}_deployment_state", namespace),
            "Deployment health state (1=healthy, 0=unhealthy)",
            &["model", "api_base"]
        )?;

        let deployment_tpm_limit = register_gauge_vec!(
            format!("{}_deployment_tpm_limit", namespace),
            "Deployment TPM limit",
            &["model", "api_base"]
        )?;

        let deployment_rpm_limit = register_gauge_vec!(
            format!("{}_deployment_rpm_limit", namespace),
            "Deployment RPM limit",
            &["model", "api_base"]
        )?;

        let deployment_cooled_down = register_int_counter_vec!(
            format!("{}_deployment_cooled_down", namespace),
            "Deployment cooldown events",
            &["model", "api_base"]
        )?;

        let deployment_success_responses = register_int_counter_vec!(
            format!("{}_deployment_success_responses", namespace),
            "Deployment successful responses",
            &["model", "api_base"]
        )?;

        let deployment_failure_responses = register_int_counter_vec!(
            format!("{}_deployment_failure_responses", namespace),
            "Deployment failure responses",
            &["model", "api_base"]
        )?;

        Ok(Self {
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

    /// Seed each Counter/Histogram with a zero value so they appear in /metrics output
    /// even before the first real request hits.
    pub fn seed_zero_values(&self) {
        let label = "";
        self.total_requests.with_label_values(&["_", "_", "_"]).inc_by(0.0);
        self.failed_requests.with_label_values(&["_", "_", "_"]).inc_by(0.0);
        self.request_latency_seconds.with_label_values(&["_", "_"]).observe(0.0);
        self.llm_api_latency_seconds.with_label_values(&["_", "_"]).observe(0.0);
        self.llm_api_ttft_seconds.with_label_values(&["_", "_"]).observe(0.0);
        self.request_queue_time_seconds.with_label_values(&["_", "_"]).observe(0.0);
        self.spend_metric.with_label_values(&["_", "_"]).inc_by(0.0);
        self.tokens_metric.with_label_values(&["_", "_", "_"]).inc_by(0);
        self.deployment_state.with_label_values(&["_", "_"]).set(0.0);
        self.deployment_tpm_limit.with_label_values(&["_", "_"]).set(0.0);
        self.deployment_rpm_limit.with_label_values(&["_", "_"]).set(0.0);
        self.deployment_cooled_down.with_label_values(&["_", "_"]).inc_by(0);
        self.deployment_success_responses.with_label_values(&["_", "_"]).inc_by(0);
        self.deployment_failure_responses.with_label_values(&["_", "_"]).inc_by(0);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_and_gather_non_empty() {
        let recorder = MetricsRecorder::init("aigw_test").expect("init metrics");
        recorder.seed_zero_values();
        // After seeding, gather() should return metric families
        let families = prometheus::gather();
        assert!(!families.is_empty(), "gather should return non-empty metric families after init + seed");

        // Verify at least the 14 families exist
        let names: Vec<&str> = families.iter().map(|f| f.get_name()).collect();
        assert!(names.contains(&"aigw_test_total_requests"));
        assert!(names.contains(&"aigw_test_failed_requests"));
        assert!(names.contains(&"aigw_test_request_latency_seconds"));
        assert!(names.contains(&"aigw_test_llm_api_latency_seconds"));
        assert!(names.contains(&"aigw_test_llm_api_ttft_seconds"));
        assert!(names.contains(&"aigw_test_request_queue_time_seconds"));
        assert!(names.contains(&"aigw_test_spend_metric"));
        assert!(names.contains(&"aigw_test_tokens_metric"));
        assert!(names.contains(&"aigw_test_deployment_state"));
        assert!(names.contains(&"aigw_test_deployment_tpm_limit"));
        assert!(names.contains(&"aigw_test_deployment_rpm_limit"));
        assert!(names.contains(&"aigw_test_deployment_cooled_down"));
        assert!(names.contains(&"aigw_test_deployment_success_responses"));
        assert!(names.contains(&"aigw_test_deployment_failure_responses"));
    }

    #[test]
    fn test_record_request_increments_counter() {
        let recorder = MetricsRecorder::init("aigw_req").expect("init");
        recorder.record_request(&RequestSummary {
            model: "gpt-4".into(),
            user: "user-1".into(),
            status_code: "200".into(),
            success: true,
            latency_secs: 0.5,
            upstream_latency_secs: 0.4,
            ttft_secs: Some(0.1),
            queue_time_secs: None,
            spend: 0.042,
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            error_type: "".into(),
            api_base: Some("https://api.openai.com".into()),
        });

        // Verify total_requests counter has the label
        let families = prometheus::gather();
        let req_family = families.iter().find(|f| f.get_name() == "aigw_req_total_requests").expect("total_requests family exists");
        let metrics = req_family.get_metric();
        assert!(!metrics.is_empty(), "total_requests should have at least one labeled metric");
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
