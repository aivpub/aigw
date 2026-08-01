# Stage 59: Multi tool_result Discard Fix

**Phase**: 21 — 协议兼容性修复
**状态**: ⏳ 待开始
**预估**: 4h
**依赖**: 无

---

## 目标

修复 `claude_message_to_openai` 中多个 `tool_result` block 仅取 `tool_results[0]` 的 bug，改为迭代全部生成多条 `role="tool"` 消息。

---

## 根因

`crates/aigw-core/src/adapter.rs:622-624`:

```rust
let (tool_use_id, content) = &tool_results[0];  // ← 只取第一个
ChatMessage { role: "tool".to_string(), content: ChatContent::Text(content.clone()), ... }
```

Claude Code 并行调用多个工具时（如 `Bash` + `Read`），assistant 返回多个 `tool_use` block，user 在下一轮提交多个 `tool_result` block（每个对应一个 `tool_use`），但 aigw 只把第一个 `tool_result` 转为 OpenAI `role="tool"` 消息，其余静默丢弃。

来源：ADR-016 附带发现（`docs/08-autonomous-decisions.md`）。

---

## 变更范围

| 文件 | 变更 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | `claude_message_to_openai` 改为返回 `Vec<ChatMessage>`；tool_result 处理从 `[0]` 改为迭代全部；调用方 `claude_to_openai_request` 改为 `extend` |

---

## 实现要点

### 1. 函数签名变更

```rust
// Before
fn claude_message_to_openai(msg: &ClaudeMessage) -> ChatMessage

// After
fn claude_message_to_openai(msg: &ClaudeMessage) -> Vec<ChatMessage>
```

### 2. tool_result 迭代逻辑

```rust
// 收集 tool_results
let tool_results: Vec<(String, String)> = msg.content.iter()
    .filter_map(|b| {
        let tui = b.tool_use_id.clone()?;
        let c = b.content.as_ref()
            .and_then(|v| v.as_str().map(String::from))
            .or_else(|| b.text.clone())
            .unwrap_or_default();
        Some((tui, c))
    }).collect();

if !tool_results.is_empty() && msg.role == "user" {
    let mut out = Vec::new();

    // 非 tool_result 的 content parts（text/image）先发 user 消息
    let non_tool_parts: Vec<_> = msg.content.iter()
        .filter(|p| p.tool_use_id.is_none() || p.type_name() != "tool_result")
        .cloned()
        .collect();
    if !non_tool_parts.is_empty() {
        out.push(ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Parts(non_tool_parts),
            name: msg.name.clone(),  // preserve name if set
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // 每个 tool_result 发一条 tool 消息
    for (tool_use_id, content) in &tool_results {
        out.push(ChatMessage {
            role: "tool".to_string(),
            content: ChatContent::Text(content.clone()),
            name: None,
            tool_calls: None,
            tool_call_id: Some(tool_use_id.clone()),
        });
    }
    return out;
}

// 无 tool_results: 回退原逻辑，返回单条消息
vec![ChatMessage {
    role: msg.role.clone(),
    content: ChatContent::Parts(parts),
    name: msg.name.clone(),
    tool_calls: tc,
    tool_call_id: None,
}]
```

### 3. 调用方适配

```rust
// claude_to_openai_request 中
// Before:
messages.push(claude_message_to_openai(msg));

// After:
messages.extend(claude_message_to_openai(msg));
```

---

## 单元测试（5）

| # | 场景 | 输入 | 期望 |
|---|------|------|------|
| UT-1 | 单 tool_result 回归 | `[tool_result { id: "tc1", content: "output1" }]` | 1 条 `role="tool"`，`tool_call_id="tc1"` |
| UT-2 | 双 tool_result | `[tool_result(id=tc1, Bash output), tool_result(id=tc2, Read output)]` | 2 条 tool 消息，tc1/tc2 各自匹配 |
| UT-3 | 三 tool_result | 3 个 tool_result block | 3 条 tool 消息，id 不交错 |
| UT-4 | tool_result + text 混合 | `[text("here is the result"), tool_result(tc1, output)]` | 1 条 user(text) + 1 条 tool(output) |
| UT-5 | 空 tool_results 边界 | user 消息无 tool_result block | 1 条 user 消息（原样），name 保留 |

---

## 门禁

- [ ] `cargo test adapter` 全部通过（含新增 5 UT）
- [ ] `cargo test` 全量通过
- [ ] BDD `cargo test --test bdd` 回归通过（93 scenarios）
