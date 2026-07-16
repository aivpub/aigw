# Stage 60: System Message Normalization（全栈）

**Phase**: 21 — 协议兼容性修复
**状态**: ⏳ 待开始
**预估**: 8h
**依赖**: 无

---

## 目标

实现 `chat_template_compat` 能力标志 + 折叠算法，解决 Claude Code 多 system 消息在 Qwen 等严格模板上游的 400 报错。

---

## 根因

Claude Code v2.1.153+ 将额外 system 上下文塞入 `messages` 数组（`role="system"`）。aigw 的 `AnthropicToOpenAI::adapt_request` 原样透传 role，Qwen 系列 Jinja chat template 强制 system 只能位于 index 0 → 400。

详案：`docs/plans/2026-07-16-system-message-normalization.md`（方案 D：折叠进相邻 user turn）。
ADR：`docs/08-autonomous-decisions.md` ADR-016。

---

## 变更范围

| 模块 | 变更 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | `ChatTemplateCompat` 枚举 + `resolve_chat_template_compat()` + `fold_extra_systems_into_adjacent_user()` + 8 UT；`AnthropicToOpenAI::adapt_request` 增条件分派 |
| `crates/aigw-core/src/deployment.rs` | `Deployment` 增 `chat_template_compat: Option<String>` |
| `crates/aigw-core/src/resolver.rs` | 装配时从 `model_info.chat_template_compat` 读取 |
| `crates/aigw-frontend/src/pages/models/` | ModelDialog 新增下拉控件 |
| 数据库 | 无 migration（字段在 `model_info` JSON 中） |

---

## 实现要点

### 1. `ChatTemplateCompat` 枚举（adapter.rs）

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChatTemplateCompat {
    Auto,   // 按模型名嗅探
    Strict, // 强制折叠非首位 system
    Loose,  // 原样透传
}
```

### 2. 嗅探逻辑

```rust
fn resolve_chat_template_compat(deployment: &Deployment) -> ChatTemplateCompat {
    match deployment.chat_template_compat.as_deref() {
        Some("strict") => return ChatTemplateCompat::Strict,
        Some("loose") => return ChatTemplateCompat::Loose,
        Some(other) => {
            tracing::warn!(%other, "未知 chat_template_compat 值，fallback 到 auto");
        }
        _ => {}
    }
    // Auto sniff: case-insensitive
    if deployment.upstream_model.to_lowercase().contains("qwen") {
        ChatTemplateCompat::Strict
    } else {
        ChatTemplateCompat::Loose
    }
}
```

### 3. 折叠算法 `fold_extra_systems_into_adjacent_user()`

核心规则（详见方案文档 4.3 节）：

- 首位 `role="system"` 保留
- 非首位 system → 用 `<system-reminder>` 标签包裹，折叠进相邻 user turn
- 通过 `pending_reminders` 缓冲区预插到下一 user 的 content 前面
- 末尾无下一 user 时追加到最后一条 user
- 无任何 user 时兜底新造一条 user 消息
- `ContentPart::ToolResult` 类型的 user 消息不接受前插 — `pending_reminders` 继续向下传递
- `ChatContent::Parts` 前插时在 `parts[0]` 处插入新 `ContentPart { type: "text", text: reminder }`
- `ChatContent::Text` 直接字符串拼接
- 后置不变式：`debug_assert!` 确保 `role="system"` 只出现在 index 0

### 4. `AnthropicToOpenAI::adapt_request` 分派

```rust
fn adapt_request(&self, req: &Value, deployment: &Deployment) -> Result<Value> {
    let claude_req: ClaudeMessageRequest = serde_json::from_value(req.clone())?;
    let messages = claude_to_openai_request(&claude_req);
    let compat = resolve_chat_template_compat(deployment);
    let messages = match compat {
        ChatTemplateCompat::Strict => fold_extra_systems_into_adjacent_user(messages),
        ChatTemplateCompat::Loose => messages,
    };
    // ... 构造 ChatCompletionRequest
}
```

### 5. Deployment 字段（deployment.rs）

```rust
pub struct Deployment {
    // ... existing fields ...
    /// chat_template_compat from model_info — "auto" / "strict" / "loose"
    pub chat_template_compat: Option<String>,
}
```

### 6. Resolver 装配（resolver.rs）

```rust
let chat_template_compat = proxy_model.model_info
    .get("chat_template_compat")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());
```

### 7. 前端 ModelDialog

在 model_info 编辑区新增下拉控件：

| 显示 | 写入 `model_info.chat_template_compat` |
|------|---------------------------------------|
| 自动嗅探（推荐） | 不写入字段（缺省即 auto） |
| 严格（Qwen 类模板） | `"strict"` |
| 宽松（原样透传） | `"loose"` |

Tooltip：部分模型（如 Qwen 系列）的 chat template 要求 system 消息只能位于首位，否则请求会被 400 拒绝。"自动嗅探"按模型名判断；如果自动嗅探误判，可手动切换。

---

## 存量兼容（无 migration）

存量 `proxy_models.model_info` JSON 中不存在 `chat_template_compat` 字段，不需要 migration。

### 完整决策链路

```
model_info.chat_template_compat
  ├─ Some("strict")  → Strict（显式覆盖）
  ├─ Some("loose")   → Loose（显式覆盖）
  ├─ Some(other)     → warn + fallthrough to auto sniff
  └─ None／缺省      → auto sniff
                         ├─ upstream_model 名含 "qwen" → Strict
                         └─ 其它                      → Loose
```

### 存量影响矩阵

| 上游模型 | model_info 有无字段 | 最终行为 | 与现在相比 |
|---------|-------------------|---------|-----------|
| qwen/qwen3.5-9b | 无 | Strict → 折叠非首位 system | **变化**: 不再 400 ✅ |
| tokenhub/deepseek-v4-pro | 无 | Loose → 原样透传 | **无变化** |
| tokenhub/glm5.2 | 无 | Loose → 原样透传 | **无变化** |
| tke/gpt-4o | 无 | Loose → 原样透传 | **无变化** |
| 任意 Qwen 变体 (qwen-max/Qwen2.5-VL) | 无 | Strict → 折叠 | **变化**: 不再 400 ✅ |
| 用户手工选了"宽松"的 qwen | `"loose"` | Loose → 原样透传 | 显式覆盖，用户意图优先 |

### 前端展现

存量模型打开 ModelDialog 编辑时：
- `model_info.chat_template_compat` → `undefined`
- 下拉显示 **"自动嗅探（推荐）"**
- 用户不改直接保存 → 不写该 key → 行为不变

> 这与新建模型的行为完全一致：不填就是 auto sniff。

---

## 单元测试（8）

| # | 场景 | 输入 | 期望 |
|---|------|------|------|
| UT-1 | 真实 body 复现 | 顶层 system + `[user_with_parts, system(agent-list)]` + Strict | `[system(top), user_with_reminder]`，无第二条 system |
| UT-2 | 多 system 夹杂 | `[u1, s1, a1, u2, s2, s3, u3]` + Strict | 折叠正确、时序保留、reminder 数量守恒 |
| UT-3 | 末尾 system | `[u1, u2, s1]` + Strict | s1 追加到 u2 末尾 |
| UT-4 | 双 system 相邻 | `[s1, u1, s2, s3, u2]` + Strict | s2、s3 依序并入 u2 |
| UT-5 | 无 user 兜底 | `[s1, assistant, s2]` + Strict | s1、s2 各变成一条 user 包裹 reminder |
| UT-6 | Loose 对照 | 同 UT-2 输入 + Loose | messages 原样透传 |
| UT-7 | 嗅探大小写 | upstream_model ∈ `{"qwen/qwen3.5-9b", "Qwen2.5-VL-72B", "gpt-4"}` | 前两个 Strict，gpt-4 Loose |
| UT-8 | 显式 override | `upstream_model="qwen-max"` + `chat_template_compat="loose"` | Loose |

---

## 前端 BDD（3 × 3 viewports）

| # | 场景 | 验证点 |
|---|------|--------|
| BDD-1 | 显示下拉控件 | 三选项可见，默认"自动嗅探" |
| BDD-2 | 选"严格"保存后重开 | model_info 含 `"chat_template_compat": "strict"` |
| BDD-3 | 选"宽松"保存后重开 | model_info 含 `"chat_template_compat": "loose"` |

---

## 端到端手工验证

- [ ] Claude Code → aigw `/v1/messages` → LMStudio qwen3.5-9b：请求通过，无 400
- [ ] Claude Code → aigw `/v1/messages` → GPT-4/DeepSeek：回归无异常
- [ ] 常规 OpenAI Client → aigw `/v1/chat/completions` → LMStudio qwen：不受影响

---

## 门禁

- [ ] `cargo test adapter` 全部通过（含新增 8 UT）
- [ ] `cargo test` 全量通过
- [ ] BDD `cargo test --test bdd` 回归通过（93 scenarios）
- [ ] 前端 BDD 108 → 111 tests（+3 × 3 viewports）
- [ ] E2E 手工验证清单完成

---

## 参考

- `docs/plans/2026-07-16-system-message-normalization.md` — 完整方案详案
- `docs/08-autonomous-decisions.md` ADR-016 — 决策记录
- `docs/debug/lmstudio-qwen-system-message.md` — 排障归档
