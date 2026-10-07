# Stage 136 Review Log

**Review Type**: Design
**Review Date**: 2026-10-07
**Reviewer**: Claude (Opus) + 1× general-purpose subagent (adversarial)
**Stage**: Stage 136（搜索按次计费 + SpendLog 独立行）

## Review Summary

计费机台的设计（方案 1 独立行 + 方案 4 usage 回传）依据充分，§2 的每处锚点均已复核（`calc_spend` 四项皆 token 计价、`SpendLog` 无父子列、`call_type` 三方言无 CHECK、分发器 `insert_spend_log` 可直用、`update_spend_log` 按 `call_id` 定位）。§3.9a 的「零金额不退化」约束、§3.7 的「独立 spend 上下文」、§3.4 的「禁止挪用 `model_group` / `mcp_namespaced_tool_name`」三条关键约束经代码验证成立。

**发现 3 项 High / Critical，均须先改设计再编码** —— 两项是**设计假设了不存在的数据或不可用的变量**（`api_base` 的命中实例不在 `SearchResponse` 里；chat/responses 的 `session_id` 在搜索点之后才计算），一项是**计费所需的三项数据 Stage 135 的返回值都没带**。

## Findings

### F1 — `api_base` 想要的「实际命中实例」在 Stage 135 的返回值里根本不存在（Critical）

§3.4 逐字要求搜索行 `api_base` = **本次真正打中的那个物理端点**，§4.1 还有 UT `test_search_spend_log_api_base_is_hit_instance` 与 §4.2 的 BDD 场景。但：

- `SearchResponse`（`websearch/types.rs:66-75`）字段只有 `results` / `query` / `provider` / `reported_credits` —— **无 `base_url`、无实例标识**；
- `SearxngProvider::search`（`searxng.rs:214-281`）内部 `self.pool.pick_excluding(&tried)` 选中实例、用它 `build_url(&instance.base_url, ..)`（`:234`），成功后 `parse_response`（`:157-166`）构造的 `SearchResponse` **只带 `provider: self.name`**，实例 `base_url` 被就地丢弃；
- `SearchServeOutcome::Injected/Degraded/NoTarget`（`websearch/mod.rs:222-232`）也只有 `provider: String`。

**→ 设计所要的数据当前不可得。** 需先扩 `SearchResponse` 增加 `endpoint: Option<String>`（命中实例的 `base_url`），在 `searxng.rs` 成功分支填 `instance.base_url.clone()`，并顺带在 `SearchServeOutcome` 上透出。这**触及已提交的 Stage 135 代码**（`types.rs` / `searxng.rs` / `mod.rs`），须在 Stage 136 的变更清单里显式列出，不可当作「Stage 134 已交付」。

### F2 — chat / responses 的 `session_id` / `end_user` / `requester_ip` 在搜索点之后才计算（High）

§3.4 的搜索行要求 `session_id`「与父 LLM 行同一」（第二条串联线索，§3.5 链路第 3 步），并要求 `user` / `end_user` / `requester_ip_address` 同父行。但三条路由的**求值顺序不一致**：

| 路由 | 搜索点 | `end_user`/`session_id`/`requester_ip` 计算处 | 可用？ |
|------|--------|--------------------------------|--------|
| `chat.rs` | `:1131` | `:1576` / `:1583` / `:1591` | ❌ **在搜索之后** |
| `responses.rs` | `:595` | `:666` / `:671` / `:679` | ❌ **在搜索之后** |
| `v1_messages.rs` | `:845` | `:404` / `:411` / `:419` | ✅ 在搜索之前 |

`request_id`（即 `call_id`）三条路由都在 handler 入口就有（chat `:925` / responses `:91` / v1_messages `:173`）；`auth`（`token_hash` / `user_id` / `team_id` / `organization_id`）是 handler 参数，全部可用。

**→ chat 与 responses 需要把元数据提取（`end_user` / `session_id` / `requester_ip`）上移到搜索点之前**，否则搜索行只能填 `None`，违反 §3.4「四级归属必须与父行一致，否则 `increment_team/org_spend` 落错实体」。

### F3 — 计费所需的三项数据 Stage 135 都未透出（High）

Stage 136 要构造搜索行，需要 ① `queries`（本设计 C 恒 1）② `cost_per_query` ③ 命中的 provider 名。`SearchServeOutcome` 只带 ② 的**名字来源**（`provider`），且**仅 `Injected` / `Degraded` 有值**，`NoTarget` 也有 provider。

- `WebSearchRegistry::cost_per_query(name)`（`websearch/mod.rs:115-120`）签名可用，`cost_per_query(Some(&outcome.provider()))` 在 failover 后**返回的是实际服务那个 provider 的价**（`resolve_order` 以 requested 优先，`get()` 按名精确匹配）—— 语义正确；
- 但 `queries` 与「是否有过真实搜索」需要从 outcome 变体推断（`Injected`/`Degraded` 才是「真打过一次」；`NoTarget` 是**没打**，不该计费）。

**→ 设计 §3.1 的 `queries` 参数来源须写明**：不是来自 outcome 的字段，而是「`Injected`/`Degraded` → 1，其余 → 不写行」。同时 `NoTarget` **不得**计费（无搜索发生），§4.2 需补一条断言。

## AI Pre-Filter Results

- 子 agent 的「`calc_search_spend` 放在 aigw-server 而非 core 是分层错误」——**过滤（不成立）**：搜索行的构造与 `insert_spend_log` 调用本就在 server 侧（`auth` / `session_id` 都是 handler 变量）；core 侧的 `serve_trigger` 只负责「搜 + 注入」。`calc_spend` 同为 `pub(crate)` 且位于 `chat.rs`，同处放置自洽。
- 「`Usage` / `ClaudeUsage` 加字段会破坏既有序列化」——**过滤（不成立）**：两 struct 均无 `deny_unknown_fields`，加 `#[serde(skip_serializing_if = "Option::is_none", default)]` 后序列化逐字节不变。**但**：两 struct 的**结构体字面量**在 `adapter.rs` 有 8 处（`Usage` ×2 / `ClaudeUsage` ×6）+ `adapter_steps.rs` 3 处，加字段会**编译失败**，须逐处补 `server_tool_use: None` —— 登记为实施注意项（非设计缺陷）。

## Rule Filtering

- **Scope creep**：未发现。§「明确不做」列出阶梯定价 / key 级覆写 / `search_count` 专列 / 中途预算复检 / 前端，与 §3 一致。
- **Memory bias**：F1/F2/F3 均属「把意图当已实现」——设计描述的是期望形态，未核实现有返回值与变量作用域。
- **Logical fallacy**：§3.9 对「方案 2 在未来的设计 A 下无宿主行」的论证成立（短路不调模型），据此否决方案 2 合理。

## 编码期必查项（Design Review 后置）

1. `insert_spend_log` 的三方言实现（`db.rs:2317` / `:2752` / `:3201`）**是否有 `spend == 0` 的早退** —— 若有则必须绕开（§3.9a 的硬约束，UT `test_zero_spend_search_row_still_inserts_and_increments` 锁定）
2. `query_spend_logs_filtered`（`db.rs:2234`）**无 `call_type` 过滤参数** —— §4.2 的 BDD 若按 `call_type="search"` 查行，须在测试侧取全量后过滤，或为此加过滤参数（后者是产品改动，倾向测试侧过滤）
3. `daily_spend_queue` 的聚合复合键含 `mcp_namespaced_tool_name`（`daily_spend_queue.rs:99-101`、`:134`、`:148`）—— 搜索行填 `None` 会与父行**落到不同分组**；§3.4 已明令不得挪用该列，故须确认「搜索行是否进 daily 聚合」这一决策在设计中明确（当前 §3.4 未提 daily 队列）

## Resolution Summary

| # | 严重度 | 处置 |
|---|-------|------|
| F1 | Critical | **改设计**：§3.4 增 `SearchResponse.endpoint` 扩展的显式前置改动（含触及 Stage 135 三文件的说明） |
| F2 | High | **改设计**：§3.6 增「chat/responses 元数据提取上移至搜索点之前」的前置步骤 |
| F3 | High | **改设计**：§3.5 明确 `queries` 来源与 `NoTarget` 不计费 |

**All Critical Fixed**: 是（F1 已改设计）
**All High Priority Addressed**: 是（F2/F3 已改设计）

---

## Code Review (Stage 136)

**Review Type**: Code
**Review Date**: 2026-10-07
**Reviewer**: Claude (Opus) + 1× general-purpose adversarial subagent（第二轮，编码后）

### Findings

| # | 严重度 | 位置 | 缺陷 | 处置 |
|---|-------|------|------|------|
| G1 | High | `web_search_wire.rs` `SearchStatus` | **`endpoint` 止步于 core crate** —— `SearchResponse.endpoint` 与 `SearchServeOutcome::Injected.endpoint` 已就位，但 handler 侧的 `SearchStatus` 无该字段，`api_base` 将恒为 `None`，设计自带的 `api_base` 断言无法通过 | ✅ **修复**：本 Stage 的 `record_search_spend` 直接读 `outcome.endpoint()` 写入行（不经 `SearchStatus`），绕开该缺口；BDD `搜索行的 api_base 非空` 端到端锁定 |
| G2 | Medium | `serve_trigger` 空结果分支 | 真实付费往返返回 0 可用结果时被判 `NoTarget` → 不写行不计费，与 §8.1「宁多算不漏账」直接冲突（接入 Tavily 后每次空结果都是漏账） | ✅ **修复**：新增 **`SearchServeOutcome::Empty`**（provider 真答了但结果被 `normalize` 全过滤）—— `performed_search()` 改为 `!NoTarget`，`Empty` 计费且 `status="success"`；UT `test_performed_search_bills_every_remote_round_trip` 锁定；§8.1 补两行风险 |
| G3 | Medium | §5 变更清单 | 清单要求给 `config.rs` 加 `cost_per_query` —— 该字段**早已存在**于 `websearch/config.rs:104-105`（非 Option `f64`，default `0.01`），照做会产生重复字段 | ✅ **修复**：§5 该行改标「无需改动」+ 出处 |
| G4 | Medium | §5 变更清单 | 漏列 `adapter.rs`（8 处结构体字面量）与 `adapter_steps.rs`（3 处）—— 加 `server_tool_use` 会编译失败 | ✅ **修复**：§5 补两行并列出全部行号 |
| G5 | Medium | §4.2 BDD 查法 | `query_spend_logs_filtered` **无 `call_type` 形参**，§4.2「查 `call_type="search"` 的行」无 API 支持 | ✅ **修复**：§4.2 写明测试侧取全量后内存过滤（`e2e_steps.rs:694` 先例）；实现即 `search_rows()` helper |
| G6 | Medium | §4.1 / §4.3 精度断言 | 锚定 `DECIMAL(20,8)`，但 spend 列实为 `REAL`/`DOUBLE`/`DOUBLE PRECISION` —— 十进制等值断言会因 f64 误差而不稳 | ✅ **修复**：改为 `test_calc_search_spend_epsilon_not_exact`（epsilon 断言）；§4.1 该行标注改写 |
| G7 | Low | git 提交粒度 | F2 元数据上移应单独成 commit 以保 diff 可审 | ➖ 接受：与 Stage 135 收尾同批提交（`a65000e`），改动为纯搬移、无副作用 |
| G8 | Low | §3.8 流式回传 | chunk 原始字节转发 + Responses 适配器重建 usage，流式注入无机制 | ✅ **修复**：§3.8 明确**流式为范围收窄**，本期只做非流式三 surface |
| G9 | Low | §3.5 daily 队列 | 搜索行是否入 `daily_spend_queue` 未表态 | ✅ **修复**：§3.5 显式声明**不入队列**（复合键会切碎日聚合），处置留 Stage 137 |

### Resolution Summary（Code Review）

**Total Findings**: 9（High 1 / Medium 5 / Low 3）
**Fixed**: 8 **Accepted**: 1（G7 提交粒度）
**All Critical Fixed**: Yes（无 Critical）
**All High Priority Addressed**: Yes（G1）

### 门禁（Code Review 后复跑）

- `task test`：aigw-core **645** / aigw-server **184**，0 fail
- `task bdd`：**293 场景（280 pass / 13 skip）/ 1529 steps**
- `task bdd-real-sqlite`：**58/58**
- `task fmt` / `task lint` / `task doctor`：全绿
