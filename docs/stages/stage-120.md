# Stage 120: GLM5 → Claude Code Invalid tool parameters 修复

**所属**: Phase 48（流式 tool_use 精度修复）
**预估**: 4-6h（后端修复 + UT + 文档订正）
**依赖**: 无（adapter.rs 内部改动）
**状态**: ⏳ 进行中

---

## 1. 目标

彻底修复 aigw 转发 GLM5（tokenhub 上游）到 Claude Code 出现 `Invalid tool parameters` / `__unparsedToolInput` 的问题。**根因是流式 SSE 转换中「首帧同时携带 tool_call id 与 arguments 时 arguments 被丢帧」，不是文档 `docs/16-glm-stream-delta-analysis.md` 早期分析的「delta 累积语义差异」**——本 Stage 同步订正该文档。

## 2. 根因（订正）

### 2.1 Anthropic 官方规范（Anthropic docs / SDK 交叉验证）

`input_json_delta.partial_json` **本身就是纯增量片段**，客户端负责累积。原文（`platform.claude.com/docs/en/api/messages-streaming`）：

> "You can accumulate the string deltas and parse the JSON once you receive a `content_block_stop` event"

官方示例序列：

```
partial_json: ""
partial_json: "{\"location\":"
partial_json: " \"San"
partial_json: " Francisc"
partial_json: "o,"
partial_json: " CA\"}"
```

明显是碎片而非累积快照。因此 aigw 直接透传上游 OpenAI `arguments` 增量作为 `partial_json` **在语义上是正确的**。

### 2.2 真正的 bug：`AnthropicToOpenAIStream::next` 首帧丢帧

`crates/aigw-core/src/adapter.rs:647-694`（`AnthropicToOpenAIStream::next` 的 tool_calls 处理段）：

```rust
if let Some(ref tool_calls) = choice.delta.tool_calls {
    for tc in tool_calls {
        if let Some(ref id) = tc.id {
            if !id.is_empty() {
                // ...
                return self.emit_event(content_block_start_event);
                // ↑ 直接 return，同 chunk 的 tc.function.arguments 被丢弃！
            }
        }
        if !tc.function.arguments.is_empty() {
            return self.emit_event(input_json_delta_event);
        }
    }
}
```

**问题**：首个带 `id` 的 chunk 若同时携带 `arguments`（tokenhub GLM-5.2 首帧就是 `id=call_xxx, arguments="{\""`），代码 emit 完 `content_block_start` 立即 `return`，把首帧 arguments **静默丢弃**。下游 Claude Code 拿到的累积 partial JSON 就永远缺开头 `{"`。

### 2.3 与观测数据对照

| 上游 | 首个带 id 的 chunk 的 arguments | 丢帧后果 |
|---|---|---|
| tokenhub GLM-5.2 | `{"` (2B) | 丢开头 `{"`，Claude Code 报 `Invalid tool parameters` |
| MAAS GLM-5 | `""` (0B) | 空丢弃无损失，正常 |
| DeepSeek V4 Pro | `""` (0B) | 空丢弃无损失，正常 |
| OpenAI 官方 | `""` (0B) | 空丢弃无损失，正常 |

Claude Code 会话日志中错误的 `__unparsedToolInput.raw = "\"command\": \"...\"}"`（缺开头 `{`）与此完全吻合。

### 2.4 反向对称问题

`OpenAIToAnthropicStream::next`（同文件 1731-1830 行）逻辑对称，**同一个 return 结构 bug**——首帧 id + arguments 同时出现时，arguments 也会被丢帧。影响面较窄（`/v1/responses` 反向路径 + `/v1/chat/completions` 上游为 Anthropic-native 的场景），但为保持双向一致必须一并修复。

## 3. 方案

### 3.1 核心思路

把 tool_calls 分支的「emit 后 return」改成「emit 后追加到本地 buffer，循环结束返回全部 SSE frames」。SSE 协议允许多个 `event:...\ndata:...\n\n` 拼接在同一 HTTP chunk 内，客户端按 `\n\n` 分帧读取，兼容性无问题。

### 3.2 `AnthropicToOpenAIStream::next` 改造

```rust
let mut out: Vec<u8> = Vec::new();
// message_start（若未开始）→ 直接 return（首帧独立，保持原语义）
// text 分支保留 return（tokens 数量少，单帧透传性能足够）
// tool_calls 分支：改为 out.extend_from_slice + 循环末尾统一返回
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
// 最终 if out.is_empty() { None } else { Some(out) }
```

**范围收敛**：只改 tool_calls 分支的 return 结构；text_delta / message_start / finish_reason 三条分支保持既有语义，避免引入 regression。

### 3.3 `OpenAIToAnthropicStream::next` 对称改造

同 3.2，逻辑对称，改动幅度相同。

### 3.4 不做的事

- **不引入 tool_use block 关闭/切换逻辑**：GLM5 场景一次调用一个工具，多工具切换目前无实证用例；先保 TODO 留下一 Stage。
- **不改 partial_json 累积语义**：Anthropic 规范就是纯增量，`docs/16-glm-stream-delta-analysis.md` 原提案「累积后再发」反而违反规范。

## 4. TDD 计划

### 4.1 新增 UT（先失败）

`crates/aigw-core/src/adapter.rs` 测试段追加：

| 用例 | 目标 |
|---|---|
| `test_stage120_glm5_first_chunk_id_and_args` | AnthropicToOpenAIStream 首帧同时含 `id` + `arguments="{\""` 时，返回值必须同时包含 `content_block_start` 与 `input_json_delta` 两个 SSE frame，且 `partial_json = "{\""` |
| `test_stage120_glm5_reverse_first_chunk_id_and_args` | OpenAIToAnthropicStream 对称场景断言 |
| `test_stage120_multiple_arg_frags_accumulate` | 后续多个纯 arguments 增量帧按顺序透传，无遗漏 |

### 4.2 UT 门禁

- `task test` 全绿（预期在修复前新增 3 个 UT 会 FAIL；修复后 FAIL → PASS）
- `task lint` 无 clippy warning
- `task fmt` 无格式偏差

## 5. 变更清单

| 文件 | 改动 |
|---|---|
| `crates/aigw-core/src/adapter.rs` | `AnthropicToOpenAIStream::next` tool_calls 分支重构 ~15 行；`OpenAIToAnthropicStream::next` 对称重构 ~15 行；测试段追加 3 个 UT |
| `docs/16-glm-stream-delta-analysis.md` | 订正第五节「责任归属」+ 第六节「修复方案」，重写为「return 丢帧」结论 |
| `docs/stages/stage-120.md` | 本文件 |
| `docs/11-next-steps.md` | Phase 48 / Stage 120 完成回写 |

## 6. 回归验证

1. `task test` 全绿（aigw-core UT ≥ 435，含 3 个新增）
2. `task test-bdd` mock BDD 246 保持基线
3. **真实链路手测（可选，用户环境）**：tokenhub GLM-5.2 走 Claude Code 会话，确认 `__unparsedToolInput` 不再出现

## 7. 门禁

- [ ] 3 个新 UT 先 fail 后 pass（TDD 证据链完整）
- [ ] `task test` / `task lint` / `task fmt` 全绿
- [ ] `docs/16-glm-stream-delta-analysis.md` 订正落盘
- [ ] `docs/11-next-steps.md` 回写 Phase 48
- [ ] git commit（精确 add，不用 `-A`/`.`）
