# Stage 69: 后端质量修复 + Usage 数据增强

**Phase**: 27 — 全栈质量修复 + Usage 页面图表增强
**状态**: ✅ 完成
**预估**: 8h
**完成日期**: 2026-07-22

---

## 目标

一次性修复 4 个后端问题 + 1 个数据增强，每个变更独立可测，最后统一回归。

| # | 问题 | 当前状态 | 目标 |
|---|------|---------|------|
| 1 | model_group 语义错误 | `litellm_params.model`（上游模型名） | `proxy_models.model_name`（部署名称），对齐 litellm |
| 2 | 无 HTTP 层重试 | RouterConfig.num_retries 存而未用 | reqwest-middleware + reqwest-retry，5xx/网络错误自动重试 |
| 3 | 手动解析 x-forwarded-for | chat.rs/v1_messages.rs 各写一遍 | axum-client-ip 中间件统一提取 |
| 4 | Daily 趋势无分解 | daily 返回 4 字段（spend/tokens/requests） | 返回 8 字段（含 prompt/completion/success/failed） |
| 5 | 无 Top Keys 排行 | 无端点 | GET /global/spend/keys/rankings |

---

## Part A — model_group 语义修复 (1.5h)

**当前错误** (`chat.rs:142-145`):
```rust
let model_group = params_json.get("model")...  // → "gpt-4" (upstream model)
```

**修正后**:
```rust
let model_group = Some(m.model_name.clone());  // → "my-gpt4-deploy" (deployment name)
```

### 修改点

| 文件 | 行 | 变更 |
|------|-----|------|
| `crates/aigw-core/src/deployment.rs` | 34 | 注释: `litellm_params.model` → `proxy_models.model_name` |
| `crates/aigw-core/src/resolver.rs` | 148-152 | model_group = `m.model_name.clone()` |
| `crates/aigw-server/src/routes/chat.rs` | 142-145 | 同上 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 搜索所有点 | 同上 |

### TDD (3 UT)

1. resolver.rs: Deployment.model_group = model_name（非 litellm_params.model）
2. chat.rs: SpendLog 写入的 model_group = model_name
3. v1_messages.rs: 同上

---

## Part B — reqwest-retry 重试中间件 (3h)

### 依赖

**`crates/aigw-server/Cargo.toml`**:
```toml
reqwest-middleware = "0.3"
reqwest-retry = "0.6"
```

### Router 新增方法

**`crates/aigw-core/src/router.rs`**:
```rust
pub fn build_retry_client(&self, num_retries: u32) -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder()
        .build_with_max_retries(num_retries);
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("reqwest client build");
    ClientBuilder::new(client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
```

策略: 5xx/网络超时重试，4xx 不重试。retry_count 存入 `metadata.retry_count`。

### 重试日志

每次重试时通过 `tracing` 打印日志：

```
WARN upstream_retry{attempt=1, max_retries=3, url="https://api.openai.com/...", status=503}
WARN upstream_retry{attempt=2, max_retries=3, url="https://api.openai.com/...", status=503}
INFO upstream_success_after_retry{retry_count=2, total_attempts=3, url="https://api.openai.com/..."}
```

**实现方式**: `reqwest-retry` 的 `RetryTransientMiddleware` 支持自定义 log 回调，配置 `RetryableStrategy` 时注入 `tracing`:

```rust
use reqwest_retry::policies::ExponentialBackoff;

let backoff = ExponentialBackoff::builder().build_with_max_retries(num_retries);

RetryTransientMiddleware::new_with_policy_and_strategy(
    backoff,
    CustomRetryStrategy, // 实现 RetryableStrategy，在 execute 时 tracing::warn!
)
```

或在 `RetryableStrategy::handle()` 中每次重试前打印:

```rust
tracing::warn!(
    attempt = retry_count + 1,
    max_retries = num_retries,
    url = %request.url(),
    "upstream retry"
);
```

### Handler 接入

**`chat.rs`** + **`v1_messages.rs`**: 上游请求改用 `retry_client`，从 response extensions 提取 retry count 写入 metadata。

### TDD (2 UT)

1. mock 上游 503×2 → 200，验证 metadata.retry_count=2
2. 4xx 不重试

---

## Part C — axum-client-ip 中间件 (1.5h)

### 依赖

**`crates/aigw-server/Cargo.toml`**:
```toml
axum-client-ip = "0.5"
```

### main.rs 注册

```rust
use axum_client_ip::UseClientIpLayer;
// Router 添加
.layer(UseClientIpLayer::default())
```

### Handler 替换

**`chat.rs` (行 842-846)** + **`v1_messages.rs` (行 286)**:

```rust
// 修改前: 手动 headers.get("x-forwarded-for") 解析
// 修改后:
use axum_client_ip::ClientIp;
// handler 签名加: Extension(client_ip): Extension<ClientIp>
let requester_ip = Some(client_ip.0.to_string());
```

### TDD (1 UT)

验证中间件提取 IP 正确写入 spend_logs

---

## Part D — Daily 趋势数据分解 (1h)

### DB 查询扩展

**`crates/aigw-core/src/db.rs`** — `query_activity_daily()` 返回值:
`Vec<(String, f64, i64, i64)>` → `Vec<(String, f64, i64, i64, i64, i64, i64)>`
= `(date, spend, tokens, requests, prompt_tokens, completion_tokens, ok_requests, failed_requests)`

SQL (三臂: SQLite/MySQL/PG):
```sql
SELECT DATE(start_time),
       COALESCE(SUM(spend), 0),
       COALESCE(SUM(total_tokens), 0),
       COUNT(request_id),
       COALESCE(SUM(prompt_tokens), 0),
       COALESCE(SUM(completion_tokens), 0),
       COUNT(CASE WHEN status = 'success' THEN 1 END),
       COUNT(CASE WHEN status LIKE 'failure%' THEN 1 END)
FROM spend_logs WHERE ...
GROUP BY 1 ORDER BY 1 ASC
```

### DailyRow 扩展

**`crates/aigw-server/src/routes/spend.rs`**: 新增 `prompt_tokens`, `completion_tokens`, `successful_requests`, `failed_requests` 字段

### TDD (1 UT)

验证 8 元组返回正确

---

## Part E — Top Virtual Keys 聚合端点 (1h)

### 新增结构体

**`crates/aigw-core/src/models.rs`**:
```rust
pub struct SpendKeyRanking {
    pub api_key: String,
    pub key_alias: Option<String>,
    pub total_spend: f64,
    pub total_requests: i64,
    pub total_tokens: i64,
}
```

### DB 方法

**`crates/aigw-core/src/db.rs`** — `aggregate_spend_by_keys(start_date, end_date, limit)`:
```sql
SELECT sl.api_key, vk.key_alias,
       COALESCE(SUM(sl.spend), 0), COUNT(sl.request_id),
       COALESCE(SUM(sl.total_tokens), 0)
FROM spend_logs sl LEFT JOIN virtual_keys vk ON sl.api_key = vk.token
WHERE {date_filter}
GROUP BY sl.api_key ORDER BY total_spend DESC LIMIT ?
```

### 端点

**`crates/aigw-server/src/routes/spend.rs`**: `global_spend_keys_rankings`
**`crates/aigw-server/src/main.rs`**: 路由注册

### TDD (2 UT + 2 BDD)

UT: 排序验证、limit 截断
BDD: admin 200、非 admin 403

---

## 测试汇总

| 层级 | # | 场景 |
|------|---|------|
| UT | 3 | model_group 修正 (resolver + chat + v1_messages) |
| UT | 2 | reqwest-retry (503→重试→200, 4xx 不重试) |
| UT | 1 | axum-client-ip IP 写入 |
| UT | 1 | query_activity_daily 8 元组 |
| UT | 2 | aggregate_spend_by_keys (排序 + limit) |
| BDD | 2 | GET /global/spend/keys/rankings (200 + 403) |

---

## 门禁

- [x] `cargo check --workspace` 编译通过
- [x] `cargo test --workspace` 全量通过 (322 pass)
- [x] `cargo test --test bdd` 99/101 scenarios passed (101 scenarios, 99 passed, 2 skipped)
- [x] model_group 写入 = proxy_models.model_name
- [x] reqwest-retry HTTP 层重试已接入（build_retry_client 已用于 chat.rs + v1_messages.rs）
- [x] axum-client-ip 提取器已接入（OptionalClientIp 包装 RightmostXForwardedFor）
- [x] `/global/spend/activity` DailyRow 返回 8 字段
- [x] `/global/spend/keys/rankings` 按 spend DESC 排序

## 实现说明

### Part C — IP 提取器实际方案
- 未使用 `UseClientIpLayer` 中间件（与规划不同）
- 采用 `OptionalClientIp` 提取器模式：包装 `RightmostXForwardedFor`，缺失时返回 None 而非报错
- 在 chat.rs 和 v1_messages.rs 的 handler 签名中声明，而非在 main.rs 注册 layer
- 功能等价，且更 Rust-idiomatic（不会因缺失 header 而拒绝请求）

### Part B — 重试日志
- 使用 `reqwest-retry` 内置的 `RetryTransientMiddleware`，重试行为由库内置日志输出
- 未自定义 `RetryableStrategy`（库默认已处理 5xx/网络错误重试，4xx 不重试）
