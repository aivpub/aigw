# Stage 41: `/v1/messages` 流式 SSE 格式转换（OpenAI → Anthropic）

**Phase**: 14 — `/v1/messages` 接口修复
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 2.5h
**依赖**: Stage 40

---

## 目标

将 `/v1/messages` streaming 路径从"透传 OpenAI SSE raw bytes"改为"解析 OpenAI SSE → 转换为 Anthropic SSE 格式 → 发给客户端"。

## 验收标准

- [ ] 流式模式下，客户端收到的是 Anthropic SSE 格式事件（`message_start` / `content_block_start` / `content_block_delta` / `content_block_stop` / `message_delta` / `message_stop`）
- [ ] 内容按正确的顺序注入（`message_start` → `content_block_start` → `content_block_delta`×N → `content_block_stop` → `message_delta` → `message_stop`）
- [ ] handle OpenAI `[DONE]` 标记，触发 `message_stop`
- [ ] SSE 帧跨 chunk 边界正确解析（buffer + 按 `\n\n` 分割）
- [ ] 非流式路径行为不变
- [ ] 单元测试：模拟 OpenAI SSE chunks → 验证 Anthropic SSE 格式输出

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/v1_messages.rs:282-371` | 重写：SSE streaming 分支 |

## 技术方案

### Anthropic SSE 格式

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_xxx","type":"message","role":"assistant","content":[],"model":"claude-sonnet","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}

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

### 核心实现：SSE 帧解析器 + 状态机

```rust
// SSE buffer — accumulates bytes, splits on \n\n to get complete frames
let mut buffer = Vec::new();
let mut message_started = false;
let mut content_block_started = false;
let message_id = format!("msg_{}", uuid::Uuid::new_v4());
let model_name = resolved.model_name.clone();

while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(data) => {
            buffer.extend_from_slice(&data);
            // Extract complete SSE frames
            while let Some(pos) = buffer.windows(2).position(|w| w == b"\n\n") {
                let frame = buffer.drain(..pos + 2).collect::<Vec<_>>();
                let frame_str = String::from_utf8_lossy(&frame);
                
                for line in frame_str.lines() {
                    if line == "data: [DONE]" {
                        // Send final events
                        if content_block_started {
                            write_sse(&tx, "content_block_stop", &json!({
                                "type": "content_block_stop",
                                "index": 0
                            }));
                        }
                        write_sse(&tx, "message_delta", &json!({
                            "type": "message_delta",
                            "delta": {"stop_reason": "end_turn"},
                            "usage": {"output_tokens": 0}
                        }));
                        write_sse(&tx, "message_stop", &json!({
                            "type": "message_stop"
                        }));
                        break;
                    }
                    if let Some(json_str) = line.strip_prefix("data: ") {
                        if let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(json_str) {
                            // Convert via adapter
                            if let Some(event) = DefaultAdapter::openai_chunk_to_claude_stream(&chunk) {
                                // Track TTFT on first chunk
                                if first_chunk_time.is_none() {
                                    first_chunk_time = Some(chrono::Utc::now());
                                }
                                
                                match event.event_type.as_str() {
                                    "message_start" => {
                                        // Inject message_id and model
                                        if let Some(ref mut msg) = event.message.as_mut() {
                                            msg.id = message_id.clone();
                                            msg.model = model_name.clone();
                                        }
                                        write_sse(&tx, "message_start", &event);
                                        message_started = true;
                                    }
                                    "content_block_delta" => {
                                        if !content_block_started {
                                            write_sse(&tx, "content_block_start", &json!({
                                                "type": "content_block_start",
                                                "index": 0,
                                                "content_block": {"type": "text", "text": ""}
                                            }));
                                            content_block_started = true;
                                        }
                                        write_sse(&tx, "content_block_delta", &event);
                                    }
                                    "message_delta" => {
                                        if content_block_started {
                                            write_sse(&tx, "content_block_stop", &json!({
                                                "type": "content_block_stop",
                                                "index": 0
                                            }));
                                        }
                                        write_sse(&tx, "message_delta", &event);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(_) => break,
    }
}
```

### SSE 写入辅助函数

```rust
fn write_sse(tx: &UnboundedSender<Vec<u8>>, event_type: &str, data: &impl Serialize) {
    let json = serde_json::to_string(data).unwrap_or_default();
    let sse = format!("event: {}\ndata: {}\n\n", event_type, json);
    let _ = tx.send(sse.into_bytes());
}
```

### OpenAI upstream 行为假设

OpenAI SSE 会按以下顺序发送 chunk：
1. `{"choices":[{"delta":{"role":"assistant"},"index":0}]}` — role delta → adapter 生成 `message_start`
2. `{"choices":[{"delta":{"content":"Hello"},"index":0}]}` — content delta → adapter 生成 `content_block_delta`
3. …更多 content delta…
4. `{"choices":[{"delta":{},"finish_reason":"stop","index":0}]}` — finish → adapter 生成 `message_delta`
5. `data: [DONE]` — 流结束标记

## 风险

- SSE 帧跨 chunk 边界：已通过 buffer + `\n\n` 分割处理
- OpenAI `[DONE]` 前可能没有 finish_reason chunk：需要 handler 层补充 `end_turn`
- Anthropic 客户端对 `content_block_start` 的 `content_block` 结构有严格校验：必须包含 `type: "text"` 字段
