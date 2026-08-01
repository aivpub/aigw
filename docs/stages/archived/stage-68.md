# Stage 68: OTEL Traces 链路追踪

**Phase**: 26 — 可观测性 (Observability)
**状态**: ⏳ 待开始
**预估**: 6h
**依赖**: Stage 67

---

## 目标

实现 OpenTelemetry tracing，覆盖 aigw 请求全链路 span 上报。

1. **traceparent 提取与注入** — W3C Trace Context 在上下游传播
2. **5 层 span** — request_received → auth → adapter → upstream → response_sent
3. **OTEL exporter 配置化** — config 文件控制是否启用 + 导出目标

---

## Part A — OTEL 中间件 (2h)

### 1.1 `crates/aigw-core/src/tracing.rs`

```rust
use opentelemetry::trace::{Tracer, Span, SpanKind, Status};
use opentelemetry::Context;
use opentelemetry_sdk::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;

pub struct OtelConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,    // e.g. http://jaeger:4317
    pub service_name: String,        // default "aigw"
    pub sample_rate: f64,            // default 1.0
}
```

### 1.2 Cargo.toml 新增依赖

```toml
opentelemetry = { version = "0.24", features = ["trace"] }
opentelemetry_sdk = { version = "0.24", features = ["trace", "rt-tokio"] }
opentelemetry_otlp = { version = "0.17", features = ["grpc-tonic", "http-proto"] }
opentelemetry-semantic-conventions = "0.16"
tracing-opentelemetry = "0.25"
```

### 1.3 tower Layer（`middleware/tracing_layer.rs`）

```rust
pub async fn otel_trace_layer(
    req: Request<Body>,
    next: Next,
) -> Response {
    // 1. 从请求 header 提取 traceparent
    let parent_cx = global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderExtractor(req.headers()))
    });

    // 2. 创建 aigw span
    let mut span = tracer
        .span_builder("aigw_request")
        .with_kind(SpanKind::Server)
        .start_with_context(&tracer, &parent_cx);

    span.set_attribute("http.method", req.method().to_string());
    span.set_attribute("http.url", req.uri().to_string());

    // 3. 注入 traceparent 到下游请求 headers
    let mut downstream_headers = HeaderMap::new();
    global::get_text_map_propagator(|prop| {
        prop.inject_context(&parent_cx, &mut HeaderInjector(&mut downstream_headers))
    });

    let response = next.run(req).await;

    span.set_attribute("http.status_code", response.status().as_u16() as i64);
    span.set_status(if response.status().is_success() { Status::Ok } else { Status::Error });
    span.end();

    response
}
```

---

## Part B — 5 层子 Span (2h)

使用 `tracing-opentelemetry` 桥接，将 `tracing::span!` 转为 OTEL span：

```
aigw_request (root)
 └── auth_check
      └── resolve_deployment
           └── adapter_adapt
                └── upstream_call
                     ├── upstream_latency_ms = ...
                     ├── upstream_status = 200
                     └── response_sent
                          ├── prompt_tokens = 5100
                          ├── completion_tokens = 3100
                          └── spend = 0.042
```

每个 span 自动继承 parent trace_id，不需要手动传递 context。

### Handler 注入

`chat.rs` 和 `v1_messages.rs` 中用 `instrument` 宏包裹关键函数：

```rust
#[tracing::instrument(skip(state, body), fields(model = %_model))]
async fn handle_chat_request_inner(...) { ... }
```

`upstream_call` 中注入 `tracing::Span` 记录上游延迟和状态码。

---

## Part C — 配置 (1h)

### 3.1 config.yaml 扩展

```yaml
general_settings:
  otel:
    enabled: false
    endpoint: "http://jaeger:4317"
    service_name: "aigw"
    sample_rate: 1.0
    exporter: "otlp_grpc"  # or "otlp_http"
```

### 3.2 运行时配置读取

启动时从 `config` 表 `general_settings` 读 `otel` 字段 → 初始化 `TracerProvider`。如果 `enabled: false`，所有 span 是 noop（零开销）。

---

## Part D — traceparent 上下游传播 (1h)

### 4.1 下游注入

构建 upstream request 时，从当前 active span context 提取 traceparent 写入 header：

```rust
// chat.rs — 发送上游请求前
if otel_config.enabled {
    let injector = HeaderInjector(&mut upstream_headers);
    opentelemetry::global::get_text_map_propagator(|prop| {
        prop.inject(&Context::current(), &mut injector);
    });
    upstream_req = upstream_req.header("traceparent", ...);
}
```

### 4.2 上游提取

tower layer 在请求入口自动提取（Part A 1.3），不依赖 OTEL 是否启用。

---

## 测试

| 类型 | # | 场景 |
|------|---|------|
| UT | 1 | traceparent 从 header 提取成功 |
| UT | 2 | traceparent 注入到 upstream request header |
| UT | 3 | 5 层 span 创建 + parent 关系校验 |
| UT | 4 | `otel.enabled: false` 时无 panic、无 span 数据 |

---

## 门禁

- [ ] `cargo test` 全量通过（217 → 221 UT）
- [ ] OTEL 禁用时零性能影响（bench 验证）
