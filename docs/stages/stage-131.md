# Stage 131: Codex Responses→Chat 桥接兼容性修复（Phase 52）

**所属**: Phase 52（Codex 客户端兼容）
**预估**: 12h（tools 归一化 + role 归一化 + content part 映射 + UT/BDD + 文档）
**依赖**: 无（独立缺陷修复；复用 Phase 21 / Stage 60 已有折叠机制）
**状态**: ✅ 完成（代码 + UT + BDD + 门禁全绿；未提交/未部署）

---

## 0. 实施结果（2026-10-05）

| 项 | 结果 |
|----|------|
| 工具归一化 | ✅ `normalize_responses_tools`：`function` 转嵌套 / `namespace` 拍平 `{ns}__{child}`（重名报错）/ `custom`+`tool_search` 降级 / 服务端工具丢弃 + `tracing::warn` |
| `tool_choice` 清理 | ✅ `normalize_tool_choice`：指向已丢工具则丢弃，`auto`/`none` 字符串形态原样 |
| `developer` 归一 | ✅ `merge_developer_into_system` + `consolidate_system_messages`（内联 system 也合并到首位）；默认映射，`developer_role_passthrough=true` 时透传 |
| content part 映射 | ✅ `input_text`/`output_text` → `text` |
| 新增字段 | ✅ `Deployment.developer_role_passthrough: Option<bool>` + resolver 从 `model_info` 解析 |
| UT | ✅ **13 个新增**（含 Codex 抓包 fixture 端到端回归）；aigw-core 517 passed |
| BDD | ✅ 改写 2 条（rejected → dropped）+ 新增 4 条；**mock BDD 283 场景（270 passed / 13 skip）/ 1436 steps 全绿** |
| 门禁 | ✅ `task test` / `task bdd` / `task fmt` / `task lint` 全绿 |
| 真实上游验证 | ✅ 按适配器产出形态构造的请求打生产网关 → **200**（此前 400） |

**实施差异（vs §3 计划）**：
1. 新增 `consolidate_system_messages`——§3.3 只提「避免产生第二个 system」，实测发现 `input[]` 自带内联 `system` 时仍会产生第二个，故补一步把所有 system 消息合并到首位（内容保真，不折叠成 `<system-reminder>`）。
2. 实现路径选「显式 `index == 0` 检查 + 合并」，而非复用 `fold_extra_systems_into_adjacent_user`（后者会把内容降权进 user turn）。§3.3 的 M1 方案即为此，本次落地未用 fold。
3. `mcp` 归入丢弃（§3.1 已定），实测上游 400 佐证。

**未做（与计划一致）**：`input[].type` item 分派（TD-017a）、内建搜索执行、`features.multi_agent=false` 等客户端侧配置（属远端环境，非网关）。

---

## 1. 目标

Codex CLI 0.157.1 以 `wire_api = "responses"` 接入 aigw 时**完全不可用**——首个请求即 400：

```
Unsupported: tool type 'namespace' is not supported in Responses→Chat bridge.
Only 'function' tools are supported.
```

修复 `ResponsesToChatCompletions::adapt_request`（`crates/aigw-core/src/adapter.rs:1990-2066`）的 5 处转换缺口，使 Codex 全流程（chat → tool call → streaming）可用。

### 验收标准

- [ ] Codex CLI `codex exec "..."` 接 aigw 返回正常文本回复（含 tool 声明）
- [ ] 既有 `responses.feature` 全部场景（改写的除外）保持绿
- [ ] `task test` / `task test-bdd` / `task fmt` / `task lint` 全绿

> **多轮 tool call 不在本 Stage 验收范围**——依赖 `input[].type` 分派（§8.2 / TD-017a）。代码侧不做半吊子实现，避免产出「看似支持多轮但实际走偏」的假象；TD-017e 记录该验证缺口。

### 明确不做（边界）

- **不改上游 MaaS**（不认 `developer` role 是上游约束，网关侧适配）
- **不降级 Codex**（实测 chat wire 路径同样 400，见 §2.5；且 0.92.0 与现版本跨 65 个版本）
- **不做 `input[].type` 分派**（`function_call` / `function_call_output` 等 item type）——单独立项，见 §8
- **不做内建搜索执行**（web_search 真实执行需搜索后端 + agentic loop，独立 Phase，见 §3.1 与调研 §3.2）
- **不改 `ChatTemplateCompat` 机制本身**，仅接入 Responses 桥接

---

## 2. 现状证据

调研全文：`docs/research/2026-10-05-codex-responses-bridge-gap.md`。以下为 5 处缺口的实测与源码定位。

### 2.1 缺口 A — 非 function 工具硬拒绝

`adapter.rs:2005-2019`：遇到任何非 `function` 类型**直接 `return Err(AdapterError::Unsupported(...))`**，整请求失败。

Codex 实际发送（抓包）：`function` × 7 + **`namespace`（`multi_agent_v1`，内含 5 个嵌套 function）** + **`web_search`**。

BDD 固化了此行为：`responses.feature:93` "web_search tool rejected"、`:110` "code_interpreter tool rejected"。

### 2.2 缺口 B — `role: "developer"` 未处理

`grep -rn '"developer"' crates/**/*.rs` → **零命中**。`ChatMessage.role: String`（无枚举约束，`models.rs:574`），`adapter.rs:1977` 原样拷贝 role。上游 MaaS 拒收。

实测（生产网关 `9.135.87.221:4001`）：

```
msg[0] role=system     -> 200
msg[1] role=developer  -> 400   ← 单条即可复现
```

OpenAI 官方文档明确 `developer` 与 `instructions` **语义等价**——aigw 已支持 `instructions`→首位 system（`adapter.rs:2028-2035`），只是 `developer` 入口未走同一条路。

### 2.3 缺口 C — 多个 system 会踩 Qwen 严格模板

Codex 消息形状恒为 `[system(instructions), developer, user...]`。**朴素重命名会产出 `[system, system, user...]`**。

aigw 已有机制但**未接入本路径**：

| 机制 | 位置 | 是否接入 Responses 桥接 |
|------|------|----------------------|
| `ChatTemplateCompat`（auto-sniff qwen） | `adapter.rs:305-330` | ❌ |
| `fold_extra_systems_into_adjacent_user` | `adapter.rs:345-394` | ❌（仅 `AnthropicToOpenAI::adapt_request:170-183` 调用） |

> 注意：deepseek/glm 上游**不报错**（实测 `两个 system 开头 -> 200`），故此缺陷在本网关现网**测不出来**，仅在 Qwen 系上游暴露。若不做归一化即为埋雷。

### 2.4 缺口 D — 扁平 function 工具从未转嵌套（既存路径的真实 400）

`adapt_request` 的 `"function" => {}` 分支**只校验不转换**，工具对象原样带进上游。实测：

```
扁平 {"type":"function","name":"foo",...}                    -> 400  tools[0].function.name is invalid or missing
嵌套 {"type":"function","function":{"name":"foo",...}}        -> 200
```

**aigw 当前「允许」的 function 工具，打到真实上游同样 400。** 被 mock BDD 掩盖（`responses.feature:85` 断言 200，但 mock 不校验 tool 形状）。

### 2.5 缺口 E — `input_text` content part 未映射

Respondes part 用 `{type:"input_text"}`，Chat 要 `{type:"text"}`。实测：

```
content=[{"type":"input_text","text":"hi"}]  -> 400
content=[{"type":"text","text":"hi"}]        -> 200
```

`adapter.rs:1974-1980` 的 `input_to_messages` 对 `content` 做 `cloned()` 原样拷贝，无 part 级映射。

### 2.6 旁证：降级 Codex 到 chat wire 不可行

实测 Codex 0.92.0（最后一个支持 `wire_api = "chat"` 的版本）直连 `/v1/chat/completions`：**同样** 400，且同一根因（`role="developer"`）。官方 [discussion #7782](https://github.com/openai/codex/discussions/7782) 确认 chat wire 已于 2026-02 移除。

**→ 修复点必须在网关，工具/role 归一化无法绕开。**

---

## 3. 方案

### 3.1 工具归一化（新增 `normalize_responses_tools`）

将现有 `adapter.rs:2005-2019` 的「校验即拒绝」改为「归一化」：

```
for tool in tools:
    match tool.type:
        "function"  -> 扁平转嵌套 {type:"function", function:{name, description, parameters, strict}}
        "namespace" -> 拍平子 function 为顶层，名 "{ns}__{child}"
        "custom"    -> 降级为 function（Codex exec 工具的形态）
        "tool_search" -> 降级为同名 function 代理（Codex 侧按名字调用，不能改名）
        _ (web_search / web_search_preview / file_search / code_interpreter /
           computer_use / image_generation / shell / mcp)
                    -> 丢弃 + tracing::warn
```

**关于 `mcp`**：litellm 对 `mcp` 是原样透传（`:1853`），但**本环境实测 `tools=[{type:"mcp",...}]` → 400**——故 aigw 归入「丢弃」而非「透传」。

**关于 `web_search` 的处置决策（选「丢弃 + 告警」）**：完整依据见 `docs/research/2026-10-05-websearch-server-tool-support.md`。核心三点：

1. **透传在本环境不可用**——实测 `web_search` / `web_search_preview` / `code_interpreter` / `computer_use_preview` / `mcp` 五种形态打到上游**全部 400**。litellm 兜底与 new-api 的「透传」路线在此行不通。
2. **派生 `web_search_options` 在本环境无实效**——参数被上游收下（200）但**不执行搜索**：同一联网问题，带与不带该参数的模型回复都是「我无法联网搜索」。
3. **故「丢弃」与「派生」实效相同**（都无搜索），差别仅在「是否在上游请求里留一个骗人的参数」。选丢弃 + `tracing::warn`：诚实反映能力缺失，且告警日志可统计「多少请求本来要搜索」，作为是否值得内建搜索的决策依据。

**内建搜索（真正能搜）不在本 Stage**：需搜索后端 + 多轮 agentic loop + SSE 交互，是独立子系统（litellm 的 `WebSearchInterceptionLogger` 路线），量级远超 12h 缺陷修复，见调研 §3.2。

**对齐依据**（三家参考实现，详见调研）：

| 仓库 | function | namespace | 服务端工具 |
|------|----------|-----------|-----------|
| litellm `transformation.py:1851-1912` | 转嵌套 | `ns__name` 拍平 | 丢弃 + `verbose_logger.warning` |
| sub2api `chatcompletions_responses_bridge.go:800-860` | 转嵌套 | `ns__name` 拍平 | 丢弃（注释明示） |
| new-api `to_oai_chat_req.go:321-357` | 转嵌套 | 透传 | 透传（Custom） |

**取舍：选「丢弃 + 告警」而非 new-api 的透传。** 理由：服务端工具在 chat 上游无对应能力，透传必然被上游 400（失败推给上游，用户看到的是上游错误而非网关降级），体验更差。

#### 3.1.1 拍平命名与冲突检测

```
flat_name = "{namespace}__{child_name}"
```

与 litellm（`:1790`）/ sub2api（`responses_namespace.go`，`"<namespace>__"` 前缀）**逐字符一致**。

冲突处理（两种冲突都必须**返回 400 明确报错**，不得静默覆盖）：

1. 拍平名与顶层 function 名冲突
2. 两个 namespace 拍平出同名

litellm 抛 `ValueError("Top-level function names conflict with flattened namespace tools: ...")`（`:1843-1848`）；sub2api 报 `"namespace tool %q/%q flattens to %q which conflicts with a top-level tool of the same name; this upstream cannot disambiguate them, rename one of the tools"`。**aigw 采用同样语义**——静默覆盖会让模型调用落到错误工具，是正确性事故。

#### 3.1.2 `tool_choice` 同步清理

丢弃工具后，指向已丢工具的 `tool_choice` 必须一并丢弃——否则 chat 上游因「tool_choice 指向未声明工具」400。

sub2api 已显式踩过此坑并注释（`chatcompletions_responses_bridge.go:941-944`）：

> 服务端工具（web_search 等）的选择项随工具本身丢弃——指向未声明工具的 tool_choice 会被 chat 上游 400 拒绝。

实现：工具归一化产出 `declared: Set<String>`，`tool_choice` 具名形态（`{type:"function"|"namespace", name}`）仅在该名存在于 `declared` 时保留，否则丢弃（`auto`/`none`/`required` 字符串形态原样转发）。

### 3.2 Role 归一化（developer → system，默认启用 + 可配置开关）

**决策：默认启用，但设计成可配开关。** 你的提问正确——若后端模型原生支持 `developer`，映射是多余的。

#### 3.2.1 默认行为

在 `input_to_messages`（`adapter.rs:1964-1987`）后、`instructions` 插入前，对 messages 做 role 归一：

```
if developer_to_system_enabled:
    role = if role == "developer" { "system" } else { role }
```

**三家参考实现完全一致，无分歧**：

| 仓库 | 实现 | 位置 |
|------|------|------|
| litellm | `map_developer_role_to_system_role` | `base_utils.py:207-224`（`main.py:5389` 对所有非 OpenAI provider 调用） |
| new-api | `case "system", "developer":` 合并分支 | `to_oai_responses_req.go:129` 等 3 处 |
| sub2api | `chatCompletionsBridgeRole` | `chatcompletions_responses_bridge.go:689-697` |

**litellm 已经证明了「按后端能力决定是否映射」的可行性**：`translate_developer_role_to_system_role` 是 base 方法，**被 OpenAI/Azure 覆写为不映射**（`base_llm/chat/transformation.py:150-159` + `openai/chat/o_series_transformation.py:40`）——即 litellm 对真 OpenAI 保留 `developer`、对其他 provider 才降级。

#### 3.2.2 配置字段设计

**字段名**: `developer_role_passthrough`（放在 `proxy_models.model_info`，与既有 `chat_template_compat` **完全同构**）

| 值 | 语义 |
|----|------|
| **缺省（absent）** | **默认映射**（developer → system）——安全默认，兼容绝大多数非 OpenAI 上游 |
| `true` | **透传 developer**（不映射）——用于确认后端原生支持的上游 |
| `false` | 显式映射（等价于缺省，允许显式声明） |

**为何默认映射（而非默认透传）**：实测本环境的 MaaS 上游**拒收** `developer`（400）。若默认透传，则每个未显式配置的部署都会失败——**默认值必须选「对绝大多数上游可用」的那一侧**。这与 `chat_template_compat` 的 `auto` 嗅探逻辑一致（`resolve_chat_template_compat`，`adapter.rs:320-330`）。

**为何不做「按 `custom_llm_provider` 自动嗅探真 OpenAI」**：`ProviderType::infer`（`deployment.rs:114-131`）把所有非 `anthropic` 值都归为 `OpenAICompatible`——`"openai"` / `"deepseek"` / `"dashscope"` 走同一分支（生产 `config.yaml` 实测含这全部前缀）。aigw **没有** litellm 那种「每个 provider 声明自己能力」的注册表，无法可靠区分「真 OpenAI」与「OpenAI 兼容」。故用**显式配置**替代嗅探。

**与 `chat_template_compat` 的对称性**：两者都是「网关侧转换行为标记」，都放 `model_info`，都可显式覆写。区别是后者用名字嗅探（qwen）作 auto，本字段无可靠嗅探信号故缺省即定值。

#### 3.2.3 交互：映射后仍须归一 system 位置

**无论是否映射，`developer` 的 `system` 类语义都要合并进首位 system**（§3.3）——这是两个独立维度：

| 维度 | 决定 |
|------|------|
| role *名字* 用 `developer` 还是 `system` | `developer_role_passthrough`（本开关） |
| `system`/`developer` 类消息*放哪* | 恒为「合并进首位 system」（§3.3，无开关） |

即：`passthrough=true` 时，`developer` 保持原名但**仍在首位 system 槽附近**；`passthrough=false`（默认）时改名为 `system` 并合并。

### 3.3 多 system 归一（接入既有折叠机制）

**核心设计决策：避免产生第二个 system，而非产生后再折叠。**

前置 `instructions` 插入（`adapter.rs:2028-2035`）+ creator 的 `developer` 归一后会产出 `[system, system, ...]`。两种处置：

| 方案 | 做法 | 保真度 |
|------|------|--------|
| **M1（推荐）** | `developer` 内容**合并进首位 system**（与 `instructions` 同槽，`\n\n` 连接） | 高——developer 是权限/沙箱策略指令，保持 system 级优先级 |
| M2 | 产出两个 system 后调 `fold_extra_systems_into_adjacent_user` | ⚠️ 内容进 user turn 的 `<system-reminder>`，**降权** |

**推荐 M1**，理由：OpenAI 文档明确 `developer` 与 `instructions` 等价（同为「应用/开发者级指令」），二者合并进同一 system 槽语义正确；且从根上不产生第二个 system，与 `ChatTemplateCompat` 模式解耦（Loose 模式也不会漏）。

**M1 的合并顺序**：`instructions` 在前，`developer` 在后（后者更具体，靠后符合「具体覆盖一般」）。

**兜底**：即便采用 M1，仍应在 `adapt_request` 末尾对消息做一次 `system 仅 index 0` 断言（沿用 `fold_extra_systems_into_adjacent_user` 内的 `debug_assert` 思路）——防御 `input[]` 里客户端自带的额外 system。

### 3.4 content part 映射

`input_to_messages`（`adapter.rs:1974-1980`）在拷贝 `content` 时做 part 级转换：

```
content[] 各 part：{type:"input_text"|"output_text"} -> {type:"text"}
                   {type:"text"}                     -> 原样
                   其他（image_url 等）                 -> 原样（多模态不属本 Stage）
```

`role` 由数组元素提供（当前实现正确），仅 part 需映射。

> **`input[]` 元素自带 `type` 字段**（`"message"` / `"function_call"` / `"function_call_output"`）当前被完全忽略。单轮不触发；**多轮 tool 回填会走偏 —— 本 Stage 不做，登记 TD**（§8）。

---

## 4. TDD 计划

### 4.1 适配器级 UT（`crates/aigw-core/src/adapter.rs` 新增）

> 注：Phase 41 遗留 TD 记录「Stage 102 计划的 19 个适配器 UT 未落地」。本 Stage 顺带补齐本缺口相关的 UT。

| UT | 输入 | 期望 |
|----|------|------|
| `test_responses_to_chat_tools_flatten_function` | 扁平 function | 转嵌套 `{type,function:{name,...}}` |
| `test_responses_to_chat_tools_namespace_flattened` | 1 个 namespace + 2 子 function | 拍平 2 条，名 `ns__child` |
| `test_responses_to_chat_tools_namespace_name_collision` | namespace 拍平名与顶层 function 重名 | `Err(Unsupported)` |
| `test_responses_to_chat_tools_server_side_dropped` | `web_search` + `code_interpreter` | 丢弃，剩余 tools 正常 |
| `test_responses_to_chat_tool_choice_dropped_with_tool` | `tool_choice` 指向被丢的 `web_search` | `tool_choice` 一并丢弃 |
| `test_responses_to_chat_tool_choice_kept_for_surviving` | `tool_choice` 指向幸存 function | 保留 |
| `test_responses_to_chat_developer_role_becomes_system` | `input[0]=developer` | 产出中无 `developer` role（默认） |
| `test_responses_to_chat_developer_passthrough_when_configured` | `model_info.developer_role_passthrough=true` | `developer` **保留原名** |
| `test_responses_to_chat_developer_merged_into_leading_system` | `instructions` + `developer` | 单条 index-0 system，含二者内容 |
| `test_responses_to_chat_no_second_system` | 完整 Codex 形状 | 断言 `messages[1..]` 无 `role=="system"` |
| `test_responses_to_chat_input_text_part_mapped` | `content:[{type:"input_text"}]` | 转 `{type:"text"}` |
| `test_responses_to_chat_codex_fixture_end_to_end` | **真实 Codex 抓包 body 固化为 fixture** | 无 400，产出结构合法 |

**最后一条是本 Stage 的核心验收**：以 §2 抓包的完整 Codex 请求体（含 8 个 tools + system/developer/user 三层消息）作为回归 fixture，锁死「Codex 原生请求必能过桥」。

### 4.2 BDD（`crates/aigw-server/tests/features/responses.feature`）

**改写 2 条**（BC 破坏，需在评审中确认）：

| 原场景 | 改写为 |
|--------|--------|
| `:93` web_search tool rejected（断言 400 + "not supported"） | web_search tool dropped（断言 **200** + 请求正常） |
| `:110` code_interpreter tool rejected | 同上 |

**新增 4 条**：

- `/v1/responses` bridge with namespace tool（→ 200，mock 断言上游收到拍平后的 function 列表）
- `/v1/responses` bridge with developer role（→ 200，mock 断言上游 `messages` 无 `developer`）
- `/v1/responses` bridge with developer passthrough（配置 `developer_role_passthrough=true` → mock 断言上游仍见 `developer`）
- `/v1/responses` bridge codex-shaped request（tools + developer + input_text 组合 → 200）

**mock 断言缺口需一并补**：现有 mock upstream 不校验 body 形状（`bdd_support/mock_upstream.rs`），这正是缺口 D 被掩盖的原因。本 Stage 需让 mock 暴露「最近一次请求体」供 then-step 断言。

### 4.3 集成验证（人工）

`codex exec --skip-git-repo-check "reply with exactly PONG"` 接 aigw 返回 `PONG`（当前 400）。

---

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | `ResponsesToChatCompletions::adapt_request` 接工具归一化 + role 归一 + part 映射；新增 `normalize_responses_tools` / `flatten_namespace_tools` / 冲突检测；13 个 UT |
| `crates/aigw-core/src/deployment.rs` | `Deployment` 新增 `developer_role_passthrough: Option<bool>`（与 `chat_template_compat` 同构） |
| `crates/aigw-core/src/resolver.rs` | 从 `model_info` 解析 `developer_role_passthrough`（照搬 `chat_template_compat` 的提取模式，`resolver.rs:291-296`） |
| `crates/aigw-server/tests/bdd_steps/model_steps.rs` | 承载新配置字段的 BDD step |
| `crates/aigw-server/tests/features/responses.feature` | 改写 2 条 + 新增 4 条场景 |
| `crates/aigw-server/tests/bdd_steps/responses_steps.rs` | 新增/改写 step；mock 请求体断言 step |
| `crates/aigw-server/tests/bdd_support/mock_upstream.rs` | 暴露「最近一次上游请求体」供断言 |
| `crates/aigw-core/src/adapter.rs`（fixture） | Codex 抓包 body 固化常量 |
| `docs/stages/stage-131.md` | 本文件 |
| `docs/research/2026-10-05-codex-responses-bridge-gap.md` | 桥接调研文档（前置，已落盘） |
| `docs/research/2026-10-05-websearch-server-tool-support.md` | web_search 业界调研（已落盘，后续规划参考） |
| `docs/stages/stage-roadmap.md` | 追加 Phase 52 + 修订记录 |
| `docs/11-next-steps.md` | Phase 52 / Stage 131 回写 |
| `docs/12-technical-debt.md` | 登记 TD-017（input item type 分派） |

---

## 6. 回归验证

1. `task test` 全绿（含 13 个新 UT）
2. `task test-bdd` mock BDD 场景数净增 3（改写 2 条不变），0 fail
3. `task bdd-real-sqlite` 通过（本 Stage 不涉 DB，仅确认无回归）
4. `task fmt` / `task lint` green
5. 人工：Codex exec 接 `9.135.87.221:4001` 返回正常回复

> 注：任务名为 `task bdd`（非 `task test-bdd`）。

---

## 7. 门禁

- [x] 13 个新 UT 先 fail 后 pass（TDD 红绿）——`test_responses_to_chat_no_second_system` 与 `..._developer_passthrough_when_configured` 初次红，修正后绿
- [x] Codex 抓包 fixture 回归 UT 通过（`test_responses_to_chat_codex_fixture_end_to_end`）
- [x] `task test` / `task bdd` / `task fmt` / `task lint` 全绿
- [x] BDD 改写 2 条（rejected → dropped，上游体断言替代原错误串断言）
- [ ] `docs/11-next-steps.md` + `stage-roadmap.md` 回写（待回写）
- [ ] git commit（精确 add，禁 `-A`/`.`；`--signoff`）

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 缓解 |
|------|------|
| **丢弃服务端工具导致能力静默缺失** | `tracing::warn` 明确记录工具名与原因（litellm 同款）；后续若需支持 `web_search`，走派生为 `web_search_options` 参数（litellm 路线，`transformation.py:1855-1865`） |
| **BC 破坏**：web_search/code_interpreter「rejected」断言被改写 | 改写为「dropped」，语义仍可验证（请求 200 + 上游无该工具） |
| **多 system 在 Loose 模式下仍可能出问题** | M1 方案（合并进首位 system）从根上不产生第二个 system，与 compat 模式解耦 |
| **上游 MaaS 对 `parameters` 结构挑剔** | 实测 Codex 的嵌套 tool（含 `strict`）已被网关接受（§2.4 嵌套 200） |

### 8.2 遗留（本 Stage 不做，登记 TD-017）

- **`input[].type` 分派**：`function_call` / `function_call_output` / `reasoning` / `local_shell_call` 等 item type 当前一律按 message 处理。**单轮可用，多轮 tool 回填会走偏。** Codex 多轮必需，建议独立 Stage（参照 sub2api `buildChatMessagesFromItems` 的 item 分派 + `normalizeChatMessages` 的 tool_call/tool 配对清理，`chatcompletions_responses_bridge.go:187-215 / 576-640`）。
- **`tool_choice` 的 `{type:"namespace"}` 形态**：Codex 会发 `tool_choice: {type:"function", name, namespace}`，命名空间字段的剥离需在 8.2 项一并处理。
- **`web_search` 真实支持**：当前丢弃；若产品需要，走 litellm 的派生路线。
- **Phase 41 遗留适配器 UT 缺口**：本 Stage 补 13 个，剩余（streaming 事件映射等）仍待补。
