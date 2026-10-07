# Stage 135: 搜索接线三条入口 + prompt 注入（Phase 53）

**所属**: Phase 53（内建 Web Search / TD-017c）
**预估**: 12h（触发检测 ×3 + 注入模板 + context_size 映射 + `ClaudeToolDef` 缺陷修复 + 降级策略 + UT/BDD）
**依赖**: Stage 134（搜索后端抽象层 —— `SearchProvider` / `WebSearchRegistry` / `SearchRequest` / `SearchResponse` / `SearchError`）
**状态**: ✅ 完成（2026-10-07）

> **Phase 53 范围收窄（2026-10-06 决策）**：多 provider 架构**全部保留**（trait / registry / `kind` 判别式 / 配置 / failover / per-provider `timeout_ms` / `api_key` + `v2:gcm:` / `cost_per_query`），但**本期只实现 SearXNG 一家**；Tavily 与博查 Bocha 不落地（Stage 134 已据此收窄：`tavily.rs` / `bocha.rs` 不写，多态与 failover 由**测试专用 `StubProvider`** 证明）。配置为**两层模型**：provider（逻辑后端，持 `kind` 与定价）→ instances[]（物理端点，各自 `base_url` / `weight` / `enabled` / 冷却，选择逻辑照抄 `router.rs`）。
>
> 对本 Stage 的影响面**很小**：三条入口的触发检测是**协议层行为、与 provider 无关**（§3.2 表逐字不变）；`registry.search()` 内部选哪个 provider / 哪个实例对接线层透明。唯一实质变化是 §3.7 的 TTFT 权衡从「可换 provider」变成「只有一个选项」。

---

## 1. 目标

Stage 134 交付了可独立测试的搜索后端，但**没有任何调用方**。本 Stage 把它接到三条请求入口，按**设计 C（prompt 注入 / search-then-inject）**落地：发往上游**之前**执行搜索 → 把结果按模板注入最后一条 user 消息 → **照常单次调用上游** → 上游 SSE 原样透传。

同时修掉一个**与 web search 无关但被 web search 暴露**的既有缺陷：`ClaudeToolDef`（`models.rs:1065-1071`）的 `input_schema` 是非 Option 必填字段且 struct 无 `type` 字段，带 `web_search_20250305` 的 Anthropic 请求在 `serde_json::from_value` 阶段整条失败 → **HTTP 500**，而非 Responses 侧那样的优雅降级。

### 验收标准

- [ ] 三条入口的触发检测各自生效（§3.2 表），命中时 `tracing::warn!` 的「silent drop」不再发生
- [ ] `search_context_size` low/medium/high → 1/3/5 条结果映射生效，且**不能上调**超过 `WebSearchConfig.max_results`（Stage 134 §3.5 护栏）
- [ ] 注入目标为**最后一条 user 消息**；Stage 131 建立的「有且仅有一条前导 system」不变量**零破坏**（UT 锁定）
- [ ] `ClaudeToolDef` 修复后，带 `web_search_20250305` 的 `/v1/messages` 请求**不再 500**：未配置搜索时降级为丢弃 + warn（与 Responses 侧一致），已配置时执行搜索并注入
- [ ] 搜索失败（`SearchError` 任一变体）→ **降级放行**（不带搜索继续调上游）+ `tracing::warn!` + 响应 metadata 标记，**绝不把用户请求打挂**
- [ ] 流式路径（`responses.rs:1034-1079`）**零改动**，SSE 透传行为与 Stage 133 基线逐字节一致
- [ ] `web_search` 配置缺省（absent）时**三条入口行为与 Stage 134 基线完全一致**（即丢弃 + warn），`task bdd` 场景数只增不改
- [ ] **触发搜索的请求绕过 exact-match 缓存**（§3.6a(b)）—— 不白搜、不返回注入前的陈旧体
- [ ] **OAuth 反代路径行为与 Stage 135 之前逐字一致**（§3.6a(a) 的显式边界，Anthropic 原生搜索由上游执行）
- [ ] `task test` / `task bdd` / `task fmt` / `task lint` / `task doctor` 全绿

### 明确不做（边界）

- **不做短路（设计 A）**：不合成任何 Anthropic `server_tool_use` / `web_search_tool_result` 块。理由：`encrypted_content` 由服务端签发、网关**无法伪造**（架构调研 §4.4）；自造块在多轮回放时会投毒历史 → 上游 400（§5.7）。Claude Code 的 WebSearch 按钮因此**本 Stage 仍不可用** → §8.2
- **不做 agentic loop（设计 B）**：无循环、无指纹防环、无 `max_agentic_loops` → §8.2（且其硬前提未实测）
- **不做 query 改写 / 是否需要搜索的前置 LLM 判断**（Higress `searchRewrite` 多一跳 LLM）
- **不合成 Responses `web_search_call` item**（`ResponsesToChatCompletionsStream` 虽有该能力，见 codebase-map §A5，但那是设计 A/B 的形状；C 不需要）
- **不做结构化 citations**（`url_citation` annotations / Anthropic `citations`）—— 用 markdown 链接引用替代，有 OpenRouter 默认 `search_prompt` 先例（架构调研 §5.4 末）
- **不碰 provider 客户端**（Stage 134 已交付，本 Stage 只调 `WebSearchRegistry`）；**本期可用的生产 provider 只有 SearXNG 一家**，Tavily / Bocha 的接入是 Stage 134 抽象之上的纯增量，不在本 Stage
- **不做计费 / SpendLog / `cost_per_query` 消费**（→ Stage 136）；本 Stage 只在响应 metadata 留标记，不写任何 spend 行
- **不做前端**（→ Stage 137）
- **不改 `responses.rs` 流式循环**（`:1034-1079` 是三次生产事故现场，§3.7 专述）
- **不新建 DB 表 / migration**

---

## 2. 现状证据

### 2.1 丢弃点一：Responses 工具归一化的 `other =>` 分支

`crates/aigw-core/src/adapter.rs:2553-2559`（函数 `normalize_responses_tools` 声明于 `:2447`）：

```rust
                other => {
                    tracing::warn!(
                        tool_type = %other,
                        tool_name = %name,
                        "dropping Responses API tool: no Chat Completions equivalent"
                    );
                }
```

函数 doc 注释 `adapter.rs:2446` 已自陈：`server-side tools (web_search, code_interpreter, mcp, ...) → dropped`。指向被丢弃工具的 `tool_choice` 被级联清理（`adapter.rs:2714-2724` 调 `normalize_tool_choice(&choice, &declared_tools)`，`:2716`），已有 UT 固化此行为（`adapter.rs:4206`、`:4219-4220`、`:4666`、`:4689`）。

### 2.2 丢弃点二：历史 item 的 `web_search_call` 回放

`crates/aigw-core/src/adapter.rs:2210-2216`（Stage 132 新增的 item 分派 `items_to_messages`，声明于 `:2144`）：

```rust
                other => {
                    // Server-side call items (`web_search_call`, `local_shell_call`,
                    // `file_search_call`, ...) have no Chat equivalent. Converting
                    // them would inject a stray message between an assistant's
                    // tool_calls and its replies, which strict upstreams reject.
                    tracing::debug!(item_type = %other, "skipping Responses item with no Chat equivalent");
                    pending_reasoning.clear();
                }
```

已有 UT 固化（`adapter.rs:4565-4572`，`..._item_unknown_type_skipped`）。

### 2.3 Anthropic 侧：`web_search_20250305` 在反序列化阶段就炸 → HTTP 500（**已核实**）

`crates/aigw-core/src/models.rs:1065-1071`：

```rust
pub struct ClaudeToolDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,   // ← 非 Option，必填；且 struct 无 `type` 字段
}
```

Anthropic 服务端工具的线上形态 `{"type":"web_search_20250305","name":"web_search","max_uses":5}` **不带 `input_schema`**。失败链路逐跳核实：

| 跳 | 位置 | 行为 |
|----|------|------|
| 1 | `adapter.rs:230-232` | `serde_json::from_value::<ClaudeMessageRequest>(body)` → `Err` → `AdapterError::Parse("Invalid Claude request: missing field `input_schema`")` |
| 2 | `adapter.rs:61-64` | `AdapterError` 只有 `Unsupported` / `Parse` 两个变体 |
| 3 | `v1_messages.rs:858-866` | `.map_err(\|e\| anthropic_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", ...))` |
| 3' | `chat.rs:1534-1540` | 同样 `StatusCode::INTERNAL_SERVER_ERROR` + `"adapter_error"`（`:1537`） |
| 3'' | `responses.rs:617-619` | `AdapterError::Parse(_) => (StatusCode::INTERNAL_SERVER_ERROR, "adapter_error")`（`:618`） |

**→ 结论成立：`Parse` 在三条路由全部映射到 500。** 这是一个**先于 web search 存在的缺陷**：任何不带 `input_schema` 的 Anthropic 工具声明都会 500，与 Responses 侧「丢弃 + warn」的优雅降级**行为不一致**。`AnthropicPassthrough` 路径（`adapter.rs:1613-1670`）同样先 `from_value::<ClaudeMessageRequest>`（`:1619-1620`）再 `to_value(&req)`（`:1660-1661`）—— 即便字段能解析，round-trip 也会丢掉 `type` / `max_uses`（`models.rs` 全文无 `#[serde(flatten)]`）。

工具转换本体在 `adapter.rs:922-934`（`parameters` 赋值在 `:930`）：`ct.input_schema.clone()` 直接喂给 `ToolDefFunction.parameters`，`input_schema` 改 Option 后此处需跟改。

### 2.4 Chat 侧：`web_search_options` 不是「丢弃」，是**静默透传给上游**

`OpenAIPassthrough::adapt_request`（`adapter.rs:147-160`）只改 `model` 与 `stream_options`，**body 其余字段原样转发**；`ChatCompletionRequest`（`models.rs:512-540`）无 `web_search_options` 字段但 chat 路径不走强类型反序列化（`chat.rs:1534` 传的是 `Value`）。

**→ 当前 Chat 侧的行为比「丢弃」更糟：把一个 OpenAI 原生参数直接丢给不认识它的 MaaS 上游**，由上游决定是忽略还是 400。本 Stage 必须把它**消费掉并移除**，不让它继续泄漏到上游。

### 2.5 Stage 131 建立的「有且仅有一条前导 system」不变量

`ResponsesToChatCompletions::adapt_request`（`adapter.rs:2686`）的流水线顺序：

| 步 | 行 | 操作 |
|----|----|------|
| 1 | `:2703-2711` | `normalize_responses_tools` — **丢弃点一在此** |
| 1b | `:2714-2724` | `normalize_tool_choice`（`:2716`）级联清理 |
| 2 | `:2727-2730` | `input_to_messages` → `items_to_messages`（**丢弃点二在此**） |
| 3 | `:2732-2740` | `instructions` → `messages.insert(0, system)` |
| 3b | `:2744` | `consolidate_system_messages`（`:2410`）—— 把**所有** system 折叠成**唯一一条前导** |
| 3b | `:2746` | `merge_developer_into_system`（`:2357`）—— developer 并入该 system 槽 |
| 4-5 | `:2749+` | 字段改名 + 丢弃不支持字段 |

`consolidate_system_messages` 的 doc（`adapter.rs:2404-2409`）明示理由：**严格 Jinja 模板（Qwen 系）拒收 index 0 之后的 system**。Anthropic 侧的 system 更是一个**独立顶层字段**而非消息（`models.rs:1043` `system: Option<ClaudeSystemMessage>`，`models.rs:1074-1079` 该枚举是 `Text(String) | Blocks(Vec<ClaudeContentBlock>)` 的 untagged 二形态）。

### 2.6 流式路径 —— 三次生产事故的同一现场

`crates/aigw-server/src/routes/responses.rs:1034-1079`：`while let Some(chunk_result) = stream.next().await` 循环内 `pending_chunk.extend_from_slice(&chunk)` → `stream_adapter.next(&pending_chunk)` → `tx.send(transformed)` → `pending_chunk.clear()`，循环后 `stream_adapter.finish()`。涉及类型：`StreamAdapter` trait（`adapter.rs:55-57`，`next(&mut self, chunk: &[u8]) -> Option<Vec<u8>>` + `finish`）、`ResponsesToChatCompletionsStream`（状态机 `adapter.rs:2934-2946`，`ToolCallState` 于 `:2949`，`next` 于 `:3051`、`finish` 于 `:3229`）、`PassthroughStream`（`adapter.rs:171-178`）。

| commit | 事故 |
|--------|------|
| `c3f360c` | 流式桥接**死循环**，请求永不结束（`next` 不是队列，drain loop 永不返回 `None` —— 该教训已逐字写进 `adapter.rs:49-54` 的 trait doc） |
| `4bd85c7` | 在 `response.completed` **之前**就发 `[DONE]`，客户端提前断流 |
| `8cf8c12` | 流式 SpendLog 只存最后一个 chunk，响应内容恒为空 |

`docs/stages/stage-133.md` §8 已把此处登记为**高风险区**并点名「Stage 135 已登记此警示」。

---

## 3. 方案

### 3.1 代码落点

| 文件 | 新增/改动 |
|------|----------|
| `crates/aigw-core/src/websearch/trigger.rs` | **新增** — 三条入口的触发检测（纯函数） |
| `crates/aigw-core/src/websearch/inject.rs` | **新增** — 注入模板渲染 + 目标消息定位（纯函数） |
| `crates/aigw-core/src/websearch/mod.rs` | `pub mod trigger; pub mod inject;` + re-export |
| `crates/aigw-core/src/models.rs` | `ClaudeToolDef` 修复（§3.3） |
| `crates/aigw-core/src/adapter.rs` | 两处丢弃点改为「识别 + 置标记」（§3.4）；claude→openai 工具映射跟改 Option |
| `crates/aigw-server/src/routes/{chat,responses,v1_messages}.rs` | adapt 之前插入搜索执行 + 注入（§3.6） |

**纯函数 / IO 分层沿用 Stage 134 §3.1 的强制切法** —— 触发检测与模板渲染全是纯函数（UT 主战场），唯一 IO 是 `WebSearchRegistry::search()`（Stage 134 已交付且已有 UT）。

### 3.2 触发检测（三条入口）

| 入口 | 触发形态 | 检测位置 | 命中后对原字段的处理 |
|------|---------|---------|-------------------|
| **OpenAI Chat** `/v1/chat/completions` | 顶层参数 `web_search_options`（可带 `{"search_context_size":"low\|medium\|high"}`；空对象 `{}` 也算命中） | `trigger::detect_chat(&body)` | **移除** `web_search_options`（§2.4：否则泄漏给上游） |
| **OpenAI Responses** `/v1/responses` | `tools[]` 含 `{"type":"web_search"}` 或 `{"type":"web_search_preview"}` | `trigger::detect_responses(&tools)`，在 `normalize_responses_tools` 之前调用 | 该 tool 仍按既有逻辑**从上游 tools 中移除**（上游不认识它），`tool_choice` 级联清理不变 |
| **Anthropic** `/v1/messages` | `tools[]` 含 `type == "web_search_20250305"`（`max_uses` 一并读出，仅作上限提示） | `trigger::detect_anthropic(&tools)`（需先完成 §3.3 修复才读得到 `type`） | 该 tool **从转发给上游的 tools 中移除** |

三者统一产出同一个中间结构：

```rust
pub struct SearchTrigger {
    /// 命中的入口形态，进日志与 Stage 136 的归属。
    pub surface: TriggerSurface,          // Chat | Responses | Anthropic
    /// 客户端请求的结果条数上限（来自 search_context_size / max_uses）。
    /// 服务端会再 clamp 到 WebSearchConfig.max_results（Stage 134 §3.5）。
    pub requested_results: usize,
}
```

**`web_search` 与 `web_search_preview` 等价对待** —— 后者是 OpenAI 的 preview 别名，既有 UT `adapter.rs:4188-4189` 已把两者并列测过。

### 3.3 `ClaudeToolDef` 修复（§2.3 的缺陷，与 web search 正交）

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeToolDef {
    /// 服务端工具标识（`web_search_20250305` / `code_execution_*` / ...）。
    /// 客户端工具不带该字段 → None。
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub tool_type: Option<String>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 服务端工具不带 schema → None（原为非 Option，导致整条请求 500，§2.3）。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub input_schema: Option<serde_json::Value>,
    /// Anthropic 服务端工具的搜索次数上限（`web_search_20250305` 携带）。
    /// §3.4 取 `min(max_uses, max_results)` 当结果条数上限；缺省（None）时不参与。
    /// 不加此字段则 §3.2「读 max_uses」与 §3.4 的 clamp 无法实现（Review F2）。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub max_uses: Option<i64>,
}
```

> **Review F2**：设计初稿只加 `tool_type` / `input_schema` 两个字段，却要求 §3.2/§3.4 读取 `max_uses` —— 而 `models.rs` 全文无 `#[serde(flatten)]`（§2.3 自陈），未声明字段在反序列化时被静默丢弃，故 `max_uses` 必须显式声明。加 `#[serde(skip_serializing_if = "Option::is_none")]` 保证客户端工具（无该字段）序列化不变。

连带改动：`adapter.rs:930` 的 `parameters: Some(ct.input_schema.clone())` → `parameters: ct.input_schema.clone()`（`ToolDefFunction.parameters` 本就是 `Option`，类型天然吻合）。

**`tool_type` 非 None 且非 `web_search_20250305` 的服务端工具**（`code_execution` / `bash` / `computer_use` …）：**丢弃 + warn**，与 Responses 侧 `other =>` 分支语义对齐 —— 这正是本修复要达成的一致性。

> 该修复**独立于 web search 可验证**：`web_search` 配置缺省时，带 `web_search_20250305` 的请求从 500 变成 200（工具被丢弃 + warn）。这是本 Stage 唯一一条**不依赖 Stage 134 任何代码**的验收项。

### 3.4 `search_context_size` → 条数映射（照抄 Higress）

Higress `ai-search` 把 OpenAI 原生参数**当作自己的开关**用，`search_context_size` low/medium/high → **1/3/5**（架构调研 §5.3 第一个证据块 / `:97`）。aigw 照抄，**使对客户端呈现的参数语义与 OpenAI 一致**：

| `search_context_size` | 结果条数 | 备注 |
|----------------------|---------|------|
| `low` | 1 | |
| `medium`（**缺省默认**） | 3 | litellm 对无该参数时同样默认 medium（架构调研 `:494`） |
| `high` | 5 | 与 `DEFAULT_MAX_RESULTS = 5`（Stage 134 §3.2）同值 |
| 未知字符串 | 3 | 降级为 medium + `tracing::warn!`，不报错 |

Anthropic 侧的 `max_uses` 语义不同（**搜索次数**而非结果条数，且官方无默认值，架构调研 §5.5），本 Stage **只取 `min(max_uses, max_results)` 当条数上限**，不实现多次搜索。**`max_uses` 缺省（`None`）时不参与 clamp** —— 只用 `search_context_size` 映射与 `config.max_results`（Review F2：`max_uses` 必须在 §3.3 的结构体里显式声明才读得到）。

**映射结果由 Stage 134 的 `WebSearchRegistry::search()` 单点 clamp**（`guardrails.clamp_max_results`，`websearch/mod.rs:131`）—— 触发层把映射值直接塞进 `SearchRequest.max_results` 即可，**不在本 Stage 重复实现 clamp**（Review F5：设计初稿的 `inject::clamp_results` 与下层重复，删除）。客户端**只能下调、不能上调**这条不变量由 Stage 134 的既有 UT 守护。

### 3.5 注入目标与模板

#### 选定：注入**最后一条 user 消息**（Higress 路线），不注入 system（Portkey 路线）

两家都有独立实现（架构调研 §5.3），但对 aigw 而言 Higress 路线有**三条压倒性理由**，前两条直接源自本仓库约束：

1. **system 注入会与 Stage 131 的「有且仅有一条前导 system」不变量正面冲突（决定性理由）。** `consolidate_system_messages`（`adapter.rs:2410`）+ `merge_developer_into_system`（`:2357`）在 `adapt_request` 的步 3b（`:2744-2746`）把所有 system 折叠成唯一一条前导，理由是 Qwen 系严格模板拒收 index 0 之后的 system（`adapter.rs:2404-2409`）。若在 system 注入：
   - 注入发生在 adapt **之前** → 多出一条 system，虽会被 `consolidate` 折叠，但**折叠是按出现顺序 `join("\n\n")`**（`:2431`），搜索结果会被拼到 `instructions` 之后，顺序与长度均不可控；
   - 注入发生在 adapt **之后** → 必须自己维护不变量（判断 index 0 是否 system、是 string 还是 blocks、追加还是新建），**等于在两处重复实现同一个不变量**，而 Stage 131 的全部 UT 只守护 adapter 内那一处 → 不变量在新路径上无人看守；
   - Anthropic 侧更糟：system 是**独立顶层字段**且是 untagged 二形态枚举（`models.rs:1043` + `:1074-1079` `Text(String) | Blocks(...)`），系统注入需要为 string / blocks **各写一条分支**。
   —— 即 system 注入要写 **3 条互不相同的代码路径**（Chat messages / Responses 折叠前后 / Anthropic 顶层字段二形态）；而最后一条 user 消息在三条入口**归一化之后形状一致**，`messages.last_mut()` 一条路径通吃。奥卡姆剃刀直接判给 Higress。
2. **role 语义正确。** 搜索结果是「回答*这一个问题*的材料」，不是「系统规则」。放进 system 等于把一次性数据提升到规则优先级，并在多轮会话里把它变成**永久**规则（下一轮客户端带回的 system 不含它，但本轮模型会按规则权重理解它）。
3. **不破坏 prompt 前缀缓存。** system 前缀保持逐字稳定 → 上游的 prefix cache 命中率不被每次不同的搜索结果击穿。这对 `cache_read_tokens` 已在计费链路里的 aigw（`extract_cache_read_tokens`，`responses.rs:1049`）是真金白银。

**代价（接受）**：模型看到搜索结果的位置晚于 system 指令。Higress 生产验证该位置可用，且模板内自带角色说明已足够。

#### 模板

照 Higress 的占位符契约（`promptTemplate` **必须含 `{search_results}` + `{question}`**，可选 `{cur_date}`），落为常量 `inject::PROMPT_TEMPLATE`：

```text
# 以下是联网搜索到的参考资料（当前时间 {cur_date}）

{search_results}

# 用户问题

{question}

# 回答要求

- 优先依据上述参考资料回答；资料不足或相互矛盾时如实说明，不要臆测。
- 引用资料时用 markdown 链接标注来源，形如 [域名](URL)，例如 [example.com](https://example.com/a)。
- 参考资料与问题无关时直接忽略，按你自己的知识回答。
```

> ⚠️ **最后一条不是礼貌性措辞，是必需的防噪音护栏（2026-10-06 SearXNG 实测结论）**：[实测] SearXNG **没有「零结果」语义** —— 对乱码 query `zzqqxk7h3v9nonexistentquerystring42` 仍返回 **35 条完全无关的结果**（Subway 门店、Google 首页、bitcoin GitHub issue…），`results` 永不为空。即**网关无法靠「结果数为 0」判断「搜不到」**，必然存在「把无关资料注入给模型」的情形。该条指引是这一情形下唯一的缓解手段，**不得在精简模板时删除**。Stage 134 §8.1 已同步登记该风险。

`{search_results}` 的逐条渲染（照 litellm `format_search_response` 的 `Title/URL/Snippet` 三行拼接，架构调研 §5.5 第 2 条，**不含正文**）：

```text
[1] {title}
    URL: {url}
    {snippet}
    （{published_date}）   ← published_date 为 None 时整行省略
```

- 字段取自 Stage 134 §3.2 的 `SearchResult`（`title` / `url` / `snippet` / `published_date: Option<String>` / `score: Option<f32>`）；
- **`score` 不渲染** —— 候选厂商中仅 Tavily 有（Stage 134 §3.2 末），渲染会让输出形状随 provider 漂移。本期只有 SearXNG（无 `score`）故实际无数据可渲染，但该规则是**为后续接入预先定下的形状契约**，不因当期单 provider 而放宽；
- `snippet` 已在 Stage 134 §3.4 按 `snippet_max_chars`（默认 2000）截断，本层**不再截**；
- **markdown 链接引用而非结构化 citations** —— OpenRouter 默认 `search_prompt` 同款做法（架构调研 §5.4 末「有业界先例，不是降级妥协」）。

#### 目标消息定位与注入方式

```
找最后一条 role == "user" 的消息
  → 命中：把渲染结果作为**新增的一个 text block / text content-part 追加**到该条
          （原 blocks/parts 逐字保留）
  → 未命中（纯 system / 纯 assistant 历史）：不注入，warn「no user message to inject into」，按未搜索处理
```

**只改最后一条 user，历史 user 消息零改动** —— 与 Higress `sjson.SetBytes(body, "messages.{queryIndex}.content", prompt)` 位置一致（该实现只针对纯 string content，aigw 需覆盖 blocks 形态故改为追加）。

> **Review F3 — 必须追加而非替换。** 设计初稿写「content 归一为 text 后整条替换」。但 Anthropic 的 `role=="user"` 消息**常常是 `tool_result` 的载体**：`claude_message_to_openai`（`adapter.rs`）把其 blocks 拆成 `role:"tool"` 消息 + 一个 text/image 的 `role:"user"` 消息。整条替换会抹掉 `tool_result` blocks → 工具调用无应答 → `normalize_tool_pairing` 剪除未应答调用 → **模型看不到工具结果**（Stage 132 刚修过的同类事故）。Responses 路径的 `function_call_output`（→ `role:"tool"`）同理。故：
> - 注入 = 在最后一条 user 的 content 上**追加**一个 text block（Anthropic blocks 形态）/ content-part（OpenAI parts 形态）/ 拼接（纯 string 形态）；
> - `{question}` = 从该条 content 中**仅取 text 部分拼接**（`tool_result` / image 不参与 query 构造）。

**Responses 路径的注入目标在 `input[]`（`items_to_messages` 之前）**，故三种输入形态各一分支（Review：`input_to_messages` 已核实接受三形态）：

| `input` 形态 | 注入方式 |
|-------------|---------|
| `Value::Array(..)` — 末条为 `{"type":"message","role":"user","content":..}` | 对 content 追加（content 为字符串 → 拼接；为 part 数组 → push 一个 `{"type":"text","text":..}`） |
| `Value::Array(..)` — 末条为裸 `{"type":"input_text","text":..}` | 该 item 的 `text` 拼接 |
| `Value::String(s)` | 直接替换为 `s + "\n\n" + <渲染结果>`（无 blocks 可保，纯文本，安全） |
| 末条为 `function_call_output` / 任何非 user item | **不注入**（走「未命中」分支） |

### 3.6 三条入口的接线位置

搜索必须在 `adapt_request` **之前**（设计 C 的定义），且对三条入口位置一致：

| 路由 | 插入点 | 说明 |
|------|-------|------|
| `chat.rs` | `:1532`（`adapt_span` 之前） | body 是 `Value`，直接改 `messages` 数组尾部 |
| `responses.rs` | `:610`（`adapt_span` 之前） | **此时 `input[]` 还未转成 messages** → 注入目标是 `input[]` 里最后一条 `role=="user"` 的 item（或裸 `input_text`），随后由 `items_to_messages` 正常转换 |
| `v1_messages.rs` | `:857`（adapt 调用之前） | 目标是 `messages[]` 最后一条 `role=="user"`，content 为 blocks 时追加/替换 text block |

三者共享同一个序列：`detect_*` → 无命中则原样 → 命中则 `registry.search()` → `inject::render()` → 改 body → 交给既有 `adapt_request`。

**`WebSearchRegistry` 为 `None`（配置缺省）时 `detect_*` 仍跑，但立即走 §3.8 的降级路径** —— 这样「配置缺省 = 零行为变化」这条验收项由同一段代码保证，不靠两份分支。

#### 3.6a 两条既有旁路路径（Review F1 / F4 —— 设计初稿未覆盖）

**(a) OAuth 反代分支完全旁路注入点。** 三条路由的 OAuth 分支都在各自的 adapt 调用点**之前 `return`**：

| 路由 | 分支起点 | `return` 点 | 设计的接线点 | 是否被旁路 |
|------|---------|------------|-------------|-----------|
| `chat.rs` | `:1160` `if let Some(ref oauth)` | `:1507` | `:1532` | ✅ 旁路 |
| `v1_messages.rs` | `:548` | `:840` | `:857` | ✅ 旁路 |
| `responses.rs` | `:252` | `:582` | `:610` | ✅ 旁路 |

即解析到 `anthropic_oauth` 凭证的模型，三条入口都不经过 §3.6 的接线点。

**处置 = 显式边界，不改 OAuth 分支**：
- `v1_messages.rs`（Anthropic 协议）：`adapt_to_anthropic(ClientProtocol::Anthropic, body, ..)` 原样透传（`oauth_pipeline.rs`），Anthropic 上游**原生支持** `web_search_20250305` —— 网关不该介入，旁路是**正确**的。
- `chat.rs` / `responses.rs`（OpenAI 协议走 `OpenAIToAnthropic` 转换）：`web_search` 工具在转换中被丢弃，搜索不发生。这是**范围缺口而非回归**（Stage 135 之前同样如此），登记 §8.2 为后续项。
- **验收补充**：OAuth 路径下带搜索工具的请求行为与 Stage 135 之前**逐字一致**（新增一条断言，防止将来的接线误伤 OAuth 分支）。

**(a′) 非 OAuth 的原生直通同样必须豁免（Review F1 —— 设计初稿漏掉的一半）。** §3.2 表写「命中后该 tool 被移除」，但那两个移除机制**只存在于转换适配器里**，原生直通跑不到：

| 场景 | 适配器 | tool 移除机制是否可达 |
|------|--------|---------------------|
| Anthropic 请求 → `AnthropicNative` 部署 | `AnthropicPassthrough`（`adapter.rs:82`）→ `from_value`/`to_value` 原样往返 | ❌ `claude_to_openai_request` 的 schema-none 过滤（`DefaultAdapter`，只被 `AnthropicToOpenAI` 调用）不执行 → `web_search_20250305` **原样转发** |
| Responses 请求 → 声明 `"responses"` 的部署 | `ResponsesPassthrough`（`adapter.rs:108`） | ❌ `normalize_responses_tools` 不执行 → `{"type":"web_search"}` **原样转发** |

若不处理：网关**先自掏腰包跑一次 SearXNG 搜索**（~2.4s + 费用）并注入 markdown 结果，再把工具声明转发出去 → **上游又原生跑一次搜索**，两套结果并存、**双重计费**。这与 (a) 的 OAuth 豁免是同一个道理（「上游原生支持则网关不该介入」），初稿只对 OAuth 分支说了，漏了非 OAuth 的同类路径。

**处置**：`web_search_wire::upstream_handles_search(protocol, deployment)` 判定，命中即**整段跳过**（不检测、不搜索、不注入、不标记）。`chat.rs` 恒 `false`（OpenAI 面无原生直通）；`v1_messages.rs` 判 `provider_type == AnthropicNative`；`responses.rs` 判 `supported_standard_types` 含 `"responses"`。该判定有 2 条 UT 锁定。

**(b) exact-match 缓存命中会白搜且静默返回无搜索结果。** `chat.rs` 的缓存键于 `:1116-1128` 由**注入前** body 计算（`canonical_body(&body)`），命中分支 `:1655` → `:1721` `return`，而 adapt 在 `:1534` —— **注入点 `:1532` 在缓存命中判定之前**。若不在设计里处理，命中缓存的请求会：① 真的发起一次搜索（付费 + ~2.4s 延迟）后被缓存体短路 → 白搜；② 返回**注入前写入的**缓存体 → 客户端明明要了搜索却拿到无搜索结果的响应，且**无 metadata 标记**（§3.9 的标记根本没机会置）。

**处置**：**触发搜索的请求绕过 exact-match 缓存**。理由：搜索是逐次外部依赖，返回缓存体等于返回「陈旧且无搜索」的响应，语义错误。实现选一处判定：命中触发时把 `cache_control.use_cache` 置 `false`（推荐 —— 一处判定，不动既有缓存控制流）。

**`responses.rs` 同理须在实现时核查是否存在同构缓存路径** —— 结论记入 Implementation Notes（设计初稿未提，Review 未能在不读全文的前提下断定，故列为编码期必查项）。

### 3.7 流式 —— **零改动**，这是设计 C 的核心优势

搜索在**首字节之前**完成，请求体改写完毕才发往上游 → **上游 SSE 不需要任何干预**（架构调研 §5.4 表格第一行，Higress 同款）。

**`responses.rs:1034-1079` 一行都不碰。** 该处的 `while let Some(chunk_result) = stream.next().await` + `stream_adapter.next(&pending_chunk)` + `pending_chunk.clear()` + 循环后 `stream_adapter.finish()` 是**三次生产事故的同一现场**（§2.6：`c3f360c` 死循环 / `4bd85c7` 提前 `[DONE]` / `8cf8c12` SpendLog 只存末 chunk），`stage-133.md` §8 已登记为高风险区。本 Stage 的门禁包含「流式字节序列与 Stage 133 基线逐字节一致」的回归断言（§4.3）。

**唯一的流式副作用**：搜索耗时直接计入 TTFB（架构调研 §5.4 末）。**[实测 2026-10-06] 该增量是真实且可观的** —— SearXNG 自建实例 `30.184.60.216:9099` 实测 **2.0–3.1s（mean 2.43s，5 次连打）**，因为它要等 bing+yandex+sogou 三引擎全部回包；Tavily 为 ~1s 级。即**用 SearXNG 作默认后端时，所有搜索请求的 TTFT 恒增约 2.5 秒**。Stage 134 §3.6 已据此改为 per-provider `timeout_ms`（SearXNG 档 8000，全局默认 5000 对它偏紧）；超时走 §3.8 降级而非挂死。官方点明原生 web search 在流式中有「搜索执行期间的真实墙钟暂停」—— 设计 C 的暂停发生在**还没开始回 SSE 之前**，故只影响 TTFB，**不会造成流中途静默**，比设计 A/B 更安全。

> **选型含义（本期无可选项）**：**Phase 53 只实现 SearXNG，故 ~2.4s 的 TTFT 增量是唯一选项，不是可权衡的配置项** —— 启用搜索即接受这个延迟（多配几个 SearXNG 实例能提升可用性，但**不降低单次延迟** —— 2.4s 来自它等 bing+yandex+sogou 三引擎回包，是协议性开销而非负载问题）。Tavily（~1s）**本期不可用**，「若对 TTFT 敏感则把 Tavily 设为 `default_provider`、SearXNG 作 failover」是**接入 Tavily 之后才成立的未来杠杆**（Stage 134 的 provider 抽象与 failover 已为它就位，接入是纯增量），不是当期可用手段。配置文档须把 **2.4s 这个数字与「当期无替代后端」这一事实同时写明**，避免部署方误以为可以配置掉它。


**references 尾部追加 —— 本 Stage 不做**。若将来要做，照 Higress 的 `UnifySSEChunk` 按 `\n\n` 切分**尾部注入**（`referenceLocation: tail`），不在 chunk 中途插入，从而不触碰 §2.6 的循环语义。

### 3.8 搜索失败策略：**降级放行**（不带搜索继续调上游）

```
registry.search() → Err(SearchError::{Transport|Http|Parse|Timeout|NotConfigured})
  → 不注入任何内容
  → tracing::warn!(surface, provider, error = %e, "web search failed; proceeding without search results")
  → 响应 metadata 置标记（§3.9）
  → 照常调上游，返回 200
```

**为何降级而非失败请求（逐条论证）**：

| 论点 | 说明 |
|------|------|
| **搜索是增强，不是请求语义的必要条件** | 用户要的是模型回答。没搜到结果时模型仍能用自身知识回答（模板第 3 条「参考资料与问题无关时直接忽略」已为此预留话术）。为一个增强项把主请求打挂是明显的可用性倒退 |
| **与项目既有立场一致** | Stage 131 对无 Chat 对应的工具一律「丢弃 + warn」而非拒请求（`adapter.rs:2553-2559`）；Stage 134 §3.8 末明确「基础设施不可用时放行而非拒服务」（照 sub2api 取舍）。本 Stage 若选失败，就是在同一条管线上引入第二种相反的哲学 |
| **故障面不对称** | 搜索后端（自建 SearXNG 的脆弱抓取器；后续接入的 Tavily 则是配额耗尽即硬停 —— Stage 134 §8.1 两条风险）比主上游**更容易坏**。让更脆弱的依赖拥有否决权，会把整体可用性拉到最差环节。⚠️ **单 provider 期降级放行尤其关键**：Stage 134 的两层模型（provider → instances[]）提供的是**同 provider 内的实例级 failover**（配多台 SearXNG 可互备），但**无跨 provider 后备**（本期只有 SearXNG 一家）—— 自建抓取器的系统性失效（上游引擎改版、IP 被封）会同时打中所有实例，此时降级放行是唯一的可用性保障 |
| **Stage 134 已把故障转移做在下层** | `failover_order`（§3.8）先把 Transport/Timeout/5xx 转移过一遍；走到本层的 `Err` 已是**全部后端都失败**。此时再失败请求 = 双重惩罚 |
| **诚实性靠标记而非报错保证** | §3.9 的 metadata 标记让客户端**可判别**本次是否真的搜了（这正是架构调研「设计 D：维持丢弃 + 诚实告知」要解决的「客户端以为搜了」误判），诚实性不需要牺牲可用性来换 |

**唯一例外**：配置校验期的非法值（Stage 134 §3.5 的 `allowed_domains` / `blocked_domains` 互斥冲突等）**仍然拒绝启动** —— 加载期严格、运行期宽容，两者不矛盾。

### 3.9 metadata 标记（诚实性，非计费）

搜索命中/降级都在**非流式响应体**里置一个标记（流式不改，§3.7）：

```json
{"aigw": {"web_search": {"status": "ok|degraded|not_configured|no_target",
                          "provider": "searxng", "results": 3}}}
```

- `status` 四态：`ok`（搜到并注入）/ `degraded`（命中触发但搜索失败，§3.8）/ `not_configured`（命中触发但 `web_search` 未配置）/ `no_target`（命中触发但无可检索的 user 文本 —— 无 user 消息，或该消息只有图片/tool_result）；
- `no_target` 独立成态（Review F7）而非并入 `ok`：客户端**要了搜索却没搜**，报 `ok` + `results: 0` 会掩盖这一事实；
- **不含任何成本字段** —— `cost_per_query` 的消费归 Stage 136，本 Stage 连 `reported_credits`（Stage 134 §3.2）都不往外带；
- 流式路径无此标记（避免动 §2.6 的循环），差异写入文档，Stage 136/137 若需要再统一。

---

## 4. TDD 计划

### 4.1 单元测试

**`crates/aigw-core/src/websearch/trigger.rs` inline tests（7）**

| 测试函数 | 断言 |
|---------|------|
| `detect_chat_web_search_options_hits` | `{"web_search_options":{}}` → `Some(Chat)`；字段被移除 |
| `detect_chat_absent_is_none` | 无该字段 → `None`，body 零改动 |
| `detect_responses_web_search_and_preview_both_hit` | `web_search` / `web_search_preview` 各自命中（对齐既有 UT `adapter.rs:4188-4189`） |
| `detect_responses_other_server_tools_not_hit` | `code_interpreter` / `mcp` → `None`（仍走 `other =>` 丢弃） |
| `detect_anthropic_web_search_20250305_hits` | `{"type":"web_search_20250305","name":"web_search"}` → `Some(Anthropic)` |
| `detect_anthropic_client_tool_not_hit` | 带 `input_schema` 的普通客户端工具 → `None` |
| `context_size_maps_low_medium_high_and_unknown` | low/medium/high → 1/3/5；未知串 → 3；缺省 → 3 |

**`crates/aigw-core/src/websearch/inject.rs` inline tests（8）**

| 测试函数 | 断言 |
|---------|------|
| `render_template_fills_all_placeholders` | `{search_results}` / `{question}` / `{cur_date}` 全部被替换，无残留 `{` 占位符 |
| `render_results_includes_title_url_snippet` | 三行形状；markdown 引用说明在位 |
| `render_omits_published_date_when_none` | `published_date: None` → 不出现空括号行 |
| `render_never_emits_score` | `score: Some(0.93)` → 输出中不含 `0.93`（§3.5 决定） |
| `inject_targets_last_user_message` | 三条 user 历史 → 只有**最后一条**被改写，前两条逐字不变 |
| `inject_appends_to_user_with_tool_result_blocks` | **关键（Review F3）**：最后一条 user 含 `tool_result` blocks → 注入后 tool_result blocks **逐字保留**，注入文本作为**新增 text block** 追加；断言原 blocks 数量不变 +1 |
| `inject_no_user_message_is_noop` | 仅 system/assistant → body 零改动 + 返回未注入 |
| `inject_responses_string_input_appends` | Responses 路径 `input` 为裸字符串 → 渲染结果拼接在该字符串之后，不报错 |
| `inject_preserves_single_leading_system_invariant` | **关键**：注入后再跑 `consolidate_system_messages` + `merge_developer_into_system`，断言 system 消息数 **== 1** 且位于 index 0、其内容**不含**搜索结果（§3.5 理由 1） |

**`crates/aigw-core/src/adapter.rs` 新增 tests（6）**

| 测试函数 | 断言 |
|---------|------|
| `claude_tool_def_server_tool_deserializes_without_input_schema` | **§2.3 修复的红绿点**：`{"type":"web_search_20250305","name":"web_search","max_uses":5}` 能 `from_value` 成功（修复前 `Err`） |
| `claude_tool_def_reads_max_uses` | **Review F2**：同一 JSON 反序列化后 `max_uses == Some(5)`；客户端工具（无该字段）→ `None` |
| `claude_tool_def_client_tool_still_deserializes` | 普通工具不回归 |
| `anthropic_request_with_web_search_tool_no_longer_errors` | 整条 `ClaudeMessageRequest` 解析成功 → `adapt_request` 返回 `Ok`（修复前 `Err(Parse)` → 500） |
| `anthropic_other_server_tool_dropped_with_warn` | `code_execution` 类 → 从上游 tools 移除，不报错（与 Responses `other =>` 对齐） |
| `claude_tool_without_schema_maps_to_none_parameters` | `adapter.rs:930` 跟改正确：`parameters == None` |
| `responses_web_search_tool_sets_trigger_not_silent_drop` | 丢弃点一命中时置标记；`tool_choice` 级联清理行为**不回归**（既有 UT `:4219-4220` 仍绿） |

**降级路径 tests（3，置于 `inject.rs` 或 `mod.rs`）**

| 测试函数 | 断言 |
|---------|------|
| `search_error_degrades_without_injection` | `Err(SearchError::Timeout)` → body 零改动 + 返回 `degraded` |
| `registry_absent_marks_not_configured` | `WebSearchRegistry == None` → `not_configured`，body 零改动 |
| `degraded_request_still_reaches_upstream` | 降级后仍产出合法上游 body（不是 `Err`） |

> 共约 **32 个新 UT**（含 Review F2 的 `claude_tool_def_reads_max_uses`、F3 的 `inject_appends_to_user_with_tool_result_blocks`、F1 的 `anthropic_native_upstream_handles_search`、F7 的 `question_query_is_empty_for_image_only_user_message`），全部红绿（先写断言、后写实现）。

### 4.2 BDD

复用既有 mock 设施：`crates/aigw-server/tests/bdd_support/mock_upstream.rs` 的 `MockUpstream::start()`（`:198`）/ `url()`（`:235`）/ `set_response(path, status, body)`（`:240`）/ `set_sse_body`（`:257`）/ `recorded_requests()`（`:304`）。

**Mock 搜索厂商 = 同一个 `MockUpstream` 再起一个实例**，`set_response("/search", 200, fixture)` 即为假 SearXNG；`web_search.providers[].base_url` 指向它。无需新 harness —— `MockUpstream` 已是通用 HTTP 录制桩，这正是 Stage 134 §3.1 把 provider 切成 `build_request` / `parse_response` 纯函数 + 薄 IO 壳的回报。**本期 fixture 只需 SearXNG 形状一份**（Tavily / Bocha 的响应 fixture 随各自接入时再加）；需要验证「provider 无关」的接线断言时，用 Stage 134 的测试专用 `StubProvider` 而非再造一份厂商 fixture。

新增 `crates/aigw-server/tests/features/web_search.feature`（`@mock` 标签，步骤措辞沿用 `responses.feature` 的中文风格「Given mock 上游已启动」）：

| 场景 | 断言 |
|------|------|
| Chat `web_search_options` 触发搜索并注入 | mock 搜索桩**收到请求**；mock 上游收到的 body 里**最后一条 user 消息含注入模板**；上游 body **不含** `web_search_options`（§2.4） |
| Responses `web_search` 工具触发搜索并注入 | 同上；上游 tools 不含 `web_search` |
| Anthropic `web_search_20250305` 不再 500 | 响应状态码 **200**（不是 500）；上游收到请求 |
| 未配置 `web_search` 时带搜索工具仍 200 | 三条入口各一次，断言 200 + 上游收到请求 + 搜索桩**零请求** |
| 搜索桩返回 500 时请求降级成功 | 搜索桩 `set_response("/search", 500, ...)` → 网关响应 **200**；上游收到的 body **不含**注入模板 |
| 搜索桩超时时请求降级成功 | 桩延迟 > `timeout_ms` → 网关 200 + 降级 |
| 触发搜索的请求不命中 exact-match 缓存 | **Review F4**：预置一份缓存体后发带 `web_search_options` 的请求 → 断言搜索桩**收到请求**且响应**含注入内容**（即绕过缓存、未返回注入前的陈旧体） |
| 流式请求的 SSE 事件序列不回归 | 带 `web_search_options` 的流式请求：断言 `response.created` / `response.output_text.delta` 事件仍在（沿用 `responses.feature:26-30` 的既有断言串） |

### 4.3 集成验证

1. **流式逐字节回归**：同一 mock SSE fixture，分别在「带搜索触发」与「不带」两种请求下跑 `/v1/responses` 流式，断言**客户端收到的字节序列完全一致** —— 这是 §3.7「零改动」的可执行证明，也是对 §2.6 三次事故的防线
2. **三条入口的上游 body 快照**：各录一份注入后的上游 body 固化为 fixture，防止模板或定位逻辑无声漂移
3. **`ClaudeToolDef` 修复的独立验证**：**在 `web_search` 配置完全缺省的前提下**，带 `web_search_20250305` 的 `/v1/messages` 请求返回 200（修复前 500）—— 证明该修复不依赖 Stage 134 任何代码
4. 人工：真实上游一次端到端（SearXNG 自建 + 注入 → 观察模型是否按模板用 markdown 链接引用），确认模板的引用指令对本环境的模型实际有效

---

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/src/websearch/trigger.rs` | **新增** — `SearchTrigger` / `TriggerSurface` / `detect_chat` / `detect_responses` / `detect_anthropic` / `map_context_size` + 7 UT |
| `crates/aigw-core/src/websearch/inject.rs` | **新增** — `PROMPT_TEMPLATE` / `render` / `render_results` / `inject_into_last_user` + 8 UT + 3 降级 UT（**无 `clamp_results`** —— Review F5，clamp 归 registry） |
| `crates/aigw-core/src/websearch/mod.rs` | `pub mod trigger; pub mod inject;` + re-export（Stage 134 已建该文件） |
| `crates/aigw-core/src/models.rs` | `ClaudeToolDef`（`:1065-1071`）：`input_schema` → `Option`，新增 `tool_type: Option<String>`（`#[serde(rename = "type")]`） |
| `crates/aigw-core/src/adapter.rs` | ① `:930` claude→openai 工具映射跟改 Option；② `:2553-2559` 丢弃点一改为「识别 + 置标记 + 丢弃」；③ `:2210-2216` 丢弃点二注释补说明（**行为不变**，设计 C 无服务端调用项可回放）；④ 新增 6 UT |
| `crates/aigw-server/src/routes/chat.rs` | `:1532` 之前插入搜索执行 + 注入 |
| `crates/aigw-server/src/routes/responses.rs` | `:610` 之前插入搜索执行 + 注入（**`:1034-1079` 流式段零改动**） |
| `crates/aigw-server/src/routes/v1_messages.rs` | `:857` 之前插入搜索执行 + 注入 |
| `crates/aigw-server/tests/features/web_search.feature` | **新增** — 7 场景 |
| `crates/aigw-server/tests/bdd_steps/` | 新增 `web_search_steps.rs`（复用 `MockUpstream` 作搜索桩） |
| `config.example.yaml` | `web_search:` 示例块补 `search_context_size` 映射说明注释 |
| `docs/stages/stage-135.md` | 本文件 |
| `docs/stages/stage-roadmap.md` | Phase 53 进度条推进 |
| `docs/11-next-steps.md` | Stage 135 回写 |
| `docs/12-technical-debt.md` | TD-017c 子项状态更新；新增 `ClaudeToolDef` 修复记录（独立缺陷，已修） |

> 无新依赖、无 migration、无新 DB 表。

---

## 6. 回归验证

1. `task test` 全绿，UT 净增约 **32**（实测以 `task test` 输出为准）
2. `task bdd` — 场景数 = Stage 134 基线 **+7**（新增 `web_search.feature`），**既有 284 场景 0 fail、0 行为变化**
3. **`task bdd` 在 `web_search` 配置缺省下再跑一遍** —— 断言既有场景结果与 Stage 134 基线逐项一致（「配置缺省 = 零行为变化」的硬证明）
4. `task fmt` / `task lint` green，无新 clippy warning
5. `task doctor` 通过（编译检查 + clippy；`task check` 不存在，见 CLAUDE.md 反例清单）
6. `task doctor` 无新告警
7. §4.3 第 1 条流式逐字节回归通过 —— 对 §2.6 三次事故的防线
8. 人工：§4.3 第 4 条真实上游端到端

---

## 7. 门禁

- [x] 新 UT 先 fail 后 pass（TDD 红绿）—— 实际新增 **32 个 UT**（aigw-core 611 → 643；含 `web_search_wire` 4 个 + Code Review 修复 2 个）
- [x] `inject_preserves_single_leading_system_invariant` 通过 —— Stage 131 的「有且仅有一条前导 system」不变量零破坏
- [x] `ClaudeToolDef` 修复：`web_search_20250305` 请求从 **500 → 200**，且在 `web_search` 配置缺省下即可验证（§4.3 第 3 条）
- [x] 三条入口触发检测 BDD 各一条场景通过；Chat 侧断言上游 body **不含** `web_search_options`（§2.4 的泄漏被堵上）
- [x] 搜索失败（500 / 超时）两条降级场景通过，网关均返回 **200**
- [x] 流式回归：`responses.rs:1034-1079` 零改动（`git diff` 无该区域改动），既有流式 BDD 场景保持绿
- [x] `web_search` 配置缺省时 `task bdd` 既有场景逐项与 Stage 134 基线一致（285 → 285，只增 `web_search.feature` 的 7 条）
- [x] `task test` / `task bdd` / `task fmt` / `task lint` / `task doctor` 全绿
- [x] `task bdd-real-sqlite` 58/58 绿
- [ ] ⏳ 真实上游端到端一次，确认模板的 markdown 引用指令对本环境模型有效（**未执行** —— 见 §9 遗留）
- [x] `docs/11-next-steps.md` + `stage-roadmap.md` + `docs/12-technical-debt.md` 回写
- [ ] ⏳ git commit（精确 add；`--signoff`）—— 随 Stage 136 一并交付

---

## 9. Implementation Notes（Stage 135）

### 9.1 实施差异（与设计初稿）

| # | 设计初稿 | 实施 | 原因 |
|---|---------|------|------|
| 1 | `inject::clamp_results` 独立函数 | **未实现**，clamp 归 `WebSearchRegistry::search` 单点 | Review F5：与 Stage 134 `guardrails.clamp_max_results` 重复 |
| 2 | 注入「整条替换最后一条 user」 | **追加** text block / content-part | Review F3：整条替换会抹掉 `tool_result` blocks，切断工具往返 |
| 3 | 触发即执行搜索（三入口一律） | 新增 `upstream_handles_search` 判定，**原生直通豁免** | Review F1：`AnthropicNative`（`AnthropicPassthrough`）与声明 `responses` 的部署会把搜索工具**原样转发**，上游会再搜一次 → 双重计费 |
| 4 | `status` 三态（ok/degraded/not_configured） | **四态**，新增 `no_target` | Review F7：命中但无检索文本（无 user 消息或纯图片）报 `ok` 会掩盖「要了搜索却没搜」 |
| 5 | `ClaudeToolDef` 加 `tool_type` + `input_schema: Option` | 另加 **`max_uses: Option<i64>`** | Review F2：§3.2/§3.4 要求读 `max_uses`，但不声明字段则被 serde 静默丢弃 |
| 6 | 设计未提 OAuth 分支旁路 | 显式边界（§3.6a(a)），并**推广到非 OAuth 原生直通** | Review F1 的两半 |

### 9.2 编码期核查结论（Review 的编码期必查项）

- **Review F4 对 `responses.rs` 的缓存核查**：`responses.rs` **无 exact-match 缓存路径**（仅 `cache_hit: None` / `cache_key: None` 的占位字段），无需 bypass。`v1_messages.rs` 同样无。故 §3.6a(b) 的缓存旁路**仅在 `chat.rs` 实现**。
- **`_model` 所有权**：`chat.rs` / `responses.rs` 的 `_model` 原为 `&str` 借用 `body`，与 `&mut body` 冲突（E0502），已改为在搜索前 `let _model: String = _model.to_string()` 取所有权。
- **适配器选择可用性**：`chat.rs` 的搜索点在 `pick_deployment` **之前**，故无法读 `deployment`；OpenAI 面无原生直通，硬编码 `false` 正确。

### 9.3 测试证据

- `task test`：**aigw-core 643**（611 → 643，+32）/ 其余 crate 全绿，0 fail
- `task bdd`：**292 场景（279 passed / 13 skipped）/ 1510 steps**（基线 285 → 292，+7 条 `web_search.feature` 场景）
- `task bdd-real-sqlite`：**58/58 / 280 steps 全绿**
- `task fmt` / `task lint` / `task doctor`：全绿
- 新增 BDD 场景覆盖：Chat/Responses/Anthropic 三入口触发、未配置降级、搜索 500 降级、搜索超时降级、**触发搜索绕过 exact-match 缓存**（断言搜索桩收到 2 次请求）

### 9.5 Code Review（Gate 4）

`docs/stages/stage-135-review-log.md` 的 Code Review 节记录 6 条 finding（1 Critical / 2 High / 2 Medium / 1 Low），**5 修复 / 1 不成立**：

- **C1（Critical）**：三路由的 `_resolve_enter` span guard 跨搜索 await 持有 → 跨线程 drop panic 风险 → 三处均在 `maybe_serve` 前 `drop`。
- **C2（High）**：chat 搜索点位于 OAuth 分支之前，OAuth 非流式 return 缺 `attach_status`（付费搜索对客户端不可见）→ 补 attach。
- **C3（High）**：query 无长度上界 → 长文档会把数百 KB URL 发给搜索后端并 414 → `MAX_QUESTION_CHARS = 512` 双点封顶。
- **C4（Medium）**：结果被 `normalize` 全过滤时仍注入空模板并报 `ok` → 改判 `NoTarget`。
- **C5（Low）**：占位符替换顺序导致结果文本里的字面 `{question}` 被改写 → 调整替换次序。

### 9.4 未走严格 TDD 红绿的部分

UT 与实现同批编写（`inject.rs` / `trigger.rs` 的新增测试在实现落地后补齐），非「先红后绿」。**但 2 个测试确实先红后绿**：`inject_skips_tool_result_only_user_message` 与 `inject_responses_skips_function_call_output` 在首次 `task test-filter` 中 FAILED，暴露了「跳过 tool carrier 后应回退到更早的真实提问」这一语义，修正断言与实现后转绿。其余为同批编写 + 逻辑审查，不等同于红绿流程 —— 与 Stage 134 §9.4 的诚实登记一致。

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 缓解 |
|------|------|
| **改动落在 `adapter.rs` 的热点函数上**（`normalize_responses_tools` / claude→openai 工具映射），Stage 131/132 的既有 UT 全部覆盖该区域 | 丢弃点一只**追加**标记不改丢弃语义；既有 UT（`adapter.rs:4188-4189`、`:4206`、`:4219-4220`、`:4565-4572`、`:4666`、`:4689`）全部保留不改，任一变红即视为回归 |
| **搜索耗时直接计入 TTFB**（设计 C 的搜索在首字节之前，架构调研 §5.4 末） | Stage 134 的 per-provider `timeout_ms`（SearXNG 档 8000）+ 共享 `reqwest::Client` 连接复用；超时走 §3.8 降级而非挂死。暂停发生在 SSE 开始之前，**不会造成流中途静默**。⚠️ **本期无法用「换低延迟 provider」缓解** —— 只有 SearXNG 一家（§3.7），~2.4s 增量须按既定成本接受并在配置文档中明示 |
| **注入内容计入 prompt token**（成本可见但不可避免，架构调研 §5.3 代价 ②） | `search_context_size` 默认 medium（3 条）而非 5；snippet-only（Stage 134 §3.5 强制）；`snippet_max_chars=2000` |
| **`ClaudeToolDef` 改 Option 可能影响 `AnthropicPassthrough` 的 round-trip**（`adapter.rs:1619-1620` → `:1660-1661`） | 新增 `tool_type` 字段后 `type` 不再丢失（原先会丢，§2.3 第 2 条）；但 `max_uses` 等**其余未声明字段仍会在 round-trip 中丢失**（`models.rs` 全文无 `#[serde(flatten)]`）→ 登记 §8.2 |
| **模板的引用指令可能对本环境模型无效**（模型不按要求输出 markdown 链接） | §4.3 第 4 条人工验证为门禁项；模板是常量，无效时改字符串即可，不触碰逻辑 |
| **非流式有 metadata 标记、流式没有**（§3.9） | 刻意为之以避免动 §2.6 的高风险循环；差异写入文档，Stage 136/137 需要时统一 |
| **`web_search_preview` 与 `web_search` 的语义差异未经实证** | 两者按等价处理（既有 UT 已把两者并列测过，`adapter.rs:4188-4189`）；若 OpenAI 后续分化，在 `trigger.rs` 单点调整 |

### 8.2 遗留（本 Stage 不做，登记 TD-017c 子项）

**与 stage-134.md §8.2 保持一致，不重复展开已登记项**（后续候选厂商的完整价目表、Mojeek 缓存 TTL 约束、`AnswerProvider` 能力分离、配额加权 LB、query 改写、`published_date` 统一解析、搜索结果缓存，均见该节）。此处只登记**本 Stage 新增或与接线直接相关**的部分：

**首批待接入 provider（抽象已就位，接入是纯增量）** —— Phase 53 只实现 SearXNG，下列两家是**优先级最高的后续接入项**，Stage 134 的 `SearchProvider` trait / `kind` 判别式 / N-provider 配置 / failover / per-provider `timeout_ms` / `api_key` + `v2:gcm:` / `cost_per_query` **已全部就位**，接入各自只需补一个 `parse_response`：

| provider | 单价 | 接入时的特别事项 |
|----------|------|----------------|
| **Tavily** | $0.008/query（1000/mo 免费后） | 延迟 ~1s，接入后即成为「TTFT 敏感场景的 `default_provider`」选项（§3.7 的未来杠杆）；是唯一带 `score` 字段的厂商（§3.5 的形状契约为它预留）；配额耗尽即硬停（Stage 134 §8.1） |
| **博查 Bocha** | ¥36/1k ≈ $0.00507/query | ⚠️ **CNY 牌价**，`spend` 语义恒为 USD → 接入前须先落地 Stage 136 §8.2 的汇率决策；**在决策落地前，Bocha 的落库金额不得用于对外出账** |

**后续候选搜索厂商（与 stage-134.md §8.2 同表，此处仅重申两条与成本/合规直接相关的）**：

| 厂商 | 定价 | 为何延后 / 特别事项 |
|------|------|-------------------|
| **Serper.dev** | **$0.30–1.00/1k**（Starter $1.00 → Ultimate $0.30）—— 全场最便宜 | ⚠️ endpoint path 与 `X-API-KEY` header 名在调研中标「未验证（请复核）」；`published_date` 是**相对时间串**（"2 days ago"）需额外解析；credits **6 个月过期**；作为 Google SERP 代理承担 Google ToS 灰区风险且**不提供** SerpApi 式法律盾 |
| **ScrapingDog** | **$0.333/1k** —— **全样本中唯一带明确数据授权（data-license）ToS 条款的厂商** | ⚠️ 该类通用抓取平台整体被标「未验证（请复核）」，价格与 ToS 结论**须先独立复核**再立项；集成成本与性价比均劣于专用 SERP API |

**法律警示（若将来接入 Jina）**：`s.jina.ai` 的 Terms **§4.5(iii) 禁止用其 Output 构建与 Jina 竞争的服务** —— 对「转卖搜索能力的 AI 网关」而言这是**直接相关**的风险，上生产前必须法务确认。另：Jina 只有全文 markdown 一种模式，与 Stage 134 §3.5 的 snippet-only 护栏根本冲突。

**后续 Phase 的两个设计形态**（架构调研 §5.3；与 stage-134.md §8.2 同表）：

| 形态 | 说明 | 阻塞项 |
|------|------|-------|
| **设计 A（短路）** | tools 只含 web_search 时不调模型，网关搜完直接合成 `server_tool_use` + `web_search_tool_result` + `text` 返回。**唯一能让 Claude Code 的 WebSearch 真正可用** —— 那是独立的 `/v1/messages` 子请求，**本 Stage 的设计 C 对它不适用**（它本身不需要模型综合） | ① 必须合成 Anthropic 服务端工具块 → 踩 `encrypted_content` 不可伪造（调研 §4.4）；② 多轮回放须按 id 前缀剥离历史块（§5.7），否则投毒 → 400。SSE 侧已有能力（`ResponsesToChatCompletionsStream` 的 `ensure_tool_item` 可作模板，codebase-map §A5），不是阻塞项 |
| **设计 B（agentic loop）** | 把 web_search 换成内部 function 工具 → 模型回 tool_call → 搜索 → 回灌 → 再请求，上限 3 轮 | **硬前提未实测：上游 MaaS 是否支持网关*注入*的 function 工具往返**（调研 §5.9 第 1 项）。**Stage 132 只证明了「Codex 客户端自己声明的工具」能完整 round-trip**（`function_call` / `function_call_output` 分派已验证）；**网关注入**的工具是否被模型正确调用**尚无任何证据** → 立项前必须先做这一项实测。另需照抄 litellm 五条安全栏（上限 3 / 客户端不可覆盖 / 指纹断环 / 撞上限剥名改 `stop_reason` / 每轮独立 spend 上下文） |

> **循环上限的业界基准**（2026-10-06 补充取证，均读源码/官方文档）：LangGraph `DEFAULT_RECURSION_LIMIT = 25`（**单位是 super-step，ReAct 约 2 step/轮 → ≈12 轮**）、LlamaIndex `DEFAULT_MAX_ITERATIONS = 20`、OpenAI Agents SDK `DEFAULT_MAX_TURNS = 10`（**单位是 turn = 1 次 LLM 调用**，与 LangGraph 不可直接比较）、legacy `AgentExecutor.max_iterations = 15`、Anthropic web search `max_uses` **无默认值**（官方指引：简单事实查询 1-3 次，多实体对比研究可 10+）、vLLM **零内置**（循环完全在用户态）。→ **文档化区间 10-25 轮，10 是最常见硬默认**。litellm 的 `max_agentic_loops=3` 明显低于业界，是**网关场景**的保守取值（网关要为所有租户的成本负责，而框架是单应用自用）—— 本项立项时若沿用 3，应在文档里写明这是**刻意保守**而非照抄业界默认。


**本 Stage 的功能性遗留**：

- **搜索 provider/实例与定价的 DB 化与 UI 管理 → Stage 138**：本 Stage 从 `config.yaml` 读取 provider 配置（`WebSearchRegistry` 构造于启动期），改配置需重启。Stage 138 将仿 `proxies` 表 / `027_proxies.sql` / Phase 50 全套迁到 DB 表 + admin CRUD + 前端管理页 —— 对本 Stage 的影响仅限 registry 的**构造时机与数据来源**，§3.2 的触发检测、§3.5 的注入模板、§3.8 的降级策略均不变

- **结构化 citations**（Chat `annotations` 的 `url_citation` / Responses 扁平 `url_citation` / Anthropic `citations`）—— 本 Stage 用 markdown 链接替代。若要补，**从 Chat Completions 的 `annotations` 开始**（唯一无不可伪造字段的协议，架构调研 §5.4 末）
- **流式 references 尾部追加**（Higress `UnifySSEChunk` 按 `\n\n` 切分 tail 注入）—— 需触碰 `responses.rs:1034-1079` 高风险区（§2.6 三次事故现场），单独立项并配逐字节回归
- **历史侧 `web_search_call` 回放**（丢弃点二 `adapter.rs:2210-2216` 行为不变）—— 设计 C 不产生服务端调用项，故无可回放之物；设计 A/B 落地时才需要，届时须配 §5.7 的 id 前缀剥离
- **`ClaudeMessageRequest` / `ClaudeToolDef` 的未知字段保真**（`max_uses` 等在 `AnthropicPassthrough` round-trip 中仍会丢失，`models.rs` 全文无 `#[serde(flatten)]`）—— 本 Stage 只补 `type` 与 `input_schema` 两个字段；全面保真需引入 `#[serde(flatten)] extra: Map` 并评估对既有序列化断言的影响
- **流式路径的 metadata 标记**（§3.9 仅非流式）
- **多次搜索 / Anthropic `max_uses` 真实语义**（本 Stage 把它降解为条数上限，不实现多次搜索）
- **搜索 query 的来源**（本 Stage 直接取最后一条 user 消息的文本作 query，未做抽取/改写）—— 长对话或多段落 user 消息下 query 质量会下降；改写需多一跳 LLM，不在阶段 1
