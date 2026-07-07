# Stage 18: 结构化日志

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 17 完成
**预估**: 2-3h

---

## 1. 目标

统一日志格式为 JSON，注入 `request_id` 实现请求链路追踪，通过环境变量控制日志级别。

---

## 2. 交付

### 2.1 技术选型

使用 `tracing` + `tracing-subscriber` 生态：
- `tracing` — 结构化 span/event 插桩
- `tracing-subscriber` — JSON 格式化输出
- `tracing-axum` / `tower-http` — HTTP 层自动 trace

### 2.2 JSON 日志格式

```json
{
  "timestamp": "2026-07-08T10:30:00.123Z",
  "level": "INFO",
  "request_id": "01JXYZ...",
  "target": "aigw_server::routes::chat",
  "message": "chat completion",
  "model": "gpt-4",
  "latency_ms": 234
}
```

### 2.3 实现要点

- 所有 HTTP 请求自动注入 `request_id`（UUID v7，在 middleware 层生成）
- `AIGW_LOG_LEVEL` 环境变量控制级别（默认 `info`）
- 覆盖：HTTP 请求/响应、DB 查询错误、upstream 调用、auth 鉴权
- `tracing::instrument` 宏用于关键函数

### 2.4 变更范围

- `aigw-server/Cargo.toml` — 添加 `tracing`, `tracing-subscriber`, `tower-http`
- `aigw-server/src/main.rs` — 初始化 tracing subscriber
- 新增 middleware 注入 request_id
- 关键 handler 添加 `#[instrument]` 注解

---

## 3. 门禁

- `AIGW_LOG_LEVEL=debug` 下所有请求输出 JSON 日志
- 同一请求的 request_id 在所有日志行中一致
- 不同请求的 request_id 不同
- 正常请求不输出 error 级别日志
