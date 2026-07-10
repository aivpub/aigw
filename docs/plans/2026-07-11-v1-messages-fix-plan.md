# /v1/messages 接口修复方案

**日期**: 2026-07-11
**Phase**: 14（Stages 40-43）
**优先级**: P0 — 最高优先级
**状态**: 待审核
**审计来源**: 7 个 bug/gap 的代码审计

---

## 问题总览

| # | 优先级 | 问题 | 根因 |
|---|--------|------|------|
| 1 | **CRITICAL** | 流式 SSE 格式未转换，透传 OpenAI 格式给 Anthropic 客户端 | 流水线直接转发 raw bytes，未调用 adapter 的 `openai_chunk_to_claude_stream()` |
| 2 | **CRITICAL** | proxy_models 上游配置被忽略，只用环境变量 | 自建了简化的 env var 查找逻辑，没有复用 chat.rs 的 `resolve_upstream_params()` |
| 3 | HIGH | 缺少 key 预算+模型权限校验 | 只验证了 token 有效性，未检查 spend/max_budget 和 models 白名单 |
| 4 | HIGH | SpendLog api_key 硬编码为 `"claude-message"` | 未保存 hash 后的 token |
| 5 | MEDIUM | 流式模式 token 计数始终为 0 | 未解析 SSE chunk 提取 usage；所有流式端点都有此限制 |
| 6 | MEDIUM | 未知 model 返回 404 而非 `invalid_request_error` | 直接用了 StatusCode::NOT_FOUND |
| 7 | LOW | user_id 未传入 SpendLog | 取了 token 后未保存 user_id 字段 |

---

## 修复策略

**总原则：最小改动，复用已有组件。** chat.rs 中的 `resolve_upstream_params()`、key 校验模式、SSE 透传逻辑都是已验证的正确实现。修复思路是把这些能力暴露给 v1_messages 复用，而非 fork 一份。

### 修复项

#### Fix-1: 复用 `resolve_upstream_params`（修复 Bug 2 + 部分 Bug 5）

**改动文件**：
- `crates/aigw-server/src/routes/chat.rs:64` — `async fn resolve_upstream_params` → `pub(crate) async fn resolve_upstream_params`

**改动文件**：
- `crates/aigw-server/src/routes/v1_messages.rs:195-226` — 替换整个 model lookup + env var 逻辑

**Before**（~30 行）:
```rust
// 4. Check if model exists in proxy_models (by model_name)
let models = state.db.list_models().await?;
let pm = models.iter().find(|m| m.model_name == model)...;
let input_cost = pm.model_info.get("input_cost_per_token")...;
// 5. Determine upstream URL from environment
let upstream_base_url = std::env::var("UPSTREAM_LLM_URL")...;
```

**After**（~5 行）:
```rust
// 4. Resolve upstream routing + pricing from proxy_models (with credential decrypt, env fallback)
let resolved = chat::resolve_upstream_params(&state, &model).await?;
```

**效果**：
- 支持 proxy_models 表中配置的 api_base/api_key
- 支持加密 litellm_params + credential references
- 保留 env var 回退（resolve_upstream_params 内部已有）
- 正确提取 input_cost_per_token / output_cost_per_token

---

#### Fix-2: 流式 SSE 格式转换（修复 Bug 1）

**改动文件**：`crates/aigw-server/src/routes/v1_messages.rs:282-371`（整个 streaming 分支）

**当前行为**：
```
Upstream → [OpenAI SSE raw bytes] → 直接透传 → Client（期望 Anthropic SSE）
结果：客户端解析失败
```

**修复后行为**：
```
Upstream → [OpenAI SSE bytes] → 解析SSE帧 → ChatCompletionChunk
    → DefaultAdapter::openai_chunk_to_claude_stream() → ClaudeStreamEvent
    → 格式化为 Anthropic SSE → Client
```

**实现要点**：

1. **SSE 帧解析器**：缓冲 upstream bytes，按 `\n\n` 分割帧，提取 `data:` 行 JSON
2. **状态跟踪**：
   - `content_block_started: bool` — 第一次遇到 content delta 时先发 `content_block_start`
   - `message_started: bool` — 第一次遇到 role delta 时发 `message_start`
3. **事件注入**：adapter 不生成的 `content_block_start`、`content_block_stop`、`message_stop` 由 handler 补充
4. **处理 `[DONE]`**：OpenAI 流结束标记，触发 `message_stop`
5. **Anthropic SSE 输出格式**：
   ```
   event: message_start
   data: {"type":"message_start","message":{...}}

   event: content_block_start
   data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

   event: content_block_delta
   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

   event: content_block_delta
   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}

   event: content_block_stop
   data: {"type":"content_block_stop","index":0}

   event: message_delta
   data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}

   event: message_stop
   data: {"type":"message_stop"}
   ```

**伪代码结构**：
```rust
// SSE parsing loop
let mut buffer = Vec::new();
let mut message_started = false;
let mut content_block_started = false;
let mut request_id = format!("msg_{}", Uuid::new_v4());

while let Some(chunk) = stream.next().await {
    buffer.extend_from_slice(&chunk);
    // Split on \n\n to get complete SSE frames
    while let Some(pos) = find_double_newline(&buffer) {
        let frame = buffer.drain(..pos + 2).collect::<Vec<_>>();
        let frame_str = String::from_utf8_lossy(&frame);
        
        // Parse data lines
        for line in frame_str.lines() {
            if line == "data: [DONE]" {
                // Send final events
                send_content_block_stop(&tx);
                send_message_stop(&tx);
                break;
            }
            if let Some(json_str) = line.strip_prefix("data: ") {
                if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(json_str) {
                    // Convert via adapter
                    if let Some(event) = DefaultAdapter::openai_chunk_to_claude_stream(&chunk) {
                        if event.event_type == "message_start" {
                            message_started = true;
                            // Inject request_id into message object
                            write_anthropic_sse(&tx, &event);
                        } else if event.event_type == "content_block_delta" {
                            if !content_block_started {
                                send_content_block_start(&tx);
                                content_block_started = true;
                            }
                            write_anthropic_sse(&tx, &event);
                        } else if event.event_type == "message_delta" {
                            send_content_block_stop(&tx);
                            write_anthropic_sse(&tx, &event);
                        }
                    }
                }
            }
        }
    }
}
```

**复杂度**：这是本次修复中工作量最大的部分，预估 2-3 小时。

---

#### Fix-3: 增加 key 预算 + 模型权限校验（修复 Bug 3）

**改动文件**：`crates/aigw-server/src/routes/v1_messages.rs:89-124`（auth 段）

**当前问题**：只验证 token 存在，不检查 budget 和 model permissions。

**修复**：在 key 查找成功后，添加：

```rust
// After successful key lookup:
let key = key.unwrap(); // we already checked is_none()

// 3a. Budget check
if let Some(max_budget) = key.max_budget_f64() {
    if key.spend >= max_budget {
        return Err(anthropic_error(
            StatusCode::TOO_MANY_REQUESTS,
            "budget_exceeded",
            "Budget exceeded for this API key",
        ));
    }
}

// 3b. Model permission check (skip for master key)
```

注意：`blocked` 和 `expires` 检查已经在 `db.get_key_by_token()` 内部完成，无需重复。

**模型权限检查**：需要从 chat.rs 提取 `resolve_key_model_list` 或者做一个简化版。考虑到复杂度，可以先做 budget check，模型权限作为后续优化。

---

#### Fix-4: 修复 SpendLog 字段（修复 Bug 4 + Bug 7）

**改动文件**：`crates/aigw-server/src/routes/v1_messages.rs`

**修复**：
1. 在 auth 阶段保存 `token_hash` 和 `user_id`
2. 替换 SpendLog 中的硬编码 `"claude-message"` → `token_hash`
3. 替换 `user: None` → `user: user_id.clone()`

涉及位置：
- 流式 SpendLog: lines 322, 338
- 非流式 SpendLog: lines 417, 442
- DailySpendLog: line 471（`entity_id` 用 `user_id`）

---

#### Fix-5: 未知 model 返回正确的错误格式（修复 Bug 6）

**改动文件**：`crates/aigw-server/src/routes/v1_messages.rs:204-209`

```rust
// Before:
StatusCode::NOT_FOUND, "not_found_error", ...

// After:
StatusCode::BAD_REQUEST, "invalid_request_error", ...
```

---

#### Fix-6: 流式 mode token 计数（修复 Bug 5）

**现状**：所有流式端点（chat.rs + v1_messages.rs）在流式模式下 token 计数都是 0。这不是 v1_messages 特有的 bug，而是架构限制。

**本次修复**：在修复 Fix-2 的 SSE 解析过程中，如果 OpenAI upstream 在最后一个 chunk 中返回了 `usage`（需请求时设置 `stream_options: {"include_usage": true}`），则提取 usage 写入 SpendLog。

**实现**：
1. 在构造 OpenAI 请求时添加 `stream_options: {"include_usage": true}`
2. 在 SSE 解析循环中捕获最后一个有 usage 的 chunk
3. 在流结束后用实际 token 数写 SpendLog

---

## 实施顺序

| 步骤 | 修复项 | 预估 | 依赖 |
|------|--------|------|------|
| 1 | Fix-5: 修改 model-not-found 错误码 | 5 min | 无 |
| 2 | Fix-3: 增加 key budget 校验 | 20 min | 无 |
| 3 | Fix-4: 修复 SpendLog api_key + user_id | 15 min | 无 |
| 4 | Fix-2: 流式 SSE 格式转换 | 2.5h | 无 |
| 5 | Fix-1: 复用 resolve_upstream_params | 20 min | chat.rs 改动 |
| 6 | Fix-6: 流式 token 计数 | 30 min | Fix-2 |
| 7 | 测试：单元 + 集成 + 手工验证 | 1.5h | 全部 |

**总预估**：~5.5 小时

---

## 测试计划

### 单元测试（新增）
- `test_streaming_sse_conversion` — 模拟 OpenAI SSE chunks，验证 Anthropic SSE 格式输出
- `test_sse_content_block_start_stop` — 验证 content_block_start/content_block_stop 在正确时机插入
- `test_sse_done_marker` — 验证 `[DONE]` 触发 message_stop
- `test_auth_budget_exceeded` — key spend >= max_budget → 429
- `test_model_not_found_error_type` — 验证返回 invalid_request_error

### 集成测试（手工 + curl）
```bash
# 非流式
curl -X POST http://localhost:3000/v1/messages \
  -H "x-api-key: sk-xxx" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet","max_tokens":100,"messages":[{"role":"user","content":"hi"}]}'

# 流式
curl -X POST http://localhost:3000/v1/messages \
  -H "x-api-key: sk-xxx" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet","max_tokens":100,"stream":true,"messages":[{"role":"user","content":"hi"}]}'
```

### BDD 场景
- 复用 `tests/features/` 中的 real_api 场景框架
- 新增 `v1_messages.feature` 覆盖非流式 + 流式场景

---

## 风险评估

| 风险 | 等级 | 缓解 |
|------|------|------|
| SSE 帧跨 chunk 边界解析错误 | 中 | 完善 buffer 逻辑，单元测试覆盖 |
| `resolve_upstream_params` 变 pub 影响 chat.rs module 封装 | 低 | 仅 `pub(crate)`，不暴露到 crate 外 |
| Anthropic SSE 格式细节遗漏 | 中 | 参考 Anthropic 官方文档，用真实 Claude SDK 测试 |

---

## 参考资料

- Anthropic Streaming Messages: https://docs.anthropic.com/en/api/messages-streaming
- 现有 adapter 实现: `crates/aigw-core/src/adapter.rs:261-319` (`openai_chunk_to_claude_stream`)
- 参考 chat.rs SSE: `crates/aigw-server/src/routes/chat.rs:773-907`
- 参考 upstream 解析: `crates/aigw-server/src/routes/chat.rs:64-267`
