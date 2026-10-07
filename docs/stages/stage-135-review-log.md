# Stage 135 Review Log

**Review Type**: Design
**Review Date**: 2026-10-07
**Reviewer**: Claude (Opus, main) + 2× general-purpose subagent (adversarial)
**Stage**: Stage 135（搜索接线三条入口 + prompt 注入）

## Review Summary

设计与仓库现状高度吻合：§2 的每一处锚点行号均已逐一复核（`adapter.rs:2210-2216` / `:2553-2559` / `models.rs:1065-1071` / 三路由 adapt 调用点），§2.3 的 500 失败链路（`Parse` → 三条路由均映射 500）确认成立。方案（设计 C + 注入最后一条 user + 降级放行 + 流式零改动）的道理链完整，选型理由（system 注入要 3 条路径 vs user 注入 1 条）经代码验证无误。

**发现 5 项 High / Critical，均需在编码前修订设计** —— 其中两项（OAuth 分支旁路、缓存命中旁路）是设计完全未提及的既有代码路径，会让接线在实际运行中静默失效；两项（`max_uses` 字段缺失、注入不得替换含 tool_result 的 user 消息）会导致设计声明的能力不可实现或破坏工具往返。另有 3 项 Medium（Occam/边界）。

## Findings

### F1 — OAuth 反代分支完全旁路注入点（Critical，需改设计）

三条路由的 OAuth 分支都在 **adapt 调用点之前 `return`**：

| 路由 | 分支起点 | `return` 点 | 设计的接线点 |
|------|---------|------------|-------------|
| `chat.rs` | `:1160` `if let Some(ref oauth)` | `:1507` | `:1532` |
| `v1_messages.rs` | `:548` | `:840` | `:857` |
| `responses.rs` | `:252` | `:582` | `:610` |

即：**解析到 `anthropic_oauth` 凭证的模型，三条入口全部绕开 §3.6 的接线点** —— 带 `web_search_options` / `web_search` 工具时，搜索不会执行，`detect_*` 也不会跑。

**证据**：`chat.rs:1160-1164` 注释与 `:1193` `adapt_to_anthropic` 调用；`v1_messages.rs:548-563`；`responses.rs:252-265`。三处的 `return` 均在各自的 `adapter.adapt_request` 之前。

**判定**：对 **Anthropic 协议**（`v1_messages.rs`）而言这是**可接受的正确行为** —— `adapt_to_anthropic(ClientProtocol::Anthropic, body, ..)` 直接 `Ok(body)` 原样透传（`oauth_pipeline.rs`），Anthropic 上游**原生支持** `web_search_20250305`，网关不该也不需介入。对 **Chat/Responses 协议**（走 `OpenAIToAnthropic` 转换）而言，`web_search` 工具会在转换中被丢弃，搜索不会发生 —— 这是范围缺口，不是回归。

**处理**：在设计 §3.6 明确写出「OAuth 分支不接线，Anthropic 原生搜索由上游执行」作为**显式边界**（§3.7/§8 已排除设计 A/B，此处是对第三条既有路径的封锁），并补一条验收：OAuth 路径下 `web_search_options` 的行为与 Stage 135 之前逐字一致。

### F2 — `ClaudeToolDef` 修复未加 `max_uses`，但 §3.2/§3.4 声明要读它（High）

§3.3 的修复结构体只加 `tool_type: Option<String>` + `input_schema` 改 `Option`，**没有 `max_uses` 字段**。但 §3.2 表写「`max_uses` 一并读出，仅作上限提示」，§3.4 写「只取 `min(max_uses, max_results)` 当条数上限」。没有字段则读不到。

**证据**：§3.3 代码块仅两个字段；`models.rs` 全文无 `#[serde(flatten)]`（§2.3 已自陈），未声明的 `max_uses` 在反序列化时被静默丢弃。

**处理**：在 §3.3 结构体补 `max_uses: Option<i64>`（`#[serde(skip_serializing_if = "Option::is_none", default)]`），并在 §3.4 明确：`max_uses` 缺省（None）时不参与 clamp，只用 `search_context_size` 映射与 `config.max_results`。

### F3 — 注入「整条替换」会摧毁含 `tool_result` 的 user 消息，破坏工具往返（High）

§3.5 目标定位写「content 归一为 text 后，**整条替换**为渲染结果」。但 Anthropic 的 `role=="user"` 消息**常常是 tool_result 的载体**：`claude_message_to_openai`（`adapter.rs`）把其 blocks 拆成 `role:"tool"` 消息 + 一个 text/image 的 `role:"user"` 消息。若对「最后一条 user」整条替换：

- 该条的 `tool_result` blocks 被抹掉 → 工具调用无应答 → `normalize_tool_pairing` 剪除未应答调用 → **模型看不到工具结果**（Stage 132 刚修的同类事故）；
- 对 Responses 路径，`items_to_messages` 的 `function_call_output`（→ `role:"tool"`）同理。

**证据**：`adapter.rs` `claude_message_to_openai` 的 `tool_results` 收集分支（`if !tool_results.is_empty() && msg.role == "user"`）。

**处理**：§3.5 改为**追加而非替换** —— 在最后一条 user 消息的 content 上追加一个 text block / content-part，保留原有 blocks/parts；并在 §4.1 加 UT：`inject_appends_to_user_with_tool_result_blocks`（断言 tool_result blocks 仍在、注入文本作为新 text block 追加）。同时把 `{question}` 的取值从「原 content」改为「原 content 中的 text 部分拼接」（tool_result 不参与 query 构造）。

### F4 — 三条入口的注入点须避开缓存命中路径（High，仅 chat/responses）

`chat.rs` 的 exact-match 缓存：`cache_key` 于 `:1116-1128` 由**注入前** body 计算，缓存命中分支 `:1655` `if let Some(cached) = cache_hit { … return }` 位于 **`:1721` return**，而 adapt 在 `:1534` —— **注入点 `:1532` 在缓存命中判定之前**。后果：

1. 命中缓存的请求**仍然会真的发起一次搜索**（付费 + 延迟），随后被缓存体短路 → 白搜；
2. 缓存体是注入前写入的 → 命中时返回的响应**不含搜索结果**，但客户端明明要了搜索 —— **静默降级且无 metadata 标记**（§3.9 只在非流式置标记，而这里根本没走到注入）。

**证据**：`chat.rs:1116` `cache_key = … canonical_body(&body)`；`:1655-1721` 命中分支含 `:1721` `return`；`:1534` adapt。

**处理**：§3.6 补充决策 —— **触发搜索的请求绕过 exact-match 缓存**（搜索是逐次外部依赖，命中缓存等于返回陈旧且无搜索的响应）。实现上二选一，写进设计：(a) 命中触发时把 `cache_control.use_cache` 置 false；或 (b) 把注入下移到缓存命中 `return` 之后。推荐 (a)：一处判定，不动既有缓存控制流。`responses.rs` 若同样有缓存路径，一并核查（设计当前未提，需在 §3.6 写明核实结论）。

### F5 — `inject::clamp_results` 与 Stage 134 的 `guardrails.clamp_max_results` 重复（Medium，Occam）

`WebSearchRegistry::search()`（`websearch/mod.rs:131`）**已经**用 `guardrails.clamp_max_results(req.max_results)` 做过 clamp，且 `normalize` 按 `max_results` 截断。§4.1 的 `clamp_requested_results_to_config_max` 测的是一个与下层重复的函数。

**处理**：删掉 `inject::clamp_results`，`SearchRequest.max_results` 直接透传触发映射值，clamp 归 Stage 134 registry 单点负责（并保留「客户端不能上调」这条既有 UT）。§5 变更清单同步删除。

## AI Pre-Filter Results

- 子 agent 提出的「Responses `input` 可能是裸字符串导致无法注入」——**保留但降级为 Medium**：`input_to_messages` 确实接受 `Value::String`（已核实），§3.6 需补该分支（裸字符串 → 直接作为 user content 处理），但这是边界而非主路径。
- 「`tool_type` 加到 `ClaudeToolDef` 会让 `AnthropicPassthrough` round-trip 多出 `type`」——**过滤（不成立）**：`#[serde(skip_serializing_if = "Option::is_none")]` 保证客户端工具（无 `type`）序列化不变；已核实 `models.rs` 现有同款写法。
- 「注入破坏唯一前导 system 不变量」——**过滤（不成立）**：注入只碰 user 消息，`consolidate_system_messages` 不受影响；§4.1 的 `inject_preserves_single_leading_system_invariant` 已覆盖。

## Rule Filtering

- **Scope creep**：未发现。§「明确不做」边界清晰，且被 §3 各项一致遵守。
- **Memory bias**：§2 的每处锚点已复核，无未声明假设；唯 `max_uses`（F2）是把「意图」当「已实现」。
- **Logical fallacy**：注入位置论证（system 需 3 条路径 vs user 1 条）经代码验证成立。

## Resolution Summary

| # | 严重度 | 处置 |
|---|-------|------|
| F1 | Critical | **改设计**：§3.6 增「OAuth 分支不接线」显式边界 + 验收项；§8.2 登记 |
| F2 | High | **改设计**：§3.3 结构体补 `max_uses: Option<i64>`；§3.4 补缺省语义 |
| F3 | High | **改设计**：§3.5 由「整条替换」改「追加 text block/part」；§4.1 加 UT |
| F4 | High | **改设计**：§3.6 增「触发搜索的请求绕过 exact-match 缓存」+ responses 复核 |
| F5 | Medium | **改设计**：删 `inject::clamp_results`，clamp 归 registry |

**All Critical Fixed**: 是（F1 已改设计）
**All High Priority Addressed**: 是（F2/F3/F4 已改设计）

## Design Review 后置（编码期验证）

- F4 对 `responses.rs` 的缓存核查结论（有/无缓存路径）须在实现时记入 §Implementation Notes
- F3 的 `{question}` 提取在 Anthropic blocks 形态下的具体切法（仅 text block 拼接）须由 UT 锁定

---

## Code Review (Stage 135)

**Review Type**: Code
**Review Date**: 2026-10-07
**Reviewer**: Claude (Opus) + 2× general-purpose 对抗式子 agent
**Files Reviewed**: `crates/aigw-core/src/{adapter,models}.rs`、`crates/aigw-core/src/websearch/{mod,trigger,inject}.rs`、`crates/aigw-server/src/{main.rs,routes/{chat,responses,v1_messages,keys,web_search_wire}.rs}`、`crates/aigw-server/tests/{bdd.rs,bdd_steps/web_search_steps.rs,features/web_search.feature}`

### Findings

| # | 严重度 | 位置 | 缺陷 | 处置 |
|---|-------|------|------|------|
| C1 | **Critical** | `chat.rs` / `responses.rs` / `v1_messages.rs` 的搜索插入点 | **`_resolve_enter` span guard 被跨 await 持有** —— 搜索 await 覆盖 provider 超时 + 实例 failover + 重试(秒级)，guard 可能在**另一个 tokio worker 线程**上 drop → tracing-subscriber sharded registry 跨线程 panic（本仓`77dc2eb` 修过的同一类事故）。三条路由的 OAuth 分支都为此 drop 了 guard，新插入的搜索 await 没有 | ✅ **修复**：三处均在 `maybe_serve` **之前** `drop(_resolve_enter)`（chat 亦移除后续重复 drop；OAuth 分支与 adapt 前的重复 drop 一并清理） |
| C2 | High | `chat.rs:1124`（OAuth 分支 `:1183` 之前） | chat 的搜索点位于 OAuth 分支**之前**，设计 §3.6a(a) 冻结的「OAuth 路径逐字一致」被破坏；且 OAuth 非流式 `return` 不调 `attach_status` → **付费搜索对客户端完全不可见** | ✅ **修复**：在 OAuth 非流式 return 前 `attach_status`；span 修复后行为并入 §3.6a 的边界说明（其余两条路由的搜索点本就在 OAuth return 之后） |
| C3 | High | `inject.rs` `question_from_content` / `searxng.rs` `build_url` | **query 无长度上界** —— 最后一条 user 消息含长文档（Claude Code 常态）时，整段文本进 GET URL（非 ASCII 膨胀 3x）→ 数百 KB URL → 414 → 每次请求降级；且整段私密 prompt 外泄给搜索后端 | ✅ **修复**：`MAX_QUESTION_CHARS = 512`（`inject.rs`）+ `SearxngProvider::MAX_QUERY_CHARS = 512`（`build_url` 二次封顶，防绕过）；UT `question_query_is_capped` |
| C4 | Medium | `websearch/mod.rs` `serve_trigger` | backend 答 200 但 `normalize` 把结果全过滤掉（无 url/title、域名名单、去重）时仍注入空模板并报 `ok` —— 客户端被告知搜索有贡献而模型拿到空壳 | ✅ **修复**：`Ok(resp) if resp.results.is_empty()` → `NoTarget` + warn |
| C5 | Low | `inject.rs` `render` | 占位符替换顺序：`{search_results}` 先替换，结果文本里的字面 `{question}` 会被后续替换成用户问题（不可信内容拼进 prompt） | ✅ **修复**：标量占位符先替换，结果块最后拼；UT `render_does_not_resubstitute_placeholders_in_results` |
| C6 | Low | —— | 报告「`mod.rs` 两语句同行会挂 `task fmt`」 | ❌ 不成立（`cargo fmt --check` green）；无处置 |

### 独立验证（每条 finding 均以代码核实，未依赖模型共识）

- C1：`grep -n "_resolve_enter"` 三路由 + 阅读 `77dc2eb` 注释确认语义；修复后 `task test` 从 641 → **643** 全绿
- C2：阅读 chat OAuth 分支的 `:1417`(流式) / `:1524`(非流式) 两个 return 确认 `attach_status` 缺失
- C3：`build_url` 直读确认无长度逻辑；`urlencode` 三倍膨胀直读确认
- C4：`types.rs::normalize` 的六步流水线直读确认可产出 0 结果
- C5：`str::replace` 顺序敏感为 Rust 语义事实

### 门禁（Code Review 后复跑）

- `task test`：aigw-core **643**（+2 修复 UT）/ 全 crate 0 fail
- `task bdd`：**292 场景（279 pass / 13 skip）/ 1510 steps** 不变
- `task fmt` / `task lint` / `task doctor`：全绿

### 补充评审（第二轮，Late Code Review）

首轮 Code Review 后由第二个评审 agent 复核，另报 7 条（1 Medium-High + 3 Medium + 3 Low）。**其中 3 条已在首轮修复**（渲染顺序 / OAuth 非流式 attach / span guard 释放），核实后新增修复 2 条：

| # | 严重度 | 位置 | 缺陷 | 处置 |
|---|-------|------|------|------|
| D1 | Medium | `chat.rs` 缓存 bypass 谓词 | `search_status.is_none()` 把「触发但未搜」（`not_configured` / `no_target`）也当作 bypass，与 `web_search_wire` doc 声称的「配置缺省 = 零行为变化」矛盾 —— 运营方移除 `web_search` 配置后，仍带 `web_search_options` 的请求会**静默失去 exact-match 缓存** | ✅ **修复**：`SearchStatus` 增 `searched: bool`（= `outcome.performed_search()`），缓存谓词改为 `!search_status.as_ref().is_some_and(|s| s.searched)`；doc 同步订正 |
| D2 | Medium | 测试缺口 | `no_target` 分支**零覆盖**；`attach_status` 的**响应体标记从未被 BDD 断言** | ✅ **修复**：新增 BDD 场景「仅含图片的 user 消息不发起搜索且标记 no_target」（断言搜索桩 0 次 + `aigw.web_search.status == "no_target"`）+ 步骤 `响应 aigw.web_search.status 为 {string}`，并把 Chat 触发场景也加上该断言 |
| D3 | Low | `trigger.rs` `detect_anthropic` | `max_uses: 0`（Anthropic 语义 = 禁用工具）被 `filter(>0)` 吞掉后回落 medium → **客户端明确关掉的搜索仍会执行** | ✅ **修复**：`Some(0) => return None`；UT `detect_anthropic_max_uses_zero_suppresses_trigger` |
| D4 | Low | `web_search_wire.rs:107` | `not_configured` 标记 `"provider": ""` | ❌ 保留（形状一致，空串即「无 provider」，无需特殊值） |

### Resolution Summary

**Total Findings**: 10（两轮合计；Critical 1 / High 2 / Medium-High 1 / Medium 3 / Low 4）
**Fixed**: 9 **Wont Fix**: 1（C6 事实不成立；D4 保留）
**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes

### 门禁（补充评审后复跑）

- `task test`：aigw-core **644**（643 → +1 `max_uses:0` UT）
- `task bdd`：**293 场景（280 pass / 13 skip）/ 1519 steps**（+1 图片消息场景）
- `task fmt` / `task lint` / `task doctor`：全绿
