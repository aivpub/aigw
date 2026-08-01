# Stage 62: select_adapter 扩展 + Handler 对接 + 全量回归

**Phase**: 22 — Anthropic 原生上游适配
**状态**: ⏳ 待开始
**预估**: 6h
**依赖**: Stage 61

---

## 目标

将 Stage 61 实现的两个 adapter 接入请求链路，通过 BDD 回归验证两条新路径的端到端正确性。

---

## 变更范围

| 文件 | 变更 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | `select_adapter` 加两个 arm |
| `crates/aigw-server/src/routes/v1_messages.rs` | handler 确认 `provider_type` dispatch（无需改动，已正确） |
| `crates/aigw-server/src/routes/chat.rs` | 同上 |
| `crates/aigw-server/tests/bdd_support/mock_upstream.rs` | 扩展 mock 支持 Anthropic Messages API 原生格式 |
| `crates/aigw-server/tests/features/` | 新增 4 BDD scenarios |
| `crates/aigw-server/tests/bdd_steps/` | 新增对应 step definitions |

---

## 实现要点

### 1. select_adapter 扩展

```rust
pub fn select_adapter(client: ClientProtocol, provider: &ProviderType)
    -> Option<&'static dyn MessageAdapter>
{
    match (client, provider) {
        (ClientProtocol::OpenAI, ProviderType::OpenAICompatible) => Some(&OpenAIPassthrough),
        (ClientProtocol::Anthropic, ProviderType::OpenAICompatible) => Some(&AnthropicToOpenAI),
        (ClientProtocol::Anthropic, ProviderType::AnthropicNative) => Some(&AnthropicPassthrough),  // NEW
        (ClientProtocol::OpenAI, ProviderType::AnthropicNative) => Some(&OpenAIToAnthropic),      // NEW
        _ => None,
    }
}
```

### 2. Handler 确认

`v1_messages.rs`：
- `resolved_deployment.provider_type` 由 `ModelResolver` 返回（已就绪）
- `select_adapter(ClientProtocol::Anthropic, &resolved_deployment.provider_type)` 已在使用
- **无需代码变更**——Stage 61 完成后 `None` 不再发生

`chat.rs`：
- `deployment.provider_type` 已就绪
- `select_adapter(ClientProtocol::OpenAI, &deployment.provider_type)` 已在使用
- **无需代码变更**

### 3. MockUpstream 扩展

现有 `mock_upstream.rs` 只提供 OpenAI `/v1/chat/completions` 端点。需新增 Anthropic 原生端点：

```rust
// 新增路由
.route("/v1/messages", post(anthropic_messages_handler))

async fn anthropic_messages_handler(
    State(state): State<Arc<MockState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    // 1. 记录请求（与现有 chat_completions_handler 相同模式）
    // 2. 从 state.mock_response 读取预设响应
    // 3. 返回 Anthropic Messages API 格式:
    //    { "id": "msg_xxx", "type": "message", "role": "assistant",
    //      "content": [...], "model": "claude-sonnet-4-20250514",
    //      "stop_reason": "end_turn", "usage": {...} }
}

// 新增 streaming 端点
async fn anthropic_messages_stream_handler(
    ...
) -> Sse<...> {
    // SSE event: "message_start" / "content_block_start" / "content_block_delta" /
    //            "content_block_stop" / "message_delta" / "message_stop"
}
```

---

## BDD 新增（4 scenarios）

| # | 路径 | 验证点 |
|---|------|--------|
| 1 | Anthropic Client → `POST /v1/messages` → MockUpstream (Anthropic) | body 直通不变；x-api-key + anthropic-version header 注入 |
| 2 | OpenAI Client → `POST /v1/chat/completions` → MockUpstream (Anthropic) | request → Anthropic Messages 格式正确；response → OpenAI Chat Completions 格式正确 |
| 3 | Streaming Anthropic Client → SSE → MockUpstream (Anthropic) | SSE 事件逐帧透传 |
| 4 | Streaming OpenAI Client → SSE → MockUpstream (Anthropic) | OpenAI chunk → Anthropic event 转换正确 |

---

## 门禁

- [ ] `cargo test` 全量通过（UTR + 10 new adapter tests）
- [ ] BDD `cargo test --test bdd`：93 → 97 scenarios 全部通过
- [ ] 前端 BDD 111 tests 回归通过
