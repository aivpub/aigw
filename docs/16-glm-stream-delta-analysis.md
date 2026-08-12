# GLM-5.2 流式 Tool Call Arguments Delta 分析报告

> 日期：2026-07-15
> 涉事会话：`7973b325-6c9f-4327-a616-b1606170a64b` (GLM-5.2 via tokenhub)
> 对照会话：`704be2bf-58d0-40a8-a36f-aeb409652d28` (DeepSeek V4 Pro via tokenhub)
> 分析范围：tokenhub/GLM-5.2 流式模式下 `Invalid tool parameters` 根因定位、日志验证、GitHub issue 交叉比对

---

## 一、现象

### 1.1 问题描述

同样的 Claude Code 会话，使用 **tokenhub/GLM-5.2**（模型 ID `ep-kf3j4a8u`）时反复出现 "Invalid tool parameters" 报错，而使用 **tokenhub/DeepSeek V4 Pro**（模型 ID `ep-iswqr9k0`）时完全不出现。

### 1.2 数据概览

| 指标 | GLM-5.2（流式） | DeepSeek V4 Pro |
|------|----------------|-----------------|
| 工具调用总数 | 22 | 1 |
| JSON 格式正确 | 12 (54.5%) | 1 (100%) |
| JSON 格式错误 (`__unparsedToolInput`) | 10 (45.5%) | 0 (0%) |
| 会话总耗时 | ~498 秒（8.3 分钟） | ~5 秒 |
| 场景 | 检查内核版本和加载模块 | 检查内核版本 |

### 1.3 错误表现

Claude Code 会话日志（`.jsonl`）中错误工具调用的 `input` 结构：

```json
{
  "type": "tool_use",
  "name": "Bash",
  "input": {
    "__unparsedToolInput": {
      "raw": "\"command\": \"ls /sys/module 2>/dev/null | wc -l\", \"description\": \"Count /sys/module entries\"}",
      "len": 92
    }
  }
}
```

注意：`__unparsedToolInput.raw` 的内容为合法的 JSON 键值对但**缺失外层的 `{`（左花括号）**。Claude Code 的 JSON parser 无法解析，最终报 `InputValidationError`。

完整的 10 次错误调用中，`raw` 字段缺失模式完全一致——全部缺开头 `{`。

### 1.4 流式 vs 非流式 对照

通过分析 `stop_reason` 和 `usage` 字段区分流式/非流式响应，发现非流式成功率 100%：

| 模式 | 工具调用次数 | 成功 | 失败 | 成功率 |
|------|------------|------|------|--------|
| **非流式** (`stop_reason` 明确, `usage` 非零) | 5 | 5 | 0 | **100%** |
| **流式** (`stop_reason=null`, `usage={0,0}`) | 17 | 7 | 10 | **41%** |

---

## 二、初期假设与验证

### 2.1 初期假设：GLM-5.2 流式输出丢失 `{`

最初分析会话日志后假设：GLM-5.2 的 tokenizer 在流式输出 tool_use JSON 时，`{` 在某 chunk 边界上丢失。

### 2.2 数据库验证

查询 aigw.db 中 GLM-5.2 会话期间的全部请求：

```
sqlite3 aigw.db "
SELECT request_id, call_type, prompt_tokens, completion_tokens
FROM spend_logs
WHERE model LIKE '%glm%' AND start_time >= '2026-07-15T02:50:00'
ORDER BY start_time;
"
```

发现了关键对照：同一 prompt 内容（`prompt_tokens=25344`），流式和非流式返回的 `completion_tokens` 完全相同（34 tokens），但流式解析失败：

```
req_c79f25dd-4f49-4baf-9feb-a886de6a7605 (completion_stream, 25344 prompt_tokens) → ❌ 失败
78d827dc-1b00-4719-8781-22a753f913df (completion, 25344 prompt_tokens) → ✅ 正常
```

**质疑**：DB 中两个请求的 `response` 字段 `arguments` 内容都是合法的完整 JSON，流式聚合后的最终结果并没有缺花括号。那问题在哪？

```
流式: "arguments":"{\"command\": \"ls ...\", \"description\": \"...\"}"
批量: "arguments":"{\"command\":\"ls ...\",\"description\":\"...\"}"
```

→ 结论：问题不在最终响应体，而在 SSE 流式 delta 传输链路中。

---

## 三、日志探测定性（关键）

### 3.1 日志埋点

在 aigw 流式链路两个关键路径加了 `🔥` 标记日志：

1. **上游 SSE 原始数据层** (`crates/aigw-server/src/routes/v1_messages.rs:527`)
   — 检测 `choices[].delta.tool_calls[].function.arguments` 的首字符

2. **Anthropic 转换层** (`crates/aigw-core/src/adapter.rs:317`)
   — 在 `AnthropicToOpenAIStream::next()` 将 OpenAI chunk 转为 Claude `input_json_delta` 时检测

日志格式：
```
🔥 UPSTREAM SSE tool_calls.arguments OK (first_char='{', len=...)
🔥 UPSTREAM SSE tool_calls.arguments MISSING opening brace (first_char='...', len=...)
🔥 input_json_delta OK (first_char='{', len=...)
🔥 input_json_delta MISSING opening brace (first_char='...', len=...)
```

### 3.2 tokenhub/GLM-5.2 日志（有问题）

第一个 tool call arguments 的 SSE delta 序列：

```
🔥 OK   (len=2):  '{"'                ← 第 1 个 delta 有 {
🔥 MISS (len=7):  '"command"'         ← 后续全是纯增量片段
🔥 MISS (len=3):  '":"'
🔥 MISS (len=5):  '"uname"'
🔥 MISS (len=2):  '" -"'
🔥 MISS (len=1):  '"a"'
🔥 MISS (len=1):  '"\""'
🔥 MISS (len=2):  '","'
🔥 MISS (len=11): '"description"'
🔥 MISS (len=3):  '":"'
🔥 MISS (len=4):  '"Show"'
...
🔥 MISS (len=1):  '"}"'              ← 最后 delta 是 JSON 的关闭
```

总计 **27 个 delta** 拼出完整 JSON `{"command": "uname -a", "description": "Show kernel version info"}`。

每个 SSE delta 是**纯增量 token**——只包含本条新增的字符，不含之前的内容。

### 3.3 MAAS/GLM-5 日志（正常）

```
🔥 WARN (len=0):  ''                                 ← 首个空 delta
🔥 OK   (len=20): '{"command": "uname -"'             ← 含完整 key + 部分 value
🔥 MISS (len=19): 'a\", \"description\": "'
🔥 MISS (len=25): '"Show kernel version info"'
🔥 MISS (len=1):  '"}'
```

仅 **5 个 delta** 拼出完整 JSON，初始 delta 已经是可部分解析的结构体。

### 3.4 结论：两种 delta 策略

| | MAAS GLM-5 | Tokenhub GLM-5.2 |
|---|---|---|
| delta 数量 | ~5 | ~27 |
| 首个有效 delta | `{"command": "uname -"` (20B) | `{"` (2B) |
| delta 策略 | **词组级累积增量** | **逐 token 纯增量** |
| Claude Code 能否解析首个 delta | ✅ 合法 partial JSON | ❌ `{"` 无法单独解析 |
| aigw adapter 能否正确处理 | 碰巧，首个 delta 够大 | ❌ 未做 delta 累积 |

**一个完整合法 JSON `arguments` 在流式链路中的表现差异**：

Tokenhub GLM-5.2 发的是：
```
delta1: {"           ← 只有这 2 字节带 {，其余全是裸片段
delta2: "command"
delta3:  ":"  
delta4:  "uname"
...
delta27: "}"
```

MAAS GLM-5 发的是：
```
delta1: {"command": "uname -"
delta2: a\", \"description\": "
delta3: "Show kernel version info"
delta4: "}"
```

---

## 四、GitHub Issue 交叉比对

### 4.1 vLLM #43267 — Streaming output for tool_calls arguments

> https://github.com/vllm-project/vllm/issues/43267

vLLM **原本不支持流式输出 tool_calls arguments**，完整 JSON 被一次性吐出。这个 feature request 推动 vLLM 改为增量式输出。

开发者确认了 OpenAI 规范中 arguments delta 的预期行为：

> *"each streamed chunk's `delta.tool_calls[i].function.arguments` should be the **next incremental fragment** (e.g. `{"city":` → ` "Paris"}`)"*

（这是**纯增量式**，不是累积式。tokenhub 的行为符合 OpenAI 规范。）

### 4.2 vLLM PR #31220 — GLM 4.7 tool call parser 修复

> https://github.com/vllm-project/vllm/pull/31220

GLM 系列在 vLLM 上有专门的 tool call parser 修复。印证了 GLM 的 tool call 输出格式一直因版本和引擎而异。

### 4.3 Bifrost #3588 — Anthropic tool call continuation deltas

> https://github.com/maximhq/bifrost/pull/3588

另一个 AI Gateway 项目，精确命中了我们遇到的问题：

> *"suppress empty Anthropic tool input deltas when converting to OpenAI chat-completions streams"*
>
> *"omit repeated `tool_calls[].type` metadata on continuation argument chunks"*

OpenAI → Anthropic 的 `input_json_delta` 转换中，delta 的"片段 vs 累积"问题已经被多个网关项目踩过。

### 4.4 llama.cpp #16932 — GLM streaming tool-call parsing

> https://github.com/ggml-org/llama.cpp/pull/16932

GLM 4.5/4.6 + 其他中国模型（MiniMax M2, SeedOSS, Kimi-K2, Qwen3-Coder）需要专门的 streaming tool-call 解析支持，说明这批模型的输出格式都偏离主流。

---

## 五、根因总结（2026-08-12 订正）

### 5.0 前次分析的误判

本文档 2026-07-15 首版判定根因为「Anthropic `partial_json` 要求累积语义 vs OpenAI 纯增量语义差异，aigw 未做累积」。**该结论错误**。交叉 Anthropic 官方规范（`platform.claude.com/docs/en/api/messages-streaming`）+ `anthropic-sdk-python` helpers 文档确认：

> "You can accumulate the string deltas and parse the JSON once you receive a `content_block_stop` event"

Anthropic 官方示例的 `partial_json` 序列本身就是碎片：

```
partial_json: ""
partial_json: "{\"location\":"
partial_json: " \"San"
partial_json: " Francisc"
partial_json: "o,"
partial_json: " CA\"}"
```

因此 aigw 直接把 OpenAI `arguments` 增量透传成 `partial_json` **在语义上完全正确**。修复方向不应是累积。

### 5.1 真正的根因：首帧丢帧

`crates/aigw-core/src/adapter.rs:647-694`（`AnthropicToOpenAIStream::next` 的 tool_calls 处理段）：

```rust
if let Some(ref tool_calls) = choice.delta.tool_calls {
    for tc in tool_calls {
        if let Some(ref id) = tc.id {
            if !id.is_empty() {
                // ...
                return self.emit_event(&content_block_start_event);
                // ↑ early-return，同 chunk 的 tc.function.arguments 被丢弃！
            }
        }
        if !tc.function.arguments.is_empty() {
            return self.emit_event(&input_json_delta_event);
        }
    }
}
```

首个带 `id` 的 chunk 若同时携带 `arguments`（tokenhub GLM-5.2 首帧就是 `id=call_xxx, arguments="{\""`），代码 emit 完 `content_block_start` 立即 `return`，把首帧 arguments **静默丢弃**。后续帧仅补齐 `"command":...}` 部分，Claude Code 累积得到的 partial JSON 永远缺开头 `{"`。

### 5.2 与观测数据吻合

| 上游 | 首个带 id 的 chunk 的 `arguments` | 丢帧后果 |
|---|---|---|
| tokenhub GLM-5.2 | `{"` (2B) | 丢开头 `{"`，累积成 `"command":..."` → 报 `Invalid tool parameters` |
| MAAS GLM-5 | `""` (0B) | 空丢弃无损失，正常 |
| DeepSeek V4 Pro | `""` (0B) | 空丢弃无损失，正常 |
| OpenAI 官方 | `""` (0B) | 空丢弃无损失，正常 |

Claude Code 会话日志中的错误 `__unparsedToolInput.raw = "\"command\": \"...\"}"`（缺开头 `{`）与「首帧被吞」完美对应。

### 5.3 反向对称问题

`OpenAIToAnthropicStream::next`（同文件 1731-1830 行）逻辑对称，**同一个 early-return bug**。影响 `/v1/responses` 反向路径 + 上游为 Anthropic-native 但客户端为 OpenAI 协议的场景（面较窄）。为双向一致必须一并修复。

### 5.4 责任归属

- **tokenhub/GLM-5.2**：行为完全合规。按 OpenAI 规范发送纯增量 `arguments` delta
- **MAAS/GLM-5**：发词组级累积 delta（大概率 vLLM 旧版部署），**首帧 arguments 为空**，碰巧避开了 aigw 的丢帧 bug
- **aigw**：`AnthropicToOpenAIStream::next` + `OpenAIToAnthropicStream::next` 首帧 early-return 丢帧，是**本次故障的唯一根因**
- **Claude Code**：按 Anthropic 规范累积 `partial_json`，行为正常

### 5.5 tokenhub vs MAAS 差异解释

MAAS 大概率用 **vLLM 旧版**（未启用 streaming tool_calls feature，或用较早的 chunker），tool_call 首帧 `id` 到达时 `arguments` 一般为空；tokenhub 用**新版 vLLM 或 SGLang**，按 OpenAI 规范发出逐 token 增量，首帧 `id` 与 `arguments="{\""` 同帧出现，触发 aigw 丢帧。

头信息被 tokenhub 网关（`x-request-id`、`x-trace-id`）屏蔽，无法从外部确认底层推理引擎。

---

## 六、修复方案（2026-08-12 订正版，Stage 120）

### 6.1 方案：tool_calls 分支去除 early-return，收集到本地 buffer 后统一返回

**位置**：`crates/aigw-core/src/adapter.rs` — `AnthropicToOpenAIStream::next()` 与 `OpenAIToAnthropicStream::next()`

**核心逻辑**：

```rust
let mut out: Vec<u8> = Vec::new();

if let Some(ref tool_calls) = choice.delta.tool_calls {
    for tc in tool_calls {
        if tc.id.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
            if let Some(ev) = self.emit_event(&content_block_start_event(...)) {
                out.extend_from_slice(&ev);
            }
        }
        if !tc.function.arguments.is_empty() {
            if let Some(ev) = self.emit_event(&input_json_delta_event(...)) {
                out.extend_from_slice(&ev);
            }
        }
    }
}
// finish_reason 也 append 到 out
// 最后 if out.is_empty() { None } else { Some(out) }
```

**关键点**：
1. **不改语义**：`partial_json` 依然是纯增量片段，符合 Anthropic 规范
2. **不加累积 buffer**：Anthropic 规范就是碎片，累积反而违反规范
3. **只改 return 结构**：从 early-return 改为「append 到本地 buffer + 循环末尾统一返回」
4. **SSE 兼容**：多个 `event:...\ndata:...\n\n` frame 可拼接在同一 HTTP chunk 内，客户端按 `\n\n` 分帧，无兼容性问题

### 6.2 范围收敛（不做的事）

- **不改 text_delta / message_start / finish_reason 三条分支**：保持既有语义，避免引入 regression
- **不做多 tool_use block 切换**：GLM5 场景一次调用一个工具，多工具无实证用例；TODO 留下 Stage

### 6.3 影响范围

- `crates/aigw-core/src/adapter.rs`：两个 stream adapter 各改 ~15 行；测试段追加 3 个 UT
- `crates/aigw-server/src/routes/v1_messages.rs`：**无需改动**
- 前端 / 数据库 / 其他 crate：**无需改动**

### 6.4 兼容性

- tokenhub GLM-5.2（首帧 `id + {"`）：✅ 修复后同帧发 `content_block_start` + `input_json_delta("{\"")`
- MAAS GLM-5（首帧 `id + ""`）：✅ 空 arguments 不 emit，行为与旧版一致
- DeepSeek / OpenAI 官方：✅ 首帧 `arguments=""`，行为不变
- 后续多个纯 arguments 增量帧：✅ 每帧独立 emit，与旧版行为一致

### 6.5 具体落地见 `docs/stages/stage-120.md`

---

## 七、附录

### A. 原始日志文件

| 文件 | 内容 |
|------|------|
| `glm5.log` | Tokenhub GLM-5.2 流式请求日志（含 🔥 delta 探针输出） |
| `glm5-ok.log` | MAAS GLM-5 流式请求日志（对照） |
| `hack/projects/-/7973b325-6c9f-4327-a616-b1606170a64b.jsonl` | GLM-5.2 完整会话记录 |

### B. 关键 req_id

| req_id | 说明 |
|--------|------|
| `req_c79f25dd-4f49-4baf-9feb-a886de6a7605` | 流式，`__unparsedToolInput`，25344 prompt tokens |
| `78d827dc-1b00-4719-8781-22a753f913df` | 非流式，正常 JSON，25344 prompt tokens（与上条同一 prompt） |
| `req_3bcc1778-03c5-44cf-821d-d7d26690efcf` | 流式，首个 `__unparsedToolInput`，22388 prompt tokens |
| `162b5094-92b8-42c0-ba3b-4aa9d220bc8f` | 非流式，相同内容的正常重试 |

### C. 相关代码位置

| 文件 | 行号 | 说明 |
|------|------|------|
| `crates/aigw-core/src/adapter.rs` | 193-196 | `BlockType` 枚举定义 |
| `crates/aigw-core/src/adapter.rs` | 244-343 | `AnthropicToOpenAIStream::next()` — Delta 转换核心 |
| `crates/aigw-core/src/adapter.rs` | 317-323 | `input_json_delta` 发送点（需加累积逻辑） |
| `crates/aigw-server/src/routes/v1_messages.rs` | 440-554 | `/v1/messages` SSE streaming 处理入口 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 525-548 | 上游 SSE delta 解析与转发 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 386-399 | 🔥 上游 Headers 探针（临时） |
