# Stage 136: 搜索按次计费 + SpendLog 独立行（Phase 53）

**所属**: Phase 53（内建 Web Search / TD-017c）
**预估**: 8h（按次计价函数 + 搜索行构造 + 预算接入 + usage 回传 + UT/BDD）—— 由原 10h 下调，省下的是**三 provider 价目核对 + Tavily 控制台次数核对 + Bocha CNY/USD 人工偏差核对**三项真实厂商对账；计费机台本身一行不减（且新增「实例级 `api_base` 溯源」与「0 单价不退化」两项），故不能再往下压
**依赖**: Stage 135（注入接线 —— 搜索执行结果已可在请求路径内取到 provider 名与 query 数）
**状态**: ⏳ 规划

> **Phase 53 范围收窄（2026-10-06 决策）与本 Stage 的关键后果**
>
> 本期只实现 **SearXNG** 一家 provider（多 provider 架构全部保留，Tavily / Bocha 是纯增量的后续接入项）。但**这不等于金额恒为 0**：
>
> **`cost_per_query` 缺省为 `0.01` USD/次**（= **$10/1k**，对齐 OpenAI 官方 web search 牌价与 sub2api `174_group_web_search_price_per_call.sql` 的 `0.01` 默认）。语义是「部署方应按自建摊销成本改写的单价」—— 自建实例有服务器、带宽、运维成本，只是不由外部厂商开票。因此：
>
> | 部署选择 | 生产后果 |
> |---------|---------|
> | **不改默认 `0.01`**（开箱即用） | 搜索行 `spend = 0.01 × N` —— **非零路径默认即在生产可达**；但 $10/1k 是 OpenAI 的**对外售价**，自建摊销通常在 `1e-4` 量级 → **系统性高估约 50 倍**（§8.1） |
> | 改为自建摊销单价（推荐） | `(服务器+带宽+运维) / 预估月查询量`，金额贴近真实成本 |
> | 显式填 `0.0` | 金额为 0，但**行与四级预算调用照样发生**（§3.9a）—— 自建成本在账面不可见，可接受但不推荐 |
>
> **选 `0.01` 而非 `0.0` 的理由**：宁可高估也不要静默为 0 —— 成本**可见**优先于成本**精确**，`0.0` 会让「搜索花了多少」永远无法回答；而高估是看得见、可一行配置修正的。
>
> **UT 的分工因此是「补边界」而非「唯一途径」**：缺省非零意味着生产即覆盖典型非零路径；UT 另用 Stage 134 的测试专用 `StubProvider`，覆盖**边界值与精度**（极小单价、1000 次累乘、`NaN`/负数、8 位小数无损性）以及**显式 `0.0`** 这一分支（§3.9a）——这些不该靠生产流量去碰。
>
> **仍然必须守住的工程约束**：`spend == 0.0`（部署方把单价显式置 0）时**不得退化为 no-op** —— 行照样 INSERT、`increment_*_spend(0.0)` 照样调用。否则该部署会直接丢掉全部搜索调用记录，且后续改填非零单价需要改代码（§3.9a + §4.1 新增 UT）。
>
> **Bocha CNY→USD 汇率决策从实现阻塞项降级为延后项**（它只因 Bocha 按人民币计价才成立，而 Bocha 本期不实现），详见 §8.2。
>
> **provider 两层模型**：Stage 134 已改为 **provider（逻辑后端，持 `kind` 与定价）→ instances[]（物理端点，各自 `base_url` / `weight` / `enabled` / 冷却）**。本 Stage 的落库口径随之明确：`custom_llm_provider` = provider 名（做聚合），`api_base` = **实际命中实例的 `base_url`**（做实例溯源），见 §3.4。

---

## 1. 目标

Stage 134/135 让网关在一次客户端请求里**先搜后问**（设计 C，prompt 注入）：每个客户端请求 = **1 次搜索调用 + 1 次正常上游模型调用**。但搜索这次调用**不花 token、按次收费**，而 aigw 当前**任何非 token 计价都不存在**（§2.1）。结果是：搜索成本既不入 `spend_logs`、也不入 `virtual_keys.spend`，网关自掏腰包，预算形同虚设。

本 Stage 引入「按次（per-call）计价」这一全新计价维度，并把每次搜索落为一条**独立 SpendLog 行**（零 migration），同时按协议回传 `usage.server_tool_use.web_search_requests`。

### 验收标准

- [ ] 新增 `calc_search_spend(queries, cost_per_query) -> f64`，有独立 UT 覆盖精度与边界。非零单价在生产真实可达（部署方填自建摊销单价，见文首），UT 额外用 `StubProvider` 覆盖**边界值与精度**（极小单价 / 1000 次累乘 / `NaN`/负数 / 8 位小数无损）
- [ ] 每次搜索产生一条 `call_type="search"` 的 SpendLog 行，`spend` 正确（= `queries × 配置单价`，未填单价时为 `0.0`）、token 列全 0、`metadata.parent_call_id` 指向触发它的 LLM 行 `call_id`
- [ ] **`spend == 0.0` 不得退化为 no-op**：部署方把 `cost_per_query` 显式置 0 时行照样 INSERT、四级 `increment_*_spend(0.0)` 照样调用 —— 否则该部署会丢掉全部搜索调用记录，且后续改填非零单价需要改代码（UT `test_zero_spend_search_row_still_inserts_and_increments` 锁定）
- [ ] 搜索费计入 key/user/team/org 四级 `increment_*_spend`（否则预算不扣，见 §3.5）；0 金额不得打断累加（`+= 0.0` 必须真实发生且不影响既有累计值）
- [ ] `api_base` 记录**实际命中实例**的 `base_url`（provider 多实例，§3.4）；`custom_llm_provider` 仍为 provider 名
- [ ] 流式模型调用下搜索行照样写入，且不被 Phase 2 UPDATE 覆盖/清除（§3.6）
- [ ] 三个 surface（chat / responses / messages）的响应体按各自形状回传搜索次数
- [ ] 每次搜索与每次模型调用各自持有**独立的 spend 上下文**（§3.7 litellm 坑）
- [ ] `task test` / `task test-bdd` / `task fmt` / `task lint` 全绿

### 明确不做（边界）

- **阶梯定价**（litellm `tiered_pricing`：按 `max_results` 区间取价）—— 本期单价恒定，候选 provider 均无阶梯
- **key / team 级搜索单价覆写** —— 本期仅全局配置（§3.3 给出理由）
- **`search_count` 专列 + migration**（方案 3）—— 登记 §8.2，待「上游原生搜索次数」出现时再做
- **请求中途预算复检** —— 设计 C 每请求恒定 1 次搜索，无 loop，超支上限可预测；loop 场景（设计 B）才需要
- **上游原生 web search 的计费**（`search_context_cost_per_query`）—— 不在设计 C 范围内
- **Tavily / Bocha 的 provider 实现与真实端到端对账** —— Phase 53 只实现 SearXNG（见文首收窄说明）；两家的单价已在 §3.2 表中预留，接入是纯增量
- **CNY→USD 汇率换算实现** —— 仅登记决策需求（§8.2）。⚠️ **本期它甚至不再是配置项要求** —— Bocha 不实现，故无 CNY 单价需要填；该决策**必须在 Bocha 实际接入前落地**
- **前端展现** —— Stage 137

---

## 2. 现状证据

### 2.1 缺口 A — 按次 / 非 token 计价**完全不存在**

唯一活跃的成本函数 `calc_spend`（`crates/aigw-server/src/routes/chat.rs:104-123`）的全部四项都是 `tokens × 单价`：

```rust
    regular * base_input
        + cache_read_tokens as f64 * read_cost
        + cache_creation_tokens as f64 * create_cost
        + completion_tokens as f64 * output_cost.unwrap_or(0.0)
```

没有任何常数项、没有任何 flat fee 入口。

| 检查项 | 结果 | 锚点 |
|--------|------|------|
| `input_cost_per_request` / `per_call` / `input_cost_per_image` / `input_cost_per_second` | 全仓零命中 | 调研实证：`docs/research/2026-10-06-aigw-websearch-codebase-map.md` §C3 |
| `ModalPricing` 是否按次 | **否** —— 字段注释逐字为 `USD per 1M image input tokens`（`crates/aigw-core/src/models.rs:136-141`），命名误导，实为 token 计价 | `models.rs:134-142` |
| `calc_spend_modal` 是否在活跃路径 | **否** —— `#[allow(dead_code)]`（`chat.rs:138`），注释自述 `Wired into the embeddings spend path once a request carries per-modality input tokens`（`chat.rs:135-137`） | `chat.rs:139-158` |
| embeddings 是否叠加固定费 | **否** —— 调 `calc_spend` 且 `completion_tokens=0`，纯 prompt token 计价 | `crates/aigw-server/src/routes/embeddings.rs:531-540` |

**→ 结论**：`calc_search_spend` 是 aigw 的**第一个**非 token 计价函数，无先例可复用。

### 2.2 缺口 B — `usage.server_tool_use.web_search_requests` 既不解析也不保留

三个 usage struct 全无搜索计数位，且**无 `#[serde(flatten)]` 兜底**，上游若回传该字段会在反序列化时静默丢弃：

| struct | 锚点 | 字段 |
|--------|------|------|
| `Usage`（OpenAI 形） | `crates/aigw-core/src/models.rs:665-676` | `prompt_tokens` / `completion_tokens` / `total_tokens` / `prompt_tokens_details` / `completion_tokens_details` |
| `TokenDetails` | `models.rs:679-691` | `cached_tokens` / `reasoning_tokens` / `audio_tokens` / `accepted_prediction_tokens` / `rejected_prediction_tokens` |
| `ClaudeUsage`（Anthropic 形） | `models.rs:1151-1161` | `input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` |

官方形状（Anthropic，`docs/research/2026-10-06-gateway-websearch-architecture.md` §4.4）：`usage.server_tool_use.web_search_requests: <int>`。

### 2.3 缺口 C — SpendLog 无父子关系

`SpendLog` struct（`crates/aigw-core/src/models.rs:146-191`，37 字段）**无 `parent_call_id`、无任何 self-referencing FK**；全部 migration 中无任何 FK 指向 `spend_logs` 自身。

可复用的「挂载位」：

| 列 | struct 锚点 | 现状 |
|---|---|---|
| `call_type` | `models.rs:148` | **判别列**。现有取值三个：`"completion"`（`chat.rs:2583`、`chat.rs:1461`、`chat.rs:1915`、`v1_messages.rs:1635`）、`"responses"`（`v1_messages.rs:382` 按 `is_stream` 选值）、`"embedding"`（`crates/aigw-server/src/routes/embeddings.rs:454`、`:555`）。**三方言 schema 均为 `TEXT`/`VARCHAR(255)` 无 CHECK 约束 → 可自由新增取值**；`"embedding"` 的引入即为「新增取值零 migration」的既有先例 |
| `metadata` | `models.rs:164` | 无 schema 约束的自由 JSON，已有先例键 `cache_read_tokens` / `cache_creation_tokens` / `image_tokens_source`（`chat.rs:2607` 起构造） |
| `session_id` | `models.rs:174` | 松散会话分组，chat 侧从 `session_id` 变量透传（`chat.rs:2656` 附近） |

三处手写 INSERT（列清单完全一致、仅占位符方言不同）：

| driver | 锚点 |
|--------|------|
| SQLite | `crates/aigw-core/src/db.rs:2317` |
| MySQL | `db.rs:2752` |
| PostgreSQL | `db.rs:3201` |
| 分发器 `Database::insert_spend_log` | `db.rs:3621` |

**→ 结论**：新增搜索行**零 migration、零 SQL 改动** —— 走 `db.rs:3621` 分发器即可，不必碰那三份手写 SQL。

### 2.4 缺口 D — 预算与 spend 聚合不会自动纳入搜索费

成本增量 trait（`crates/aigw-core/src/db.rs:365-368`）：

```rust
    async fn increment_key_spend(&self, token_hash: &str, cost: f64) -> Result<()>;
    async fn increment_user_spend(&self, user_id: &str, cost: f64) -> Result<()>;
    async fn increment_team_spend(&self, team_id: &str, cost: f64) -> Result<()>;
    async fn increment_org_spend(&self, org_id: &str, cost: f64) -> Result<()>;
```

实现 `db.rs:955` / `:964` / `:973` / `:982`（SQLite）、`db.rs:1392` 起（MySQL）、`db.rs:1834` 起（Postgres），目标列 `virtual_keys.spend` / `users.spend` / `teams.spend` / `organizations.spend`。

现有调用点（**每处只传一个 `cost`，即该行的 `spend`**）：

| surface | 非流式 | 流式 Phase 2 |
|---------|--------|-------------|
| chat | `crates/aigw-server/src/routes/chat.rs:2676-2684`；OAuth 分支 `chat.rs:1505` | `chat.rs:2343-2351` |
| responses | `crates/aigw-server/src/routes/responses.rs:1588-1590` 起 | `responses.rs:1301-1303` 起 |
| messages | `crates/aigw-server/src/routes/v1_messages.rs:1683-1685` 起 | `v1_messages.rs:1447-1449` 起 |
| embeddings | `crates/aigw-server/src/routes/embeddings.rs:601-612` | — |

预算检查侧：`check_entity`（`crates/aigw-core/src/budget.rs:45-57`，`spend > limit` 严格大于）；多级入口 `check_budget`（`budget.rs:116`）/ `check_budget_multi`（`budget.rs:152`）。**预算读的是 `virtual_keys.spend` 等累加列，不读 `spend_logs`。**

**→ 若搜索行只进 `spend_logs` 而不调 `increment_*_spend`，后果（必须写进验收）**：
1. 预算永远不会因搜索费触顶 —— 一个 `max_budget=$1` 的 key 可以无限搜索；
2. `spend_logs` 的 `SUM(spend)` 与 `virtual_keys.spend` **口径不一致**，对账直接失效；
3. `check_budget` 的「请求前检查」看到的是偏低的累计值，超支只会在下一次请求前被发现 —— 而那个值永远不涨。

### 2.5 缺口 E — 流式 SpendLog 刚被动过，有同类陷阱

`git show 8cf8c12 --stat`：

```
 crates/aigw-server/src/routes/responses.rs         | 84 +++++++++++++++++++++-
 .../aigw-server/tests/bdd_steps/responses_steps.rs | 22 ++++++
 .../aigw-server/tests/features/responses.feature   |  1 +
```

commit message 根因原文：「`assembled_response` 在非空分支里只取 `chunk_jsons.last()`。流式 SSE 的最后一个 chunk 只带 `finish_reason`，delta 为空——文本全在之前的 chunk 里。」修复把单取 last 改为**遍历全部 chunk 累加重建** assistant 消息（`responses.rs:1105` 起，diff 实证）。

**→ 对本 Stage 的含义**：流式路径的落库 `response` 是**从上游 chunk 重建**出来的。搜索调用的产物**不在任何上游 chunk 里**（是网关自己产的），任何「从 chunk 重建」的逻辑都会让它凭空消失。故搜索必须走**独立行**、并在模型调用 Phase 1 之前/并行写入，不依赖 Phase 2 的重建结果。

### 2.6 外部定价先例

| 来源 | 结论 | 锚点 |
|------|------|------|
| sub2api | groups 表加 `web_search_price_per_call DECIMAL(20,8)`，`NULL` 表示用内置默认 **0.01 USD/次** | `~/works/play/sub2api/backend/migrations/174_group_web_search_price_per_call.sql` |
| OpenAI 官方 | Web search **$10.00 / 1k calls**（= $0.01/call）为当前规范价；**$25/1k 仅存活于「`web_search_preview` + 非推理模型」**这一特例 | 架构调研 §4.3（`https://developers.openai.com/api/docs/pricing`） |
| litellm | `search_provider_cost_per_query` 返回 `(queries × cost_per_query, 0.0)`，**output_cost 恒为 0**；`searxng/search` 价为 **0.0** | 架构调研 §2.8 路径 1 |

---

## 3. 方案

**定案 = 方案 1（独立行）+ 方案 4（usage 回传）**，依据架构调研 §5.6「推荐」表：阶段 1（设计 C）选此组合，理由是零迁移 + 账目清晰 + 搜索与模型天然就是两次独立调用。

### 3.1 新增按次计价函数

位置：`crates/aigw-server/src/routes/chat.rs`（与 `calc_spend` / `calc_spend_modal` 同处，保持成本函数聚拢）。

```rust
/// Per-call (non-token) cost — aigw's first flat-fee pricing unit.
/// Mirrors litellm `search_provider_cost_per_query`: total = queries × unit,
/// and there is no output-side cost (always 0).
pub(crate) fn calc_search_spend(queries: i32, cost_per_query: Option<f64>) -> f64 {
    let unit = cost_per_query.unwrap_or(0.0);
    if queries <= 0 || !unit.is_finite() || unit <= 0.0 {
        return 0.0;
    }
    queries as f64 * unit
}
```

设计点：
- `cost_per_query: Option<f64>`。**配置层缺省填充 `0.01`**（Stage 134 的 `#[serde(default = "default_cost_per_query")]`，对齐 sub2api `174_*.sql` 的 `0.01`）；函数签名保留 `Option` 以与 `calc_spend` 风格一致，`None` 兜底为 `0.0`（仅当配置被显式置空时可达）。**显式 `0.0` 只让金额为 0，不让落库与预算调用消失**（§3.9a）
- 负数 / `NaN` / `inf` 一律归零，防止脏配置污染 `virtual_keys.spend`（该列无约束）
- `queries` 来自 Stage 135 的搜索执行结果（设计 C 下恒为 1，但函数按 N 设计，为后续 loop 留口）

### 3.2 provider 单价（USD/query）

| provider | 牌价 | `cost_per_query` (USD) | 本期状态 | 来源 |
|----------|------|------------------------|---------|------|
| `searxng` | 自建，**无外部开票但有自建成本** | **缺省 `0.01`**（牌价占位）→ 应改为摊销值（典型 `1e-4` 量级） | ✅ **本期唯一实现** | 默认值对齐 OpenAI $10/1k 与 sub2api `174_*.sql` 的 `0.01`；**不取 litellm 的 `searxng/search = 0.0`**，因为 0 会让自建成本永久不可见 |
| `tavily` | 1000/mo 免费，之后 $8/1k | **0.008** | ⏳ 待接入（价目预留） | 与 litellm 价表 `tavily/search` = 0.008 一致 |
| `bocha`（博查） | ¥36/1k ≈ $5.07/1k | **0.00507**（⚠️ 换算值，见 §8.2） | ⏳ 待接入（价目预留） | 任务上下文给定；**CNY 牌价，非 USD 原生** |

> ⚠️ **SearXNG 的 `0.01` 是牌价占位，不是事实单价**。$10/1k 是 OpenAI 的对外售价；自建的服务器/带宽/运维成本真实存在但通常**远低于**该值（典型 `1e-4` 量级，相差约 50 倍）。**配置文档与 Stage 138 的 UI 必须提示部署方改写为摊销单价**（`月度基础设施成本 ÷ 月均查询数`），否则预算、Usage、对账三处金额被系统性放大（§8.1 风险）。显式填 `0.0` 则一切照常运行、金额为 0 —— **可接受但不推荐**（成本不可见）。
>
> 上表后两行是**为后续接入预留的价目锚点**，本期不会被任何生产配置消费。

> **免费额度不建模**：Tavily 的 1000/mo 免费配额是账单侧事实，网关侧按牌价计提（与 litellm 一致）—— 否则需要跨月计数器状态，复杂度远超收益。差额由财务侧对账吸收，登记 §8.1。该取舍在 Tavily 实际接入时才生效。

### 3.3 单价配置位置 —— 本期仅全局配置

**决策：`cost_per_query` 只在全局配置（`AigwConfig` 下的搜索配置块，由 Stage 134 定义 provider 抽象时落位）配置，不做 key/team 级覆写。**

理由（奥卡姆）：

| 维度 | 全局配置 | key/team 级覆写 |
|------|---------|----------------|
| 代价 | 零 migration；单价与 provider 定义同处，配置方一次填完 | 需要 migration（仿 sub2api `174_*.sql` 给 `virtual_keys`/`teams` 加 `DECIMAL(20,8)` 列）+ 解析 + 覆写优先级链 |
| 语义 | 单价是**网关对上游的真实采购成本**，与哪个 key 发起无关 —— 这是事实，不是策略 | 覆写表达的是**加价/折扣策略**（sub2api 的 `groups` 正是售卖分组），aigw 目前**无任何售卖加价机制**（`calc_spend` 里也没有 ratio/markup 概念） |
| 结论 | ✅ 本期采用 | ❌ 引入即意味着先引入「加价」这一产品概念，超出 Phase 53 范围 |

→ 登记为 §8.2 的未来路径：若 aigw 后续引入 group/markup 定价层，按 sub2api `DECIMAL(20,8)` 列形态补 key/team 级覆写。

### 3.4 搜索行的精确构造

**复用现有 INSERT**：调 `Database::insert_spend_log`（分发器 `crates/aigw-core/src/db.rs:3621`）。该分发器下的三份手写 SQL（`db.rs:2317` / `:2752` / `:3201`）**一行不改** —— 搜索行只是 `SpendLog` 的一组新取值，不新增列。

**逐列取值表**（对照 `crates/aigw-core/src/models.rs:146-191`）：

| 列 | struct 锚点 | 搜索行取值 | 说明 |
|---|---|---|---|
| `call_id` | `models.rs:147` | **新 UUID v7** | 与父 LLM 行各自独立 PK |
| `call_type` | `models.rs:148` | `"search"` | 第四个取值（现有 `"completion"` / `"responses"` / `"embedding"`）；schema 无约束，`"embedding"` 即先例 |
| `api_key` | `models.rs:149` | `auth.token_hash.clone()` | 与父行同一 key，归属必须有主（litellm 的教训：无主花费会被 spend hook 丢弃，架构调研 §2.8） |
| `spend` | `models.rs:150` | `calc_search_spend(queries, cost_per_query)` | |
| `total_tokens` / `prompt_tokens` / `completion_tokens` | `models.rs:151-153` | **0 / 0 / 0** | 搜索不花 token。注意这三列 `NOT NULL DEFAULT 0`，填 0 合法 |
| `start_time` / `end_time` | `models.rs:154-155` | 搜索调用的真实起止 | 不复用父行时间 —— 搜索发生在模型调用**之前** |
| `request_duration_ms` | `models.rs:156` | 搜索耗时 | 对可观测性有实际价值（搜索慢会拖长 TTFT） |
| `completion_start_time` | `models.rs:157` | `None` | 无流式概念 |
| `model` | `models.rs:158` | `"<provider>/search"`（本期恒为 `"searxng/search"`） | 与 litellm 价表 key 同形，`aggregate_spend_by_model`（`db.rs:3764`）可直接按此聚合。**用 provider 名而非实例名** —— 同 `custom_llm_provider` 的理由 |
| `model_id` | `models.rs:159` | `None` | 无 deployment |
| `model_group` | `models.rs:160` | `None` | ⚠️ **不复用为搜索工具名**。litellm 把 `model_group` 占用为搜索工具名（架构调研 §2.8），但 aigw 的 `aggregate_spend_by_model_group`（`db.rs:3787`）与前端「Spend by Model Group」图直接消费该列，塞工具名会污染既有图表 |
| `custom_llm_provider` | `models.rs:161` | **provider 名**（本期恒为 `"searxng"`；后续 `"tavily"` / `"bocha"`） | `spend_providers`（`crates/aigw-server/src/routes/spend.rs:839`）可据此出「搜索厂商花费」。**是 provider 名而非实例名** —— 聚合维度必须跨实例稳定 |
| `api_base` | `models.rs:162` | **实际命中实例的 `base_url`** | ⚠️ Stage 134 已改为 provider→instances[] 两层模型（实例选择照抄 `router.rs` 的 `cooldown_until:26` / `has_weight:366-377` / `weighted_pick:419`）。此列必须记**本次真正打中的那个物理端点**，而非 provider 名或配置里的第一个实例 —— 否则多实例部署下无法按实例对账、无法定位「哪台 SearXNG 在拖慢/报错」。与 `custom_llm_provider` 分工：**provider 名做聚合，`api_base` 做实例溯源** |
| `user` | `models.rs:163` | 同父行 | |
| `metadata` | `models.rs:164` | `{"search_query_count": N, "parent_call_id": "<父行 call_id>", "search_provider": "<provider>", "cost_per_query": <unit>}` | 单价快照进 metadata，便于历史对账（配置改价后旧行仍可复算） |
| `cache_hit` / `cache_key` | `models.rs:165-166` | `None` / `None` | |
| `request_tags` | `models.rs:167` | 同父行 | 保证 tag 维度日聚合口径一致 |
| `team_id` / `organization_id` / `end_user` / `requester_ip_address` | `models.rs:168-171` | 同父行 | 四级归属必须与父行一致，否则 `increment_team/org_spend` 落错实体 |
| `messages` | `models.rs:172` | `{"query": "<搜索词>"}` | 让详情抽屉有东西可渲染（Stage 137 消费） |
| `response` | `models.rs:173` | `{"result_count": M, "results": [...]}`（按 body_archive 策略截断） | 同上 |
| `session_id` | `models.rs:174` | **与父 LLM 行同一 `session_id`** | 第二条串联线索（§3.8） |
| `status` | `models.rs:175` | `"success"` / `"failure"` | 复用既有三态语义；搜索无流式，**永不为 `"streaming"`** |
| `mcp_namespaced_tool_name` | `models.rs:176` | `None` | ⚠️ 该列已被 `daily_spend_queue` 的聚合复合 key 使用（`crates/aigw-core/src/daily_spend_queue.rs` 聚合键含此列），填值会裂开日聚合分组 |
| `agent_id` | `models.rs:177` | 同父行（当前全路径恒 `None`） | |
| `proxy_server_request` | `models.rs:178` | `None` | 搜索请求非客户端原始请求 |
| `body_archived` / `parquet_path` | `models.rs:179-180` | `false` / `None` | 搜索 body 体积小，走热路径 |
| `request_id` | `models.rs:186` | 搜索 provider 返回的 request id（有则填） | 该列语义是**上游 provider id**（`models.rs:182-185` 注释），与 `parent_call_id` 无关，**不可挪用** |
| `image_tokens` | `models.rs:191` | `None` | |

### 3.5 父子关联：`metadata.parent_call_id`

**链路**（设计 C 下时序明确：搜索在模型调用之前）：

1. 请求入口先生成 LLM 行的 `call_id`（即现有 `request_id` 变量，chat 侧 `chat.rs:2580` 处 `call_id: request_id.clone()`）—— **此值在搜索发生前就已存在**，无需重排时序
2. 搜索执行 → 得到 `queries` / `provider` / 耗时 / 结果
3. 构造搜索行，`metadata.parent_call_id = <步骤 1 的 call_id>`，`session_id = <父行 session_id>`
4. `insert_spend_log(搜索行)` + `increment_{key,user,team,org}_spend(搜索 spend)`
5. 模型调用照常走现有路径（其自身的 `insert_spend_log` + `increment_*` 一字不改）

> 关键：**步骤 4 的 `increment_*_spend` 是第二组调用**，与模型侧的 `chat.rs:2676-2684` / `responses.rs:1588` / `v1_messages.rs:1683` 并列而非替代。这正是 §2.4 指出的必须项。

**为何不塞进父行 metadata**：N 次搜索塞一个 JSON 数组虽技术可行，但无法按次计费归集、无法分页、无法独立统计，与 `/spend/logs` 的分页模型冲突（基线地图 §B4「不可行的路线」）。

### 3.6 流式：搜索行与 Phase 2 UPDATE 不相干

| 阶段 | 做什么 | 现有锚点 |
|------|--------|---------|
| 搜索 | 独立 INSERT + 独立 `increment_*`，**立即终态**（`status="success"`） | 本 Stage 新增 |
| 模型 Phase 1 | 占位 INSERT（`status="streaming"`、`response: {"status":"streaming"}`） | `chat.rs:1987`→`:2025`；responses `responses.rs:360`→`:398` |
| 模型 Phase 2 | 按 `call_id` **UPDATE 父行**（token / spend / 重建 response） | `chat.rs:2241`、`chat.rs:2313`；`responses.rs:1208`、`responses.rs:1275` |

`update_spend_log`（分发器 `crates/aigw-core/src/db.rs:3630`）按 `call_id` 定位 —— 搜索行有**自己独立的 `call_id`**，故 Phase 2 UPDATE 在物理上就不可能触及它。**这是选独立行（而非方案 2 的「并入同行」）的额外收益**：绕开了 commit `8cf8c12` 修过的那类「从 chunk 重建覆盖落库内容」陷阱（§2.5）。

→ 但必须有 BDD 守住：若将来有人把搜索改成「写进父行 metadata」，`8cf8c12` 那类重建逻辑会再次吞掉它。见 §4.2 流式场景。

### 3.7 独立 spend 上下文（litellm 实修过的 bug）

litellm 的 follow-up 调用注释逐字（架构调研 §2.8 末）：

> `litellm_logging_obj` MUST be excluded so the follow-up call creates its own `Logging` instance via `function_setup`. Reusing the initial call's logging object triggers the dedup flag (`has_logged_async_success`) which **silently prevents the initial call's spend from being recorded** — the root cause of the SpendLog / AWS billing mismatch.

aigw 侧的对应风险：搜索与模型两次计费如果共用同一组「已记账」标记 / 同一个 `spend_amount` 累加变量 / 同一个 `SpendLog` 可变实例，就会出现「后者覆盖前者」或「前者被当重复丢弃」。

**约束（写进实现与 UT）**：
- 搜索行与模型行各自构造**独立的 `SpendLog` 值**，不得由同一个 `let mut sl` 改字段复用
- 搜索的 `increment_*_spend(search_spend)` 与模型的 `increment_*_spend(model_spend)` 各自传**自己的金额**，不得合并成一次调用后再拆
- UT 断言：两次调用后 key 的累计 = `search_spend + model_spend`（而不是其中任一）

### 3.8 `usage.server_tool_use.web_search_requests` 回传（方案 4）

**三个 surface 形状不同**，需各自注入点：

| surface | 注入位置 | 形状 | struct 改动 |
|---------|---------|------|------------|
| `/v1/messages`（Anthropic 原生） | 响应 `usage` 对象 | `"usage": { ..., "server_tool_use": {"web_search_requests": N} }` —— **官方字段，协议保真**（架构调研 §4.4） | `ClaudeUsage`（`models.rs:1151-1161`）加 `#[serde(skip_serializing_if = "Option::is_none", default)] server_tool_use: Option<ServerToolUse>` |
| `/v1/chat/completions` | 响应 `usage` 对象 | OpenAI Chat 规范**无此字段** → 按 Anthropic/OpenRouter 的事实标准同形填 `usage.server_tool_use.web_search_requests`，与上游兼容生态一致 | `Usage`（`models.rs:665-676`）加同名可选字段 |
| `/v1/responses` | 响应 `usage` 对象 | 同上；**不合成 `web_search_call` output item** —— 设计 C 不产生服务端工具块，合成块无法被安全回放（架构调研 §5.7 实证：真 Anthropic 会逐字节校验 `encrypted_content`，自造块必 400） | 共用 `Usage` |

新增 struct：

```rust
/// Server-side tool usage counters (Anthropic `usage.server_tool_use`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolUse {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub web_search_requests: Option<i32>,
}
```

- **流式**：次数在最后一个 usage 承载帧补齐（chat 的 `usage` chunk / messages 的 `message_delta`）；若该帧不存在则不回传（不为此新造帧）
- 全部字段 `Option` + `skip_serializing_if` → **未启用搜索时响应体字节级不变**，既有 BDD 不受影响

### 3.9a 零金额不退化为 no-op（必须守住的约束）

把 `cost_per_query` 显式置 0 的部署下 `spend == 0.0`。实现时**必须不加任何「金额为 0 就跳过」的短路**：

| 动作 | `spend == 0.0` 时 | 为什么不能跳过 |
|------|------------------|---------------|
| `insert_spend_log(搜索行)` | ✅ **照样执行** | 可观测性不能因金额为 0 而消失（搜索次数、provider、命中实例、耗时、query 全在这行里）；Stage 137 的整个展现层依赖它存在 |
| `increment_{key,user,team,org}_spend(0.0)` | ✅ **照样执行四次** | ① 保持与非零路径**同一条代码路径** —— 部署方后续改填非零单价时，**零代码改动即生效**；② 若此处短路，`spend_logs` 与 `virtual_keys.spend` 的对账等式会在「0 元行」与「非 0 元行」之间出现两套语义 |
| `metadata.cost_per_query` 快照 | ✅ 照样写 `0.0` | 历史对账要能区分「当时单价是 0」与「当时没记单价」 |

> **反模式**：`if spend > 0.0 { insert(); increment(); }` —— 这种写法对「显式填 0」的部署看起来「省了无用写入」，实际是把该部署的整个搜索可观测性、以及它日后改填非零单价的路径一起删掉。UT `test_zero_spend_search_row_still_inserts_and_increments` 专为锁死此点。

### 3.9 被否决的方案（登记理由）

| 方案 | 否决理由 |
|------|---------|
| **方案 2：附加费并入同一行**（new-api 路线：`spend = token费 + search_count × 单价`，次数记 metadata） | ① 搜索费被 token 费淹没，**无法独立聚合**（除非扫 metadata JSON）；② `spend` 不再等于 `tokens × 单价`，对账困惑；③ 多轮搜索明细丢失；④ 流式路径要在 Phase 2 UPDATE 里把搜索费并进去，**正面撞上 §2.5 的重建陷阱**；⑤ 架构调研 §5.6 明示：阶段 2（设计 A 短路）根本没有宿主行，方案 2 届时不可用 —— 现在选它等于给阶段 2 埋返工 |
| **方案 3：新增 `search_count` 列 + migration** | 本期无收益：设计 C 下搜索次数恒为 1，`metadata.search_query_count` 已足够；代价是**三方言 migration**（`027_*.sql` × 3）+ `SpendLog` struct 加字段 + `db.rs` 里三份手写 INSERT（`db.rs:2317` / `:2752` / `:3201`）各改 SQL + 三份 SELECT 列清单（`db.rs:2273` / `:2286` / `:2301` 等）同步。**→ 登记 §8.2：当「上游原生搜索次数」（`search_context_cost_per_query` 路径）也需要落库时，方案 3 立刻变得有吸引力 —— 届时一个可索引列同时覆盖「网关自执行」与「上游回报」两种来源，配 `metadata.search_count_source` 区分，与 `image_tokens` 的既有模式（`models.rs:188-191` 注释：源标记存 `metadata.image_tokens_source`）完全同构** |

---

## 4. TDD 计划

### 4.1 UT（`crates/aigw-server/src/routes/chat.rs` 测试模块，与 `calc_spend` 的 UT 同处）

**计价数学与精度**

> 非零单价的典型路径在生产已真实可达（部署方填自建摊销单价）；**UT 在此之上负责边界与精度** —— 极小单价、千次累乘、脏值、8 位小数无损性，这些不该靠生产流量去碰。表中的 `0.008` / `0.00507` 等取值是 §3.2 预留的后续厂商价目，用作**有代表性的非零被测值**，不代表本期会有这些 provider。

| 测试名 | 断言 |
|--------|------|
| `test_calc_search_spend_single_query` | `(1, Some(0.008))` → `0.008` |
| `test_calc_search_spend_multiple_queries` | `(3, Some(0.008))` → `0.024`（浮点比较用 epsilon） |
| `test_calc_search_spend_free_provider_is_zero` | `(5, Some(0.0))` → `0.0`（单价被显式置 0） |
| `test_calc_search_spend_amortized_micro_unit_price` | **自建摊销单价的典型形态**：`(1, Some(0.000012))` → `0.000012`；`(1000, Some(0.000012))` → `0.012`（极小单价不被 f64 吞掉，`DECIMAL(20,8)` 可存） |
| `test_calc_search_spend_missing_price_is_zero` | `(3, None)` → `0.0` |
| `test_calc_search_spend_zero_and_negative_queries` | `(0, …)` / `(-1, …)` → `0.0` |
| `test_calc_search_spend_rejects_nonfinite_price` | `NaN` / `inf` / 负单价 → `0.0`（防脏配置污染 `virtual_keys.spend`） |
| `test_calc_search_spend_decimal_8_precision` | `0.00507 × 1000` 在 `DECIMAL(20,8)` 的 8 位小数精度内可无损表示；断言与 `5.07` 的误差 < `1e-8`（对齐 sub2api `DECIMAL(20,8)` 的存储精度，且验证 `f64` 在该量级不丢位） |
| `test_calc_search_spend_openai_canonical_price` | `(1, Some(0.01))` → `0.01`（= $10/1k，sub2api 默认价与 OpenAI 规范价一致） |

**搜索行构造**

| 测试名 | 断言 |
|--------|------|
| `test_search_spend_log_row_shape` | `call_type=="search"`、`model=="searxng/search"`、`custom_llm_provider=="searxng"`、`prompt/completion/total_tokens` 全 0、`model_group.is_none()`、`mcp_namespaced_tool_name.is_none()` |
| `test_search_spend_log_api_base_is_hit_instance` | **多实例溯源**：provider 配两个实例，断言 `api_base` == **本次真正命中那个实例**的 `base_url`（而非 provider 名、也不是配置里的第一个实例）；切换命中实例后该列随之变化，而 `custom_llm_provider` / `model` **保持不变**（聚合维度跨实例稳定，§3.4） |
| `test_search_spend_log_inherits_ownership` | `api_key` / `team_id` / `organization_id` / `end_user` / `session_id` 与父行逐一相等 |
| `test_search_spend_log_metadata_parent_linkage` | `metadata["parent_call_id"]` == 父行 `call_id`；`metadata["search_query_count"]` == N；`metadata["cost_per_query"]` == 单价快照 |
| `test_search_spend_log_call_id_differs_from_parent` | 搜索行 `call_id != 父行 call_id`（守住 Phase 2 UPDATE 不会误伤） |
| `test_search_spend_log_status_never_streaming` | 流式父请求下搜索行 `status=="success"`，**不为 `"streaming"`** |
| `test_search_spend_log_does_not_borrow_request_id_for_parent` | `request_id` 字段不被挪用为父 id（语义是上游 provider id） |

**预算累加与独立上下文**

| 测试名 | 断言 |
|--------|------|
| `test_search_spend_accumulates_into_key_budget` | 搜索 + 模型两次 `increment_key_spend` 后，key 累计 == `search_spend + model_spend` |
| **`test_zero_spend_search_row_still_inserts_and_increments`** | **§3.9a 的锁定测试，本 Stage 最关键的一条**：以 `cost_per_query = Some(0.0)`（或 `None`）走完整条路径，断言 ① 搜索行**确实被 INSERT**（不被「金额为 0」短路掉）；② `increment_{key,user,team,org}_spend` **四个都被调用了一次，实参为 `0.0`**；③ 调用后 key 累计值 == 调用前 + `0.0`（即不变，但**调用确实发生**，用 mock/spy 计数而非只看余额）；④ `metadata.cost_per_query == 0.0` 已写入（区别于「没记单价」）。**反例对偶**：把实现换成 `if spend > 0.0 { … }` 时本测试必须变红 —— 它守住的是「部署方后续改填非零单价时零代码改动即生效」 |
| `test_search_spend_independent_logging_context_regression` | **litellm bug 回归**：以两个独立 `SpendLog` 值 + 两次独立 `increment_*` 调用走完流程，断言**两笔花费都在**；再以「复用同一可变实例 / 合并一次调用」的反例构造，断言其会丢失其中一笔 —— 锁死「不可共用 spend 上下文」这条约束 |
| `test_search_spend_four_level_increment_called` | key/user/team/org 四级各被调一次（与模型侧 `chat.rs:2676-2684` 同形） |

**usage 回传**

| 测试名 | 断言 |
|--------|------|
| `test_server_tool_use_serializes_on_claude_usage` | `ClaudeUsage` 带 `server_tool_use` 时序列化出 `usage.server_tool_use.web_search_requests` |
| `test_server_tool_use_absent_keeps_usage_bytes_unchanged` | 不启用搜索时 `Usage` / `ClaudeUsage` 序列化结果与现状**逐字节一致** |
| `test_server_tool_use_roundtrip_deserialize` | 上游回传该字段时不再被静默丢弃（修 §2.2） |

### 4.2 BDD（`crates/aigw-server/tests/features/`，mock 搜索后端按基线地图 §E2 的 mock upstream 机制接入）

> **非零金额的端到端场景用 Stage 134 的测试专用 `StubProvider` 配非零 `cost_per_query`**（本期生产 provider 只有 SearXNG，其单价由部署方决定、默认 0）。这同时也是 provider 多态与 failover 的被测对象 —— 一个桩同时服务三个目的。

| 场景名 | 断言要点 |
|--------|---------|
| `Search call writes its own spend log row` | 一次带搜索的 chat 请求后，`spend_logs` 出现 `call_type="search"` 行；其 `spend` == 配置单价 × 1；`prompt_tokens`/`completion_tokens`/`total_tokens` 均为 0 |
| `Search spend log links to its parent LLM row` | 搜索行 `metadata.parent_call_id` == 同一请求的 `completion` 行 `call_id`；两行 `session_id` 相同；两行 `call_id` 不同 |
| `Search spend counts toward the key budget` | 请求前后 `virtual_keys.spend` 增量 == 搜索费 + 模型费（不是只有模型费）—— 守住 §2.4。用 `StubProvider` 的非零单价，否则该断言退化为恒真 |
| `Streaming model call still records the search row` | 流式请求（`stream: true`）完成后，搜索行存在、`status="success"`、`spend` 正确；且父行 Phase 2 UPDATE 后搜索行内容**未被改写**（守住 §2.5 / §3.6） |
| `Zero unit price still records a search row and increments spend` | `cost_per_query` 显式置 0 → 搜索行**存在**且 `spend == 0.0`，四级 spend 累加调用**照样发生**（**行要在、调用要在，钱为 0** —— §3.9a；可观测性不能因金额为 0 而消失） |
| `Search row records the instance that actually served it` | provider 配两个实例 → 搜索行 `api_base` == 命中实例的 `base_url`，`custom_llm_provider` 仍为 provider 名（§3.4） |
| `Response usage echoes web_search_requests` | 响应体 `usage.server_tool_use.web_search_requests == 1` |
| `Request without search leaves usage untouched` | 未触发搜索的请求，响应 `usage` 中**无** `server_tool_use` 键 |

### 4.3 集成验证

1. 真实 SearXNG 自建实例（**沿用缺省 `cost_per_query = 0.01`**）→ 一次带搜索的 chat 请求 → 查 `spend_logs`：两行（`completion` + `search`），搜索行 `spend == 0.01`、`api_base` == 命中实例地址、`parent_call_id` 正确，且 `virtual_keys.spend` 增量 == 模型费 + `0.01` —— **开箱即用形态的非零计费真实验证**
2. 真实 SearXNG（**改填自建摊销单价**，如 `0.000012`）→ 断言搜索行 `spend == 单价 × 1` 且 `virtual_keys.spend` 增量含该值 —— 验证**极小单价在生产全链路（f64 → DECIMAL(20,8) → 聚合）不丢精度**
3. 真实 SearXNG（**显式填 `0.0`**）→ 搜索行**仍存在**、`spend == 0`、`api_base` 正确，`virtual_keys.spend` 增量 == 模型费（搜索贡献 0，但**四次 increment 调用已发生**，由 §4.1 的 spy UT 保证）—— §3.9a 的端到端对偶
4. 真实 SearXNG 多实例（两个 `base_url`，其一故意不可达）→ 连打若干次：命中实例在 `api_base` 列可区分；失败实例进冷却后不再被选中（Stage 134 的 `cooldown_until` 机制）
5. 真实 PG + 流式请求 → 确认 `8cf8c12` 修过的重建逻辑运行后搜索行完好
6. `task bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` 三 driver 各跑一遍（三份手写 INSERT 未改但新取值要过三方言类型校验）

> **延后到首个外部付费 provider 接入时**：Tavily 控制台次数 vs 搜索行数的核对、Bocha 的 CNY 账单 vs USD 落库值偏差实测（后者是 §8.2 汇率决策的输入数据）。本期无付费 provider，这两项无从执行。

---

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs` | 新增 `calc_search_spend`（紧邻 `calc_spend:104` / `calc_spend_modal:139`）；搜索行构造 + `insert_spend_log` + 四级 `increment_*_spend`；非流式/流式两条路径的注入点 |
| `crates/aigw-server/src/routes/responses.rs` | 同形接入（注意与 `responses.rs:1105` 起的 chunk 重建逻辑**互不干涉**） |
| `crates/aigw-server/src/routes/v1_messages.rs` | 同形接入；`usage.server_tool_use` 为此 surface 的**原生官方字段** |
| `crates/aigw-core/src/models.rs` | 新增 `ServerToolUse` struct；`Usage`（`:665`）与 `ClaudeUsage`（`:1151`）各加 `server_tool_use: Option<ServerToolUse>`（`skip_serializing_if` + `default`） |
| `crates/aigw-core/src/config.rs` | 搜索 provider 配置块加 `cost_per_query: Option<f64>` + `#[serde(default = "default_cost_per_query")]`（返回 `Some(0.01)`），挂在 Stage 134 定义的 **provider**（而非 instance）配置下 —— 定价是逻辑后端属性，同一 provider 的多个实例共享单价；配置注释须写明「**该缺省值是 OpenAI 对外牌价占位，不是自建成本**，自建请改写为摊销单价（`月度基础设施成本 ÷ 月均查询数`）；填 `0.0` 则成本不可见」 |
| `crates/aigw-server/tests/features/*.feature` | 新增 8 条场景（§4.2） |
| `crates/aigw-server/tests/bdd_steps/*.rs` | 对应 steps：查 `spend_logs` 的 `call_type="search"` 行、校 `metadata.parent_call_id`、校 key spend 增量 |
| `docs/12-technical-debt.md` | 登记 §8.2 三条 TD |
| `docs/stages/stage-roadmap.md` + `docs/11-next-steps.md` | Phase 53 进度回写 |
| **不改** | `crates/aigw-core/src/db.rs`（三份手写 INSERT `:2317`/`:2752`/`:3201` 一行不动）；`crates/aigw-core/migrations/**`（零 migration） |

---

## 6. 回归验证

1. `task test` — aigw-core / aigw-server UT 全绿（新增约 23 个）
2. `task test-bdd` — Mock BDD 全绿（现基线 285 场景 / 1453 steps，新增 8）
3. `task bdd-real-sqlite` / `task bdd-real-pg` / `task bdd-real-mysql` 三驱动全绿
4. `task fmt` / `task lint` green
5. **字节级回归**：未启用搜索时，`/v1/chat/completions`、`/v1/responses`、`/v1/messages` 的响应 `usage` 与改动前逐字节一致（由 §4.1 的 `..._keeps_usage_bytes_unchanged` + 既有 BDD 共同守护）
6. **流式回归**：`8cf8c12` 引入的 `then_spendlog_stream_response_has_content`（`crates/aigw-server/tests/bdd_steps/responses_steps.rs:301` 起）保持绿
7. **对账等式**：`SUM(spend_logs.spend)` == `virtual_keys.spend` 增量（含搜索行）

---

## 7. 门禁

- [ ] 约 23 个新 UT 先 fail 后 pass（TDD 红绿），含 `test_search_spend_independent_logging_context_regression`、`test_zero_spend_search_row_still_inserts_and_increments`、`test_search_spend_log_api_base_is_hit_instance`
- [ ] 8 条新 BDD 场景全绿，含流式场景、零单价场景、多实例 `api_base` 场景
- [ ] `task test` / `task test-bdd` / `task fmt` / `task lint` 全绿
- [ ] `task bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` 三驱动全绿
- [ ] 真实 provider 端到端：SearXNG **缺省 `0.01` / 摊销单价 `0.000012` / 显式 `0.0` 三种配置各一次**（§4.3 第 1–3 项），`spend_logs` 两行、`parent_call_id` 正确、`api_base` 为命中实例；前两次 `virtual_keys.spend` 增量含搜索费
- [ ] 对账等式成立：`SUM(spend_logs.spend)` == `virtual_keys.spend` 增量（上述三种配置下都成立）
- [ ] 未启用搜索时响应 `usage` 字节级不变
- [ ] `docs/12-technical-debt.md` 登记 §8.2 各条（含 Stage 138 的 DB 化、Bocha 汇率延后项）
- [ ] `docs/stages/stage-roadmap.md` + `docs/11-next-steps.md` 回写
- [ ] git commit（精确 add；`--signoff`）

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| **零 token 行污染既有聚合口径** | `aggregate_spend_by_model`（`db.rs:3764`）的 SQL 是 `SUM(total_tokens) … COUNT(*) as requests … GROUP BY model`（`db.rs:2498`）：搜索行使 `requests` 计数上涨而 `total_tokens` 不涨 → 任何「平均 token/请求」类口径被稀释。`global_spend_activity` 的 `ActivityMetadata.total_requests`（`crates/aigw-server/src/routes/spend.rs:1124` 起）同理 | 搜索行的 `model` 为 `"<provider>/search"`，在按 model 分组时**自成一组**，不污染任何真实模型的组内口径；跨组的总计类指标（`total_requests`）确实会变 → **交 Stage 137 逐处审计并在前端口径上排除**（§Stage 137 过滤/聚合影响一节） |
| 配置方填了脏单价（负数/超大） | 直接污染 `virtual_keys.spend`（该列无 CHECK 约束） | `calc_search_spend` 对非有限值与非正值一律归零（§3.1）+ UT 锁定 |
| ⚠️ **SearXNG 沿用默认 `0.01` 导致自建成本被系统性高估** | 缺省 `0.01`（$10/1k）是 OpenAI **对外售价**，而自建摊销典型在 `1e-4` 量级 → **放大约 50 倍**。预算提前触顶、Usage 的 `searxng` 花费虚高、对账失真。**本期唯一的生产 provider 正是 SearXNG，故这是最可能发生的真实配置** | ① 配置注释与 Stage 138 的 UI 均明示「该默认值非自建成本，请改写」并给摊销算例；② 该取舍是**刻意的**——宁可高估（看得见、一行可改）也不要静默为 0（永远无法回答「搜索花了多少」）；③ Stage 138 考虑加输入软校验（自建 kind 填 > `0.001` 时提示确认）；④ §3.9a 保证显式填 `0.0` 时行与 increment 调用照样发生，改填任意值即刻生效，**无需改代码、无需回填历史** |
| Tavily 免费额度未建模导致账面高于实际 | 每月前 1000 次按 $0.008 计提但实际 $0 | 与 litellm 一致的取舍；差额由财务对账吸收。若需精确，须引入跨月计数器（远超本期范围）。**该风险在 Tavily 实际接入时才生效，本期不实现** |
| 行数膨胀 | 启用搜索后 `spend_logs` 行数约翻倍 | 现有 `body_archive`（migration 021，`body_archived` / `parquet_path`）即为此准备；搜索行 body 极小，影响可控 |
| 搜索失败是否计费 | provider 侧可能按失败请求计费（因 provider 而异） | 本期：`status="failure"` 的搜索行 `spend` 仍按牌价计提（保守，宁多算不漏账），metadata 保留错误信息供人工冲正 |

### 8.2 遗留（登记 TD）

| TD | 内容 |
|----|------|
| **首批待接入 provider（抽象已就位，接入是纯增量）** | Phase 53 只实现 SearXNG。**Tavily**（$0.008/query，~1s 延迟）与**博查 Bocha**（¥36/1k）是优先级最高的后续接入项 —— Stage 134 的 `SearchProvider` trait / `kind` 判别式 / N-provider + N-instance 配置 / failover / per-provider `timeout_ms` / `api_key` + `v2:gcm:` / `cost_per_query` **已全部就位**，各自只需补一个 `parse_response` 与一条价目配置。**本 Stage 的计费机台无需为它们改任何代码** —— §3.9a 的「零金额不退化」约束正是为此而设。接入时需补做的是 §4.3 延后的两项真实对账（Tavily 控制台次数核对、Bocha CNY/USD 偏差实测） |
| **搜索 provider/实例与定价的 DB 化与 UI 管理 → Stage 138** | 本期 provider、instances[]、`cost_per_query` 全部来自 `config.yaml`（改价需改配置 + 重启）。Stage 138 将仿 `proxies` 表 / `027_proxies.sql` / Phase 50 全套，把它们迁到 DB 表 + admin CRUD + 前端管理页。对本 Stage 的影响：`cost_per_query` 的**读取位置**会从 config 换成 DB，但 `calc_search_spend` 的签名、`metadata.cost_per_query` 单价快照语义、§3.9a 的约束**均不变**（单价快照的价值在 DB 化后反而更高 —— 运行时改价后历史行仍可复算） |
| ⏸ **Bocha 以 CNY 计价而 aigw 的 spend 恒为 USD —— 汇率决策（已降级为延后项）** | **本期不再是实现阻塞项**：该问题只因 Bocha 按人民币计价才成立，而 **Bocha 本期不实现**，故没有任何 CNY 单价需要配置方填写。问题本身依旧真实 —— 博查牌价是 **¥36/1k**，而 `SpendLog.spend: f64`（`models.rs:150`）与 `virtual_keys.spend` 等全部金额列**无货币字段、语义恒为 USD**（`Deployment` 的定价字段注释逐字 `USD / input token`，`crates/aigw-core/src/deployment.rs:27-61`）。**决策时点：Bocha 实际接入之前（而非本 Stage 之后）**，三个候选不变：① 保持 USD 单一货币 + 文档明示「填 USD，汇率自担」（零成本，但把汇率风险静默转移给配置方且历史账目随汇率漂移失真）；② 配置支持 `cost_per_query_cny` + 全局固定汇率（可审计，汇率需人工维护）；③ 引入 `currency` 字段 + 落库时换算并把汇率快照进 `metadata`（最正确，代价最高；与 Stage 138 的 DB 化一并做最划算）。**规则不变且必须保留：在决策落地前，Bocha 的落库金额不得用于对外出账。** |
| **方案 3（`search_count` 专列）为未来路径** | 当「上游模型原生执行搜索」的计费（`search_context_cost_per_query`，架构调研 §2.8 路径 2）需要落库时，加 `search_count: Option<i32>` 列 + `metadata.search_count_source`（`"gateway"` / `"upstream"`）即可**用一个可索引列统一两种来源**，与 `image_tokens` 的既有模式（`models.rs:188-191`）同构。代价：三方言 migration（`027_*.sql` × 3）+ `db.rs:2317`/`:2752`/`:3201` 三份手写 INSERT 与配套 SELECT 列清单同步改。本期不做（设计 C 下次数恒为 1，metadata 足够） |
| 按次计价机制可泛化 | `calc_search_spend` 是 aigw 第一个非 token 计价函数。仓内其他本该按次/按张/按秒计价的路径目前**一律没有固定费**：embeddings（`crates/aigw-server/src/routes/embeddings.rs:531-540` 纯 token）、图像生成（无按张计价，`input_cost_per_image` 全仓零命中）、`calc_spend_modal` 仍是 dead code（`chat.rs:138`，TD-012b）。若未来接入这些，`calc_search_spend` 的形态（`count × unit`，`Option` 单价，非有限值归零）可直接推广为通用 `calc_per_call_spend` |
| key/team 级单价覆写 | §3.3 决策为本期只做全局配置。若引入 group/markup 定价层，按 sub2api `web_search_price_per_call DECIMAL(20,8)`（`~/works/play/sub2api/backend/migrations/174_group_web_search_price_per_call.sql`）的列形态补 `virtual_keys` / `teams` 级覆写 + 优先级链 |
| 阶梯定价 | litellm `tiered_pricing`（按 `max_results` 区间取价，如 `exa_ai/search` 0.005/≤25 结果、0.025/26-100）在候选 provider 均不适用，故未实现。若接入 Exa / Firecrawl 需补 |
| 设计 A（短路）/ 设计 B（agentic loop）的计费形态 | 见 stage-135.md §8.2 的两个未来形态表。**设计 A 没有宿主 LLM 行**（短路不调模型）→ 独立行方案天然适配、方案 2 届时不可用（§3.9 已据此否决方案 2）；**设计 B 每轮搜索一行 + 每轮模型一行**，需照抄 litellm 的「每轮独立 spend 上下文」（§3.7 的约束届时按轮复用即可，无需新机制） |
