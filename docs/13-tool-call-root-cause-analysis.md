# Claude Code 工具调用故障根因分析报告

> 日期：2026-07-14
> 涉事会话：`704be2bf-58d0-40a8-a36f-aeb409652d28`
> 分析范围：从第一次实现 tool_calls 转发的 commit 到最终修复的完整链路

---

## 一、问题 1：为什么多次修改代码都无法让 Claude Code 正常调用工具？

### 1.1 关键事实

Claude Code 向 aigw 发送的是 **非流式（non-streaming）** 请求。

**证据** — 会话日志 `/v1/messages` 的 assistant 响应：

```json
{
  "type": "assistant",
  "message": {
    "id": "20c5797e-6664-4851-a154-66f7b4ec1369",
    "model": "ep-iswqr9k0",
    "role": "assistant",
    "stop_reason": "tool_calls",
    "content": [{"text": "", "type": "text"}]
  }
}
```

这不是 SSE 事件流，而是一个完整的单条 JSON 消息 — 说明 Claude Code 使用 **非流式模式**。

### 1.2 代码变更时间线（按提交顺序）

| 提交 | 时间 | 内容 | 修复了 Claude Code 的问题吗？ |
|------|------|------|:---:|
| `03ccd70` | 17:43 | 实现 tools 转发 + 流式 tool_calls 组装 | ❌ 只修了流式路径 |
| `6ffa5d5` | 18:49 | 用 AnthropicToOpenAIStream 替换 DefaultAdapter SSE 循环 | ❌ 只修了流式路径 |
| `4a2223a` | 18:54 | 把 AnthropicToOpenAIStream 设为 public | ❌ 编译修复 |
| `7c4c2db` | 20:13 | 逐行喂 SSE 给 AnthropicToOpenAIStream | ❌ 只修了流式路径 |
| `50a0d79` | 20:22 | 修复 input_json_delta 字段名 | ❌ 只修了流式路径 |
| `6ae4a84` | 21:47 | content_block_stop 放在 message_stop 之前 | ❌ 只修了流式路径 |
| `4fc829b` | 22:17 | tool_calls 优先于 text delta | ❌ 只修了流式路径 |
| `ef85c41` | 23:40 | 修复非流式路径 + tool_choice 转换 | ✅ **这是第一次修对** |
| `7eba83c` | 23:50 | 过滤 tool_use block 不出现在 ContentParts 中 | ✅ 修复后续 400 |

### 1.3 根因分析

核心问题是 **非流式路径被遗漏**。`v1_messages.rs` 中有两条代码路径：

```rust
// v1_messages.rs — messages_handler()
if is_stream {
    // 流式路径 — 使用了 AnthropicToOpenAIStream
    // ← 7 次提交都在修这个分支
} else {
    // 非流式路径 — 使用了 DefaultAdapter::openai_to_claude_response()
    // ← 这个分支一直没有被修改！
}
```

在 `ef85c41`（23:40）之前，非流式路径的转换代码是：

```rust
// 修复前（v1_messages.rs 旧代码）
let oai_response: ChatCompletionResponse =
    serde_json::from_value(resp_body.clone())...?;

let claude_response =
    DefaultAdapter::openai_to_claude_response(&oai_response);
```

`DefaultAdapter::openai_to_claude_response()` 的实现（adapter.rs）：

```rust
fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse {
    // 只取 choice.message.content → 纯文本
    let content_text = resp.choices.first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();
    // 只生成一个 text 类型的 content block
    ClaudeMessageResponse {
        content: vec![ClaudeContentBlock {
            content_type: "text".to_string(),
            text: Some(content_text),  // tool_calls 时为空字符串 ""
            ...
        }],
        stop_reason: ...,  // 从 finish_reason 转换
        ...
    }
}
```

**此函数完全忽略了 `tool_calls` 字段**。当上游 OpenAI 返回：

```json
{
  "choices": [{
    "message": {
      "role": "assistant",
      "content": "",
      "tool_calls": [{"id": "call_001", "function": {"name": "Bash", "arguments": "..."}}]
    },
    "finish_reason": "tool_calls"
  }]
}
```

转换结果变成：
```json
{
  "content": [{"type": "text", "text": ""}],
  "stop_reason": "tool_use"
}
```

`tool_calls` 被丢弃，content 里只有一个空文本块。Claude Code 收到这个响应后，看到 `stop_reason: "tool_use"` 但没有 `type: "tool_use"` 的内容块，无法执行任何工具。

**为什么之前的 7 次提交没能解决问题？** 因为它们全部聚焦在流式路径（SSE 流的转换），而 Claude Code 根本不走流式路径。

### 1.4 修复（ef85c41）

```rust
// 修复后
let claude_response = adapter.adapt_response(resp_body.clone())
    .map_err(|e| { ... })?;
```

`AnthropicToOpenAI::adapt_response()` 内部调用 `oai_response_to_claude_messages()`，这个函数**正确处理了 tool_calls → tool_use 的转换**（将每个 OpenAI tool_call 映射为一个 `type: "tool_use"` 的 Claude content block）。

---

## 二、问题 2：为什么第一次工具调用成功后，后续请求返回上游 400？

### 2.1 现象

`ef85c41` 修复后（commit time 23:40），Claude Code 成功收到了第一个工具调用：

```
用户: "检查主机名称"
→ aigw 转换 → 上游返回 tool_calls → aigw 转换为 tool_use → Claude Code 收到 ✅
→ Claude Code 执行 Bash("hostname") → 得到结果 "468c2e4affb9"
```

然后 Claude Code 发送第二轮请求，把完整的对话历史和工具结果一起发给 aigw，aigw 转换后转发给上游，上游返回：

```
HTTP 400: messages[3]: missing field `text` at line 1 column 9993
```

### 2.2 数据库中的实际请求体对比

**成功的请求**（req_1db699c8）：只有 3 条消息
```
[0] role=system  content="You are Claude Code..."  (长系统提示)
[1] role=user     content=parts[...]                 (用户输入 "检查主机名称")
[2] role=system   content="Available agent types..." (Agent 列表)
```

**失败的请求**（3cfccdb0）：有 5 条消息，多了 assistant + tool
```
[0] role=system    content="You are Claude Code..."    (长系统提示)
[1] role=user       content=parts[...]                   (用户输入)
[2] role=system     content="Available agent types..."   (Agent 列表)
[3] role=assistant  content=[{"type":"text"}]            ← 问题消息！
                    tool_calls=[{id:"call_00_...", function:{name:"Bash",...}}]
[4] role=tool       content="468c2e4affb9"
                    tool_call_id="call_00_nQI1DXZozm7El6jF6BW05865"
```

**messages[3] 的实际 JSON：**

```json
{
  "role": "assistant",
  "content": [
    {
      "type": "text"       // ← 缺少 "text" 字段！值被 serde skip_serializing_if 省略了
    }
  ],
  "tool_calls": [
    {
      "id": "call_00_nQI1DXZozm7El6jF6BW05865",
      "type": "function",
      "function": {
        "name": "Bash",
        "arguments": "{\"command\":\"hostname\",\"description\":\"Check hostname\"}"
      }
    }
  ]
}
```

上游 tokenhub 解析到 `content[0]` 时要求 `type: "text"` 的 ContentPart 必须有 `text` 字段，但这里 `text: null` 被序列化时跳过了 → `missing field 'text'`。

### 2.3 根因分析

问题出在 `adapter.rs` 的 `claude_message_to_openai()` 函数。

Claude Code 在第二轮请求中，把 assistant 的工具调用消息作为对话历史发给 aigw。这条消息在 Anthropic 格式中为：

```json
{
  "role": "assistant",
  "content": [
    {"type": "text", "text": ""},
    {"type": "tool_use", "id": "call_00_...", "name": "Bash", "input": {...}}
  ]
}
```

`claude_message_to_openai()` 处理的是 `ClaudeContent::Blocks`（多个 content block）。**旧代码**的问题：

```rust
ClaudeContent::Blocks(blocks) => {
    // BUG：把所有 block（包括 tool_use）都转成了 ContentPart
    let parts: Vec<ContentPart> = blocks.iter().map(|b| {
        if b.content_type == "image" {
            ContentPart { content_type: "image_url", text: None, ... }
        } else {
            // ← tool_use block 也走这里！
            // b.text 是 None → 序列化后变成 {"type":"text"} 缺 text 字段
            ContentPart { content_type: "text", text: b.text.clone(), image_url: None }
        }
    }).collect();
    // ...
    // tool_calls 也正确生成了（从 tool_use blocks 提取）
    let tool_calls: Vec<ToolCall> = blocks.iter()
        .filter(|b| b.content_type == "tool_use")
        ...
    // 最终结果：content=[{"type":"text"}], tool_calls=[...]
    // content[0] 缺少 text 字段 → 上游 400
}
```

**致命矛盾**：同一个 tool_use block 被处理了两遍：
1. 被错误地转成了 `ContentPart { type: "text", text: None }`（因为没过滤）
2. 又被正确地转成 `ToolCall` 放进 `tool_calls` 字段

`ContentPart` 结构体定义中的 `text` 字段是 `Option<String>` 且标记了 `skip_serializing_if = "Option::is_none"`：

```rust
pub struct ContentPart {
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,    // ← None 时序列化直接跳过
    ...
}
```

所以序列化结果变成 `{"type":"text"}` — 没有 `text` 字段。

### 2.4 修复（7eba83c）

```rust
ClaudeContent::Blocks(blocks) => {
    // 只对 text/image 类型的 block 生成 ContentPart
    // tool_use 和 tool_result 由后面的 tool_calls/tool_call_id 处理
    let parts: Vec<ContentPart> = blocks.iter()
        .filter(|b| b.content_type == "text" || b.content_type == "image")
        .map(|b| {
            if b.content_type == "image" {
                ContentPart { content_type: "image_url", text: None, ... }
            } else {
                ContentPart { content_type: "text", text: b.text.clone(), image_url: None }
            }
        }).collect();
    // tool_calls 提取不变...
    // tool_results 提取不变...
}
```

加入 `.filter(|b| b.content_type == "text" || b.content_type == "image")` 后，tool_use block 不再被错误地映射为 ContentPart，消息变为：

```json
{
  "role": "assistant",
  "content": [],                    // 空 — 没有 text 类型 block
  "tool_calls": [{...}]            // 工具调用信息在这里
}
```

或者如果 assistant 消息中有 `{"type": "text", "text": ""}` 空文本块，也会被正确保留为 `{"type": "text", "text": ""}`。

---

## 三、完整根因总结

| # | Bug | 位置 | 影响 | 修复提交 |
|---|-----|------|------|----------|
| 1 | 非流式响应只转 text，丢弃 tool_calls | `v1_messages.rs` + `adapter.rs` | Claude Code 收到空 tool_use 响应 | `ef85c41` |
| 2 | 流式 finish() 丢弃 content_block_stop 返回值 | `adapter.rs` | SSE 事件流缺少 content_block_stop | `ef85c41` |
| 3 | 流式 finish() 被重复调用 | `v1_messages.rs` | 重复 message_stop | `ef85c41` |
| 4 | tool_choice Claude 格式直传 OpenAI | `adapter.rs` | 上游 400: unknown variant 'auto' | `ef85c41` |
| 5 | tool_use block 被错误映射为 ContentPart | `adapter.rs` | 上游 400: missing field 'text' | `7eba83c` |

### 为什么多次修改才修好？

**根因是路径分裂**：aigw 的 `/v1/messages` handler 有两条完全不同的代码路径 — 流式与非流式。前 7 次提交全部聚焦在流式路径的完善上（SSE 逐行解析、input_json_delta 字段名、content_block_stop 顺序、tool_calls 优先级等），但 **Claude Code 实际使用的是非流式路径**，而这条路径从一开始就有 Bug 1（丢弃 tool_calls）。

**教训**：在排查问题时，需要先确定客户端实际使用的路径（流式 vs 非流式），再针对性地修复。本次排查的关键转折点是分析了对话日志文件 (`704be2bf-...jsonl`)，发现 assistant 响应是一个完整的 JSON 消息而非 SSE 事件流，从而锁定了非流式路径。

### 为什么第一次成功后第二次反而报 400？

因为 Bug 1（丢弃 tool_calls）恰好 "掩盖" 了 Bug 5。在 Bug 1 存在时，Claude Code 虽然收到了 `stop_reason: "tool_use"`，但没有任何可用的 tool_use 内容块，模型无法完成工具调用，对话就此中断 — 永远不会发送包含工具调用历史的第二轮请求。

当 `ef85c41` 修复了 Bug 1 后，Claude Code 终于成功收到了完整的 tool_use，也就能执行工具并继续对话。但第二轮请求的 assistant 消息中的 tool_use block 触发了 Bug 5 — `claude_message_to_openai()` 把它错误地映射成了不含 `text` 字段的 ContentPart，导致上游 400。

**这是一个典型的 "修复暴露下一个 bug" 的案例**。上游的 tool_choice 转换 Bug（Bug 4）也是类似情况 — 基本请求不触发，只有 Claude Code 携带了 `tool_choice: {"type": "auto"}` 时才暴露。

---

## 四、测试验证

以下测试全部通过（330 个单元测试 + 两个端到端 Python 测试脚本）：

| 测试 | 状态 | 覆盖 |
|------|:----:|------|
| `test_tool_choice_auto_conversion` | ✅ | Claude `{"type": "auto"}` → OpenAI `"auto"` |
| `test_tool_choice_any_conversion` | ✅ | Claude `{"type": "any"}` → OpenAI `"required"` |
| `test_tool_choice_specific_tool_conversion` | ✅ | Claude `{"type": "tool", "name": "x"}` → OpenAI `{"type": "function", ...}` |
| `test_assistant_tool_use_excludes_empty_text` | ✅ | tool_use block 不出现在 ContentParts 中 |
| `test_full_conversation.py` | ✅ | 两轮完整对话（提问→工具调用→返回结果→总结回答） |
| `test_tool_use.py` | ✅ | 流式 SSE 事件完整性（content_block_start/stop、message_stop 不重复） |
| `test_tool_use_nonstreaming.py` | ✅ | 非流式 tool_use 响应 |
| `test_capture_upstream.py` | ✅ | Claude Code 风格请求（system blocks + tool_choice） |
