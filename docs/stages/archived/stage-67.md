# Stage 67: Prometheus Metrics

**Phase**: 26 — 可观测性 (Observability)
**状态**: ✅ 完成
**完成日期**: 2026-07-16

---

## 目标

对齐 litellm `PrometheusLogger` 的 14 个核心指标，暴露 `GET /metrics` 端点。

1. **14 个 Prometheus metric**（Counter/Histogram/Gauge），命名空间可配置
2. **Handler 层注入** — 每个请求结束自动记录
3. **Grafana dashboard 模板** — `docs/grafana/aigw-dashboard.json`

---

## Part A — 命名空间与配置 (0.5h)

### 1.1 配置优先级

```
ENV PROMETHEUS_NAMESPACE > config.yaml general_settings.prometheus_namespace
  > config 表 general_settings.prometheus_namespace
  > 默认值 "aigw"
```

这是 aigw 既有的配置加载链：`CLI > ENV > config.yaml > config 表 > 默认值`。见 `main.rs:88-132`。

### 1.2 config.rs 扩展

`GeneralSettings` 新增：

```rust
#[serde(rename = "prometheus_namespace", skip_serializing_if = "Option::is_none")]
pub prometheus_namespace: Option<String>,
```

启动时：

```rust
let ns = std::env::var("PROMETHEUS_NAMESPACE").ok()
    .or_else(|| config.as_ref()
        .and_then(|c| c.general_settings.as_ref())
        .and_then(|g| g.prometheus_namespace.clone()))
    .or_else(|| db.get_config("general_settings").ok().flatten()
        .and_then(|v| serde_json::from_str::<Value>(&v).ok())
        .and_then(|v| v.get("prometheus_namespace").and_then(|n| n.as_str()).map(String::from)))
    .unwrap_or_else(|| "aigw".into());
```

### 1.3 示例输出

```prometheus
# TYPE aigw_proxy_total_requests counter
aigw_proxy_total_requests{model="gpt-4",user="admin",key="sk-abc",status_code="200"} 42

# TYPE aigw_request_total_latency_seconds histogram
aigw_request_total_latency_seconds{model="gpt-4",quantile="0.5"} 0.042
```

`prometheus::default_registry()` 全局 registry 在 `MetricsRecorder::init(namespace)` 时一次性创建。

---

## Part B — 14 个 Metric 定义 (1.5h)

### 2.1 命名与 labels

```
请求级 Counter:
  aigw_total_requests       labels=[model, user, status_code]
  aigw_failed_requests      labels=[model, user, error_type]

延迟 Histogram (seconds):
  aigw_request_latency_seconds        labels=[model, user]
  aigw_llm_api_latency_seconds        labels=[model, user]
  aigw_llm_api_ttft_seconds           labels=[model, user]
  aigw_request_queue_time_seconds     labels=[model, user]

用量/成本 Counter:
  aigw_spend_metric                   labels=[model, user]
  aigw_tokens_metric                  labels=[model, user, token_type]

Deployment Gauge/Counter:
  aigw_deployment_state               labels=[model, api_base]       // 0/1
  aigw_deployment_tpm_limit           labels=[model, api_base]
  aigw_deployment_rpm_limit           labels=[model, api_base]
  aigw_deployment_cooled_down         labels=[model, api_base]
  aigw_deployment_success_responses   labels=[model, api_base]
  aigw_deployment_failure_responses   labels=[model, api_base]
```

> 无 `key` label，避免千级 key 造成维度爆炸。`user` 维度即 user_id/team_id。未来若需要 key 维度可在 `prometheus_metrics_config` 中 opt-in 开启。

---

## Part C — Handler 注入 (1.5h)

### 3.1 `chat.rs` 注入点

请求结束后（两个路径：成功/失败），构建 `RequestSummary` → `state.metrics.record(summary)`。

### 3.2 `v1_messages.rs` 注入点

同上。两个 handler 共享同一个 `MetricsRecorder` 实例在 `AppState` 中。

### 3.3 `AppState` 扩展

```rust
pub struct AppState {
    // … existing fields …
    pub metrics: Option<Arc<MetricsRecorder>>,  // None if not initialized
}
```

---

## Part D — `GET /metrics` (1h)

### 4.1 端点

```rust
// routes/metrics.rs
pub async fn metrics() -> Response {
    let encoder = prometheus::TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    Response::builder()
        .header("Content-Type", "text/plain; version=0.0.4")
        .body(Body::from(buffer))
        .unwrap()
}
```

### 4.2 main.rs 注册

```rust
.route("/metrics", get(metrics::metrics))
```

无需 auth（metrics 不暴露敏感数据，labels 已 hash 脱敏）。

---

## Part E — Grafana Dashboard 模板 (1h)

`docs/grafana/aigw-dashboard.json`（可 import 的 JSON dashboard）：

| Panel | Metrics | 可视化 |
|-------|---------|--------|
| Overall Throughput | `rate(aigw_proxy_total_requests[5m])` | Stat |
| Error Rate | `rate(aigw_proxy_failed_requests[5m]) / rate(aigw_proxy_total_requests[5m])` | Gauge |
| P50/P95 Total Latency | `histogram_quantile(0.5, aigw_request_total_latency_seconds)` | Time series |
| P50 Upstream Latency | `histogram_quantile(0.5, aigw_llm_api_latency_seconds)` | Time series |
| TTFT | `histogram_quantile(0.5, aigw_llm_api_time_to_first_token_seconds)` | Time series |
| Spend Rate | `rate(aigw_spend_metric[5m])` | Time series |
| Token Throughput | `rate(aigw_tokens_metric[5m])` | Time series (stacked by token_type) |
| Deployment Health | `aigw_deployment_state` + `aigw_deployment_cooled_down` | Table |

---

## Part F — 测试 (0.5h)

| 类型 | # | 场景 |
|------|---|------|
| UT | 1 | `MetricsRecorder::init` — 14 个 metric 注册 + namespace 验证 |
| UT | 2 | Counter inc → `/metrics` 端点输出验证 |
| UT | 3 | Histogram observe → `/metrics` bucket + sum + count |
| UT | 4 | 请求成功 → 所有 label 正确填充 |
| UT | 5 | 请求失败 → `failed_requests` inc + error_type label |
| UT | 6 | 默认 namespace = `aigw` |

---

## 门禁

- [ ] `cargo test` 全量通过（206 → 212 UT）
- [ ] `curl localhost:4000/metrics` 输出 Prometheus 文本格式
- [ ] 输出包含 `aigw_proxy_total_requests{...}` 等带正确 namespace 的指标
- [ ] `npm run build` 前端无回归
