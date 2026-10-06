# Stage 133: Responses 原生直通 + Codex 合规 SSE 事件序列（Phase 52）

**所属**: Phase 52（Codex 客户端兼容）
**预估**: 8h（实际交付后补写文档）
**依赖**: Stage 131（工具归一化）、Stage 132（多轮 tool 历史）
**状态**: ✅ 完成（2026-10-05，commit `c7cd1f4`）

> **文档补记说明**: 本 Stage 的代码已随 `c7cd1f4` 交付，但当时未同步撰写 stage 文档，导致 `docs/stages/` 缺 133、总进度计数与实际交付不一致（roadmap 记 136，实际 137）。本文档为事后补写，内容依据 commit message 与改动文件回填，**不含规划态内容**。

---

## 1. 目标

Stage 131/132 修完请求侧转换后，部署上线 Codex 仍报：

```
stream disconnected before completion: stream closed before response.completed
```

用「把 aigw 实际产出的 SSE 字节导成假上游、喂给真实 Codex 0.160.0」逐项定位，确认是**两个独立问题**，一并修复。

### 验收标准

- [x] 声明 `supported_standard_types` 含 `responses` 的部署走原生直通，不再被强制降级为 Chat 转换
- [x] 桥接产出的 SSE 事件序列满足 Codex 实测规范（payload 带 `type` + `sequence_number` 单调递增 + 完整 item 生命周期）
- [x] `response.completed` 先于 `[DONE]`
- [x] 真实 Codex 0.160.0 端到端正常渲染、零报错

## 2. 现状证据

### A. 原生直通缺失

`ProviderType` 只有 `OpenAICompatible` / `AnthropicNative`，**未表达「上游原生支持 Responses」**。`model_info.supported_standard_types`（生产 config 已在声明 `responses`）此前 aigw **零消费** —— 导致本该直通的部署被强制降级成 Chat 转换，白跑一遍有损转换。

### B. SSE 事件序列不合规

实测得出的 Codex 分派规范（与常识不符的两点）：

| 规范 | 说明 |
|------|------|
| Codex 按 **payload 的 `type` 字段**分派事件 | **不认 `event:` 行** |
| delta 必须挂在**已声明的 item** 上 | 否则报 `OutputTextDelta without active item` |

## 3. 方案

### 3.1 Responses 原生直通

- `Deployment` 新增 `supported_standard_types`（resolver 从 `model_info` 解析）
- 新增 `select_responses_adapter()`：声明含 `responses` → `ResponsesPassthrough`（原样转发 `/v1/responses`，只改 model 名），否则回落 `ResponsesToChatCompletions`
- **能力判定放在这里而非 `select_adapter`**：`ProviderType` 描述 chat/messages wire 家族，而 `supported_standard_types` 是可按部署追加 Responses 的声明，两者不同维度
- `responses.rs` 上游路径同步：声明含 `responses` → `responses`，否则 `messages` / `chat/completions`

### 3.2 补齐合规 SSE 事件序列

每个事件 payload 补 `type` + 单调递增 `sequence_number`，并补齐完整 item 生命周期：

```
response.created → in_progress → output_item.added → content_part.added
  → output_text.delta× → output_text.done → content_part.done
  → output_item.done → completed
```

`tool_call` 分支同理：先 `output_item.added`（`function_call`）再 `function_call_arguments.delta/done` + `output_item.done`。

`response.completed.output` 回填 message / function_call 项与 usage details。

## 4. TDD

3 个新 UT 锁契约：

- 每个事件 payload 的 `type` == 事件名
- `sequence_number` 单调 + 完整生命周期顺序
- `completed` 先于 `[DONE]`；`completed.output` 镜像 delta 累积

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | +591 / 事件序列补齐 + 直通适配器 |
| `crates/aigw-core/src/deployment.rs` | +6 / `supported_standard_types` 字段 |
| `crates/aigw-core/src/resolver.rs` | +20 / 从 `model_info` 解析 |
| `crates/aigw-server/src/routes/responses.rs` | +52 / 上游路径分支 |
| `crates/aigw-core/src/{oauth_pipeline,router}.rs`、`routes/{chat,health}.rs`、`bdd_steps/anthropic_native_steps.rs` | 字段接线 |

共 9 文件、+598 / −79。

## 6. 回归验证

1. `task test` — aigw-core **530** passed
2. `task bdd` — **285** 场景（272 passed / 13 skipped）/ 1452 steps
3. `task fmt` / `task lint` green
4. **端到端（硬验证）**：导出 aigw 桥接层实际生成的 SSE 字节做成假上游，让真实 Codex 0.160.0 请求 → 正常渲染回复、tokens used 17、**零报错**

## 7. 门禁

- [x] 3 个新 UT 通过
- [x] `task test` / `task bdd` / `task fmt` / `task lint` 全绿
- [x] 真实 Codex 0.160.0 端到端零报错
- [x] git commit（`c7cd1f4`，`--signoff`）
- [ ] ~~stage 文档~~ → 本文档补记

## 8. 风险与遗留

- **直通路径仅经单元层验证**：`supported_standard_types` 含 `responses` 的分支，当前环境**无声明该能力的可达上游**可做实证。待有此类上游后补真实验证。
- 后续两个修复（`4bd85c7` 提前 `[DONE]`、`8cf8c12` 流式 SpendLog 只存最后一个 chunk）均落在同一流式管线，说明 `responses.rs` 流式路径是**高风险区**——后续任何改动需格外小心（Stage 135 已登记此警示）。
