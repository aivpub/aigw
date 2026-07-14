# Stage 52: Handler 瘦身

**Phase**: 17 — 代理转发架构重构（P1）
**状态**: ⏳ 待开始
**预估**: 3h
**依赖**: Stage 51（MessageAdapter + tool 转换）

---

## 目标

将 chat.rs 和 v1_messages.rs 中的通用逻辑（校验→resolve→adapt→upstream call→spend log）瘦身为薄层 handler，消除文件间的结构重复。

1. `chat.rs` — handler 只做编排，不再包含 resolve_upstream 和 adapter 细节
2. `v1_messages.rs` — 对齐 chat.rs 的编排模式，消除与 chat.rs 之间的结构重复
3. 清理死代码和注释

## 验收标准

- [ ] `chat.rs` 中 `chat_completions()` 简化为：校验 → resolve → select_adapter → adapt_request → upstream call → adapt_response/stream → spend log
- [ ] `v1_messages.rs` 中 `messages_handler()` 对齐相同编排模式
- [ ] 两文件不再包含：直接查 proxy_models 表的逻辑、直接调 DefaultAdapter 的代码
- [ ] 清理 `#[allow(dead_code)]` 标记的 `provider_registry`、`router_state`（如已接入则使用，否则删除标记并记录技术债）
- [ ] **TDD**:
  - BDD: chat_completions 端点行为对比（重构前后请求/响应一致）
  - BDD: /v1/messages 端点行为对比（重构前后请求/响应一致，含 tool_use 场景）
- [ ] **门禁**: 全量 UT (316+) + BDD (92+ scenarios) + 前端 Playwright (108 tests) 全部通过

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs` | **瘦身** — 移除 `resolve_upstream_params()`，handler 改为编排模式 |
| `crates/aigw-server/src/routes/v1_messages.rs` | **瘦身** — 对齐 chat.rs 编排模式，移除直接 adapter 调用 |
| `crates/aigw-server/src/routes/keys.rs` | 修改 — 清理 dead_code 标记（如已使用） |

## 技术方案

### A. Handler 编排模式

两个 handler 统一为 6 步：

```
 1. validate: body["model"], body["messages"] 校验
 2. auth:     key 权限检查、model allowlist、budget 检查
 3. resolve:  state.resolver.resolve(model) → Vec<Deployment>
 4. adapt:    select_adapter(client_protocol, deployment.provider_type)
              → adapter.adapt_request(body, deployment)
 5. upstream: 拼 URL、设 Auth header、reqwest send
 6. spend:    记录 SpendLog + daily_spend queue
```

### B. chat.rs 改造前后对比

```rust
// 改造后 chat_completions 核心流程
pub async fn chat_completions(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    Json(body): Json<Value>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // 1. validate
    let model = validate_model(&body)?;
    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    validate_messages(&body)?;

    // 2. auth
    if !auth.is_master_key {
        check_model_permission(&state, &auth, model).await?;
        check_budget(&state, &auth).await?;
    }

    // 3. resolve
    let deployments = state.resolver.resolve(model).await?;
    let deployment = &deployments[0];

    // 4. adapt
    let adapter = select_adapter(ClientProtocol::OpenAI, &deployment.provider_type)
        .ok_or_else(|| unsupported_protocol_error())?;
    let upstream_body = adapter.adapt_request(body, deployment)?;

    // 5. upstream call + 6. spend log
    if is_stream {
        stream_upstream(&state, &deployment, upstream_body, adapter, &auth).await
    } else {
        non_stream_upstream(&state, &deployment, upstream_body, adapter, &auth).await
    }
}
```

### C. 提取公共 upstream 调用

```rust
/// 非流式 upstream 调用（chat.rs + v1_messages.rs 共享）
async fn non_stream_upstream(
    state: &SharedState,
    deployment: &Deployment,
    body: Value,
    adapter: &dyn MessageAdapter,
    auth: &ChatAuth,
    endpoint: &str,  // "chat/completions" | "messages"
) -> Result<Response, (StatusCode, Json<Value>)> {
    let url = format!("{}/{}", deployment.api_base.trim_end_matches('/'), endpoint);
    let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let mut req = client.post(&url).json(&body);
    if let Some(ref key) = deployment.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }

    let start = Utc::now();
    let resp = req.send().await?;
    let status = resp.status();
    let resp_body: Value = resp.json().await?;

    if status.is_success() {
        let adapted = adapter.adapt_response(resp_body.clone())?;
        record_spend(state, deployment, auth, start, &body, &resp_body, true).await;
        Ok(Json(adapted).into_response())
    } else {
        record_spend(state, deployment, auth, start, &body, &resp_body, false).await;
        Err((status, Json(resp_body)))
    }
}
```

### D. v1_messages.rs 对齐

与 chat.rs 完全相同编排，差异仅在于：
- `ClientProtocol::Anthropic`（而非 `OpenAI`）
- upstream URL 为 `/v1/messages`（而非 `/chat/completions`）
- 流式转换走 `AnthropicToOpenAI` 的 `StreamAdapter`（Stage 51 已实现）

## 风险

- 提取公共函数时容易改动行为细节，BDD 端到端回归是关键安全网
- `v1_messages.rs` 流式转换的跨 chunk 状态（Stage 51 新增）需在瘦身后完整保留，不能因代码移动破坏
- 前端 108 个 Playwright 测试也需通过 — 确保 API 返回格式不被重构破坏
