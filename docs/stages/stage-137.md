# Stage 137: 搜索调用在管理控制台的展现与聚合口径修正（Phase 53）

**所属**: Phase 53（内建 Web Search / TD-017c）
**预估**: 9h（列表/抽屉渲染 + `call_type` 过滤 + 聚合口径审计与修正 + i18n + fe-bdd）—— 由 8h 上调 1h，新增的是**「spend=0 vs spend>0 双态渲染」与「命中实例 `api_base` 展示」两项**（§3.2a / §3.5），各需一组 fixture 与场景
**依赖**: Stage 136（按次计费 + SpendLog 独立行）
**状态**: ⏳ 规划

> **Phase 53 范围收窄（2026-10-06 决策）对本 Stage 的影响**
>
> 本期只实现 **SearXNG** 一家 provider（多 provider 架构保留，Tavily / Bocha 为后续纯增量接入）。对前端的三个具体后果：
>
> | 后果 | 处置 |
> |------|------|
> | provider badge **本期恒为 `searxng`** | 渲染逻辑仍按「provider 名动态取值」写（不硬编码），fixture 以 `searxng` 为主、另留一个非 `searxng` 值的 fixture 防硬编码 |
> | `spend` **默认非 0**，也可能为 0 | **`cost_per_query` 缺省 `0.01` USD/次**（= $10/1k 牌价占位，Stage 136 §3.2）→ **开箱即用的默认形态是 `spend>0`**；部署方改为自建摊销值（典型 `1e-4`）仍 `spend>0`；**仅显式填 `0.0` 时才 `spend=0`**。**三种都是真实生产形态，UI 必须双态优雅** → §3.2a（本 Stage 的核心渲染决策）。注意：默认值是牌价占位而非自建真实成本，**UI 不应暗示该金额已校准** |
> | 搜索行 **token 列恒为 0**（与单价无关） | §3.2 的 `—` + tooltip 决策**不变** |
> | provider 有**多个物理实例** | Stage 136 的 `api_base` 列记「实际命中实例」→ 抽屉须可见（§3.5），列表徽章仍只显 provider 名 |
>
> **本期前端只做「搜索调用日志的只读展现」，不含任何 provider / 实例 / 定价的管理 UI** —— 那是 **Stage 138**（配置源从 `config.yaml` 迁到 DB 表 + admin CRUD + 管理页，仿 `proxies` / `027_proxies.sql` / Phase 50）。读者勿将本 Stage 误解为配置页。

---

## 1. 目标

Stage 136 落地后，每个启用了搜索的客户端请求会在 `spend_logs` 里产生**两行**：一条 `call_type="search"` 的搜索行 + 一条正常的 LLM 行。这两行**立即、自动**出现在 `/global/spend/logs` 列表里（无需任何后端改动），但现有前端对它们一无所知：

- 搜索行的 token 列全为 0 → 表格渲染成 `0 / 0`，和「真的没花 token」与「数据坏了」无法区分（基线地图 §B5「需要改什么」第 2 条）；**且 `spend` 列在部署方把 `cost_per_query` 显式置 0 时也是 `0`**（Stage 136 §3.1 缺省为 `0.01`，但置 0 是合法配置）→ 一行里「0 token + $0」叠加出现，极易被读成「这条记录是坏的」。**让 0 读起来像真值而不是缺失，是本 Stage 的核心渲染任务**（§3.2a）；
- `call_type` 既**不是筛选项**（筛选器只有 model / status / token 区间，`index.tsx:1290-1348`），搜索行无法被排除也无法被单独查看；
- `metadata.parent_call_id` 这条父子线索**完全没有被渲染**，看日志的人不知道某条搜索属于哪次对话；
- 更严重的是 Usage 页的多处聚合口径会被零 token 行稀释（§2.5 / §4.3），这是本 Stage 唯一有「数字算错」后果的部分。

本 Stage 只做展现与口径，不碰 provider 客户端（Stage 134）、注入接线（Stage 135）、计费数学（Stage 136）。

### 验收标准

- [ ] 列表与移动端卡片对 `call_type="search"` 行渲染**专属 badge**（与 `completion` / `responses` / `embedding` 视觉可区分）；provider 名**动态取自数据**（本期恒为 `searxng`，但不得硬编码）
- [ ] 搜索行的 token 单元格**刻意渲染为 `—` 并带 tooltip**说明「搜索按次计费、不消耗 token」，而不是 `0 / 0`（§3.2）
- [ ] **`spend` 的 0 与非 0 双态均优雅**（§3.2a）：`spend == 0`（部署方把 `cost_per_query` 显式置 0）渲染为 **`$0.00` + 可解释提示**而非 `—`/空白 —— 它是真实零值，不是缺失数据；`spend > 0`（含缺省 `0.01` 与自建摊销值）走与 LLM 行完全一致的金额展示。**同一行里「token `—` + spend `$0.00`」的组合必须读起来像一条完整记录**
- [ ] 搜索行在列表与抽屉里展示：query 文本、结果条数、provider 名、spend（均来自 `metadata` / `messages` / `response`，无需新后端字段）
- [ ] 抽屉展示**本次实际命中的实例**（`api_base`），使多实例部署可按实例溯源（§3.5）
- [ ] 抽屉里对 LLM 行展示「本次请求的搜索调用」入口、对搜索行展示「返回父调用」入口，两向可跳（§3.3）
- [ ] 筛选器新增 `call_type` 下拉（`all` / `completion` / `responses` / `embedding` / `search`），后端 `/global/spend/logs` 支持 `call_type=` 参数（§3.4）
- [ ] Usage 页所有「平均 token/请求」「请求数」「Top N 排名」类口径对搜索行的影响被逐处审计并修正或显式标注（§4.3 表格逐项打勾），**含 spend 类图表在 `spend=0` 与 `spend>0` 两种配置下的表现**
- [ ] 抽屉里对搜索行**隐藏** cache token / TTFT / image token 区块（对搜索行恒无意义）
- [ ] en + zh-CN 两份 i18n 同步新增键，无 `missing key` 告警
- [ ] `task fe-lint` / `task fe-bdd`（3 viewport）/ `task test` / `task lint` 全绿

### 明确不做（边界）

- **父子关系的 DB 层 FK / JOIN**（基线地图 §B4 路线 B）—— 本期沿用 Stage 136 的 `metadata.parent_call_id` 约定，前端按值跳转，不加 migration
- **树形/折叠表格组件** —— 现有 `components/ui/table.tsx` 无嵌套行能力，引入会牵动全部 13 列的布局；本期用「跳转 + 徽标」而非折叠（§3.3 给出理由）
- **CSV 导出补搜索专属列** —— `buildCSVHeaders`（`index.tsx:269-283`）保持 13 列不变，搜索行按现有列导出（token 列为 0）
- **搜索 provider / 实例 / 定价的管理 UI** —— 本期前端对搜索**只做只读日志展现**。配置源的 DB 化（表 + admin CRUD + 管理页，仿 `proxies` / `027_proxies.sql` / Phase 50）是 **Stage 138**；本 Stage 不加任何配置表单、不改 `config.yaml` 读取路径
- **按实例下钻的新图表** —— 命中实例（`api_base`）只在抽屉可见（§3.5）即满足溯源需求；「按实例聚合花费」需要新后端聚合函数，不在本期
- **Bocha CNY/USD 汇率在 UI 上的换算或双币种展示** —— ⏸ **本期该问题不发生**：Bocha 本期不实现（Phase 53 只有 SearXNG，USD 原生），故 UI 上不会出现任何 CNY 来源的金额。Stage 136 §8.2 的汇率决策已降级为「Bocha 实际接入前必须落地」的延后项；本期仅**保留 i18n 键与提示组件**（§4.4 / §8.1），使 Bocha 接入时前端零改动即可显示「不可用于出账」告示
- **`daily_*_spend` 日聚合表的搜索行口径** —— 该链路（`crates/aigw-core/src/daily_spend_queue.rs:96-161` 的 8 元组聚合键含 `model` / `custom_llm_provider`）由 Stage 136 决定是否入队；前端当前**无任何页面消费 `daily_*_spend`**，本期不审计
- **按 `session_id` 筛选的 UI** —— `SpendLogsQuery.session_id`（`crates/aigw-server/src/routes/spend.rs:45`）后端早已存在但 handler 根本没传给 DB（`spend.rs:662-674` 的参数列表里无它），属既有技术债，与本 Stage 无关
- **Playground / Dashboard 的搜索专属展现** —— Dashboard 仅做总额与 Top 图，审计见 §4.3，不新增 UI

---

## 2. 现状证据

### 2.1 Spend Logs 页的真实形状

| 关注点 | 锚点 |
|--------|------|
| 页面组件 | `crates/aigw-frontend/src/pages/spend-logs/index.tsx`，`SpendLogsPage` 导出于 `index.tsx:1044` |
| 列表 TS 类型 | `interface SpendLog`，`index.tsx:65-96`（含 `call_type: string`、`metadata?: unknown`、`image_tokens`） |
| 详情 TS 类型 | `interface SpendLogDetail`，`index.tsx:99-134`（= 列表全字段 + `messages` / `response` / `proxy_server_request`） |
| 表头 | `index.tsx:1413-1454`，**13 列** |
| 行渲染 | `index.tsx:1456-1565`（desktop，`data-testid="spend-log-row"`，整行 `onClick` 打开抽屉） |
| 移动端卡片 | `index.tsx:1567-1648`（`md:hidden`，同 `data-testid`） |
| 详情抽屉 | `DetailDrawer`，`index.tsx:631`（基于 `components/ui/sheet.tsx`，DOM 上是 `[role='dialog']`） |
| 列表数据源 | `/global/spend/logs`，`index.tsx:1144` 起拼 query string |
| 详情数据源 | `/global/spend/logs/{call_id}`，`index.tsx:1176` |
| Live tail | `LIVE_TAIL_INTERVAL = 15_000`，`index.tsx:1042` |

**13 列清单**（表头 i18n key → 渲染内容）：

| # | i18n key | 渲染 | 搜索行会怎样 |
|---|----------|------|-------------|
| 1 | `spendLogs.table.callId` | `truncateUuid(call_id)` + 复制 | 正常（搜索行有独立 UUID v7） |
| 2 | `spendLogs.table.requestId` | `request_id` 或 `—` | 多数 provider 无 id → `—`，已容错 |
| 3 | `spendLogs.table.time` | `format(start_time, "MM-dd HH:mm:ss")` | 正常（搜索真实起止，Stage 136 §3.4） |
| 4 | `spendLogs.table.type` | `<Badge variant="outline">{log.call_type \|\| "—"}</Badge>`，`index.tsx:1487-1494` | 显示裸字符串 `search`，**无视觉区分** |
| 5 | `spendLogs.table.model` | `model_group` badge + `model` + 🖼️ 标记 | `model_group` 为 `None`（Stage 136 刻意不填）→ 只剩 `searxng/search` 文本 |
| 6 | `spendLogs.table.key` | `key_name \|\| user \|\| "—"` | 正常（同父行 key） |
| 7 | `spendLogs.table.endUser` | `truncateEndUser(end_user)` | 正常（同父行） |
| 8 | `spendLogs.table.ip` | `requester_ip_address ?? "—"` | 正常（同父行） |
| 9 | `spendLogs.table.status` | `<StatusBadge>`，`index.tsx:975` | 正常（`success` / `failure`，搜索永不 `streaming`） |
| 10 | `spendLogs.table.ttft` | `fmtTtft(ttft_ms)` | `ttft_ms` 由后端 `compute_ttft` 算（需 `completion_start_time`，搜索行恒 `None`）→ 已有 `—` 容错 |
| 11 | `spendLogs.table.duration` | `fmtDuration(request_duration_ms)` | 正常且**有价值**（搜索耗时直接拖长 TTFT） |
| 12 | `spendLogs.table.tokens` | `fmtTokens(prompt_tokens) + " / " + fmtTokens(completion_tokens)` + metadata cache 行，`index.tsx:1542-1560` | ⚠️ **渲染成 `0 / 0`** —— `fmtTokens(0)` 返回 `"0"`（`crates/aigw-frontend/src/lib/format.ts:7-13`，`abs < 1000` 走 `v.toString()`），与「坏数据」不可辨 |
| 13 | `spendLogs.table.cost` | `fmtSpend(spend)` | 正常 |

### 2.2 抽屉当前展示什么

`DetailDrawer`（`index.tsx:631`）的结构：

| 区块 | 锚点 | 对搜索行的意义 |
|------|------|---------------|
| 标题 + Call ID / Request ID badge | `index.tsx:666-701` | 保留 |
| Summary pills（status / `call_type` badge / model / spend / `prompt↑/completion↓ · TTFT/duration`） | `index.tsx:703-717` | ⚠️ `index.tsx:714-716` 硬渲染 `0↑ / 0↓ · — / 1.2s`，同 §2.1 第 12 列问题 |
| image tokens + tooltip（已有「刻意渲染 + tooltip」的先例！） | `index.tsx:718-745` | 搜索行 `image_tokens` 为 `None` → 自动不渲染 |
| meta 行（start / end / group / provider / id / base / user / endUser / session / cache / team / org / cacheHit / cacheKey / mcpTool） | `index.tsx:754-877` | 多数条件渲染（`log.x ? ... : null`），`provider` / `session` / `base` 对搜索行有值且有用 |
| prompt / response 双 tab（visual + raw，`parseMessages`） | `index.tsx:900-965` | Stage 136 已把 `messages = {"query": ...}` / `response = {"result_count": M, "results": [...]}` 写进去（§3.4 表格）→ **raw tab 直接可读**，visual tab 的 `parseMessages` 对非 messages 形状会落到 `noPromptData` 空态 |
| `metadata` 本体 | **未渲染** —— `extractCacheTokens(log.metadata)`（`index.tsx:224`）只挑 `cache_read_tokens` / `cache_creation_tokens` 两键；`imageTokensSource`（`index.tsx:254`）只挑 `image_tokens_source` | ⚠️ `search_query_count` / `parent_call_id` / `search_provider` / `cost_per_query` **全部不可见** |

> i18n 已有 `spendLogs.drawer.metadata` 键（en.json 的 `spendLogs.drawer` 键集里存在）但**源码中零引用** —— 本期可直接复用，不必新造。

### 2.3 筛选器现状 —— 无 `call_type`

| 筛选项 | 前端 state | 拼参 | 后端 query 字段 | DB 条件 |
|--------|-----------|------|----------------|---------|
| 时间预设 15m/4h/24h/7d/custom | `index.tsx:1047-1049`，`PRESET_KEYS` `index.tsx:454` | `start_date` / `end_date` | `spend.rs:42-43` | `db.rs:8147-8160` |
| call_id / request_id 模糊 | `requestIdFilter` `index.tsx:1058` | `request_id=` `index.tsx:1147` | `spend.rs:44` | 双列 `LIKE … ESCAPE`，`db.rs:8162-8175` |
| model 下拉 | `modelFilter` `index.tsx:1050` | `model=` `index.tsx:1145` | `spend.rs:40` | `db.rs:8141` |
| status（all/success/failure/streaming） | `statusFilter` `index.tsx:1060`，选项 `index.tsx:1337-1347` | `status=` `index.tsx:1149` | `spend.rs:46` | 三分支，`db.rs:8177-8185` |
| min/max tokens | `index.tsx:1352` / `:1363` | — | `spend.rs:47-48` | `db.rs:8186-8191` |

**→ `call_type` 不在这五项中的任何一项。** 后端 `SpendLogsQuery`（`spend.rs:38-52`）也**没有** `call_type` 字段；DB 层 `query_spend_logs_with_status_filter`（`crates/aigw-core/src/db.rs:8116-8128`，11 个参数）同样没有。虽然该函数的 `SELECT` 列清单里有 `call_type`（`db.rs:8203`），但从未进 `WHERE`。

### 2.4 `metadata` 过滤能力 —— 后端完全没有

`query_spend_logs_with_status_filter` 的 `WHERE` 条件是**字符串拼接**（`db.rs:8132-8191`，每条 `conditions.push(format!(...))` 且手工 `replace('\'', "''")` 转义），**没有任何 JSON 路径条件**。仓内唯一的 JSON 提取发生在聚合侧：`query_activity_metadata` 的 `json_extract(metadata, '$.cache_read_tokens')`（`db.rs:3966-3967`，三方言各一份）。

**→ 结论**：「按 `metadata.parent_call_id` 筛选」在后端**当前不可能**。本 Stage 必须补（§3.4 给出最小改法）—— 这是本 Stage 唯一的后端改动。

### 2.5 聚合口径：零 token 行的污染面

Stage 136 §8.1 已点出方向，这里把**具体 SQL 与前端消费点**钉死：

> **范围收窄不降低本节的重要性**：下表的失真**几乎全部由「零 token 行 + 多出来的行数」驱动**，与 spend 是否为 0 **无关** —— `total_requests` 翻倍、成功率分母变大、Top Keys 按 requests 换序、分页页数翻倍，这些在 `spend=0` 的 SearXNG 部署下**一模一样地发生**。因此 §4.3 的 15 项逐项审计**一项不减**。
>
> 变化的只是**与金额相关的那几项**：`spend=0` 时搜索组/扇区在 spend 类图表上退化为零值条目（见上表后两行的 ⚠️ 与 §4.3 #7 / #9），需要显式处置 —— 这是本次收窄**新增**的审计负担，不是减负。

| 聚合 | DB SQL 锚点（三方言） | 输出字段 | HTTP handler | 前端消费 | 零 token 行影响 |
|------|---------------------|---------|-------------|---------|----------------|
| `aggregate_spend_by_model` | SQLite `db.rs:2498-2499` / MySQL `:2953-2954` / PG `:3377-3378`；分发器 `:3764` | `model, SUM(total_tokens), SUM(spend), COUNT(*) as requests` | `/global/spend/models` `spend.rs:959`、`/spend/models` `spend.rs:800` | Usage「Spend by Model」`usage/index.tsx:292-299`；Dashboard `dashboard/index.tsx:148-155` | **组内不污染**（`model = "searxng/search"` 自成一组），但 `ORDER BY total_tokens DESC` 使搜索组恒排最末（token=0），**搜索花费在按 token 排序的图里看不见**。⚠️ **若部署方把 `cost_per_query` 显式置 0**，该组 `SUM(spend)` 也是 0 → 组存在但两个数值都为 0，在任何图上都是**零高度条/零面积扇区**（缺省 `0.01` 下不会发生；§4.3 #7 给处置） |
| `aggregate_spend_by_model_group` | `db.rs:2530-2531` / `:2985-2986` / `:3407-3408`；分发器 `:3787` | `COALESCE(model_group,'unknown'), …` | `/global/spend/model-groups` `spend.rs:1048` | Usage「Spend by Model Group」`usage/index.tsx:316` | ⚠️ 搜索行 `model_group IS NULL` → 全部并入 **`'unknown'` 组**，与「真的没打 group 的 LLM 调用」混在一起，`'unknown'` 组的 token/请求比被稀释 |
| `aggregate_spend_by_provider` | `db.rs:2560-2568` / MySQL `:3001` / PG `:3418`；分发器 `:3810` | `COALESCE(NULLIF(custom_llm_provider,''),'unknown'), SUM(total_tokens), SUM(spend), COUNT(call_id)` | `/spend/providers` `spend.rs:839`、`/global/…` `:854` | Usage「Spend by Provider」donut `usage/index.tsx:303-310`；Dashboard `:159-166` | **组内不污染**（`searxng` 自成组；后续 `tavily`/`bocha` 同理），且这是**唯一天然正确**的图 —— 搜索厂商花费独立可见（缺省 `0.01` 下扇区即有面积）。⚠️ **若显式置 `cost_per_query = 0`**，`searxng` 扇区的 `SUM(spend)` 为 0 → donut 上**零面积扇区**，且 `mergeSmallProviders`（`usage/index.tsx:201`）的 <1% 阈值会把它并进 `others` → **此时「搜索厂商花费独立可见」失效**（§4.3 #9 给处置） |
| `query_activity_metadata` | `db.rs:3941-4056`（三方言），`SELECT SUM(spend), SUM(total_tokens), COUNT(call_id), COUNT(CASE status='success'), COUNT(CASE status LIKE 'failure%'), SUM(prompt_tokens), SUM(completion_tokens), json_extract cache…` | → `ActivityMetadata`（`spend.rs:1125-1145`） | `/global/spend/activity` `spend.rs:1107` | Usage 顶部 6 个 stat tile（`usage.cards.*`），`usage/index.tsx:492`（requests）/`:549`（tokens）/`:581-582`（成功率） | ⚠️⚠️ **跨组总计，必被污染**：`total_requests` 翻倍（每请求多一行）、`total_tokens` 不变 → 任何「token/请求」心算失真；成功率 `successful_requests / total_requests`（`usage/index.tsx:581-582`）分母变大、分子也变大，**比值被拉向 100%**（搜索成功率通常高于 LLM） |
| `query_activity_daily` / `query_activity_hourly` | `db.rs:4058-4161` / `:4163-4262`，`COUNT(call_id)` 作为 `requests` | 同上 handler | Usage 趋势图的 requests 系列（tooltip `usage/index.tsx:671` 明确展示 `successful_requests` / `failed_requests` / `requests`） | 同 `activity`：requests 曲线整体抬升 |
| `aggregate_spend_by_keys` | `db.rs:4265-4310+`（三方言），`SUM(spend), COUNT(call_id) as total_requests, SUM(total_tokens)` GROUP BY `api_key` | `SpendKeyRanking` | `/global/spend/keys/rankings` `spend.rs:1345` | Usage「Top Virtual Keys」`usage/index.tsx:324-331`，三种排序（spend / tokens / requests）`:777-793` | ⚠️ **排名可能换序**：按 `requests` 排序时，启用搜索的 key 请求数翻倍 → 相对未启用搜索的 key 被人为抬高；按 `total_spend` 排序正确（搜索费本就该算它的）；按 `total_tokens` 排序不受影响 |
| `get_global_spend` | `db.rs:2473-2478`，`SELECT SUM(spend) FROM spend_logs` | `f64` | `/global/spend` `spend.rs:632` | Dashboard「Total Spend」`dashboard/index.tsx:128-132` | **正确**（搜索费本就该入总额） |
| `query_spend_logs_count` | `db.rs:2669-2690` / `:3118` / `:3532`；分发器 `:3867`，`SELECT COUNT(*)` | `total_count` → 分页 | `spend.rs:674-680` | 分页条 `index.tsx:1393` 起 | ⚠️ 行数翻倍 → 页数翻倍（§8 登记）；且该 count 的参数列表（5 个：`api_key/model/start/end/call_id`）**比列表查询少 status/token 区间**，本来就与 `data.length` 口径不一致（既有技术债，不扩大） |
| Dashboard `periodSpend` | — | 前端对 `/global/spend/logs?limit=100` 的 `data` 做 `reduce(sum + l.spend)`，`dashboard/index.tsx:168-172` | | Dashboard「Period Spend」 | ⚠️ **双重失真**：① 搜索行挤占 100 条窗口，窗口覆盖的时间变短；② `logsData?.count ?? 0`（`dashboard/index.tsx:258`）被当作「Total Requests」展示，而它是**本页返回条数**（≤100），搜索行使它更快撞到 100 的天花板 |

### 2.6 fe-bdd 现有约定

| 关注点 | 锚点 |
|--------|------|
| task | `Taskfile.yml:119-133`（`fe-bdd`：强制 `npx bddgen` 再 `npx playwright test`；注释记录了 Stage 85 的 stale-spec 事故） |
| feature 目录 | `crates/aigw-frontend/tests/features/`（15 个 `.feature`），本页为 `spend-logs.feature` |
| steps | `crates/aigw-frontend/tests/steps/spend-logs.steps.ts` |
| API mock | `crates/aigw-frontend/tests/steps/api-mocks.ts` —— `sampleSpendLogs` `:109-112`、`EMB_SPEND_ROW` `:115-134`、详情路由分派 `:490-507` |
| 3 viewport | `crates/aigw-frontend/playwright.config.ts:30-42`：`chromium-desktop`(1280×720) / `chromium-mobile`(iPhone SE) / `chromium-tablet`(iPad Mini) —— **每条场景自动跑 3 遍** |
| 「新 call_type」的现成先例 | Stage 111 的 embedding：feature `spend-logs.feature:127-141`、通用 step `the spend log row with call id {string} should show the {string} type badge`（`spend-logs.steps.ts:342-351`）—— **本 Stage 可直接复用该 step，零新 step 代码** |

---

## 3. 方案

### 3.1 搜索行的识别与视觉区分

单一判别式，集中在一处 helper（新增于 `index.tsx` 的 Helpers 区，紧邻 `extractCacheTokens:224` / `imageTokensSource:254`）：

```ts
const SEARCH_CALL_TYPE = "search";
function isSearchRow(log: { call_type: string }): boolean
function extractSearchMeta(metadata: unknown): {
  search_query_count?: number; parent_call_id?: string;
  search_provider?: string; cost_per_query?: number;
} | null
```

`extractSearchMeta` 与 `extractCacheTokens`（`index.tsx:224-253`）**同形**：对 `unknown` 做 typeof/字段逐一校验后返回窄对象，解析失败返回 `null`。字段名逐字对齐 Stage 136 §3.4 的 `metadata` 取值。

类型 badge（第 4 列 `index.tsx:1487-1494` + 移动端 `index.tsx:1581-1583` + 抽屉 pills `index.tsx:706-708`）改为：搜索行用 `variant="secondary"` + 放大镜图标 `<Search className="h-3 w-3" />`（`lucide-react`，该页已在用同库图标 `Clock` / `RefreshCw`），文案走 `t("spendLogs.callType.search")`；其余 `call_type` 原样渲染裸字符串（**不改动既有三种取值的渲染**，避免回归 `spend-logs.feature:141` 的 embedding badge 断言）。

### 3.2 token 列：**刻意**的 `—` + tooltip（核心决策）

> **决策**：不隐藏列，而是**在该单元格内渲染 `—` 并挂 tooltip**。

理由：
1. 表格是 13 列固定布局（`index.tsx:1413-1454`），**按行隐藏列在 HTML table 里做不到**（只能按列隐藏，影响全部行）；
2. 页内**已有完全同构的先例**：image tokens 用 `TooltipProvider` + `Tooltip` + `cursor-help` 区分「upstream 实测 / 本地估算」（`index.tsx:718-745`），同样是「数值本身不足以自解释」的场景 → 沿用同一模式，视觉与交互零学习成本；
3. `0` 这个值**本身是真的**（Stage 136 §3.4 明确三个 token 列填 0 且 `NOT NULL DEFAULT 0`），所以不能改后端，只能改渲染。

落点三处：

| 位置 | 锚点 | 改为 |
|------|------|------|
| desktop token 单元格 | `index.tsx:1542-1560` | 搜索行 → `<span className="text-muted-foreground cursor-help">—</span>` 包 `Tooltip`，content = `t("spendLogs.search.noTokensHint")`；同时**跳过** `extractCacheTokens` 子行（搜索行 metadata 里无 cache 键，本就不渲染，显式短路省一次解析） |
| 抽屉 summary pills | `index.tsx:713-717` | 搜索行 → 用 `{query_count} queries · {result_count} results · {duration}` 替换 `prompt↑/completion↓ · TTFT/duration`；**TTFT 一并去掉**（搜索无流式概念） |
| 移动端卡片 token 行 | `index.tsx:1630-1633` | 同 desktop，`fmtTokens(log.total_tokens)` → `—` + `title` 属性（移动端不挂 Tooltip 组件，与 `index.tsx:1603-1609` 的 `title=` 写法一致） |

### 3.2a spend 列：**`$0.00` 而非 `—`**（与 token 列刻意相反的决策）

> **决策**：token 列用 `—`（因为「token」这个概念对搜索**不适用**）；spend 列用 **`$0.00` + 「未计价」提示**（因为「花费」这个概念**适用，值恰好是 0**）。**两列刻意采取相反处置** —— 区分「不适用」与「值为零」正是让这行读起来像真实记录而非坏数据的关键。

为什么不能把 spend 也渲染成 `—`：

1. `—` 的既定含义在本页已被 §3.2 占用为「该指标对此行不适用」。spend **适用** —— 搜索确实有成本（自建实例的服务器/带宽/运维），只是该部署把 `cost_per_query` 显式置成了 0（Stage 136 §3.1 缺省是 `0.01`，置 0 是刻意选择）；
2. **`spend` 多数情况下非 0**：缺省 `0.01` 与自建摊销单价（典型 `1e-4`）都 `> 0`。若 0 显示 `—`、非 0 显示金额，同一列会在两套部署间形状漂移，用户无法判断「这套部署到底有没有计搜索成本」；
3. 对账视角：`SUM(spend_logs.spend)` 含这行的 `0.0`（Stage 136 §7 的对账等式），UI 把它藏成 `—` 会让「列表金额加总 ≠ 总额」看起来像 bug。

**具体渲染**（三处落点，与 §3.2 的三处并列）：

| 位置 | 锚点 | `spend == 0` | `spend > 0`（缺省 / 摊销单价，**主流形态**） |
|------|------|-------------|------------|
| desktop spend 单元格 | spend 列（`index.tsx:1413-1454` 列定义内，与 §3.2 的 token 单元格同行） | `$0.00`（**正常字色，不用 `text-muted-foreground` 淡化** —— 淡化会读成缺失）+ 紧随一枚极小 `Badge variant="outline"` 文案 `t("spendLogs.search.notPriced")`（en `Not priced` / zh `未计价`），badge 挂 `title` = `t("spendLogs.search.zeroSpendHint")` | 与 LLM 行**完全一致**的 `fmtSpend(log.spend)`，**无 badge** |
| 抽屉 summary / 搜索详情块 | §3.5 的搜索详情块内 | `$0.00` + 一行说明 `t("spendLogs.search.zeroSpendHint")`：「该搜索后端的单位成本被配置为 0；自建实例的基础设施成本未计入账面」 | 金额 + `cost_per_query` 单价快照（既有设计） |
| 移动端卡片 spend | `index.tsx:1603-1609` 区（与 token 行同处） | `$0.00` + `title` 属性承载同一提示（移动端不挂 Tooltip 组件，与既有写法一致） | 同 LLM 行 |

**判别式**：复用 `isSearchRow(log) && log.spend === 0`，**不读配置** —— 前端无配置通道，`spend === 0` 本身就是「单价被显式置 0」的充分信号（搜索一旦发生，`queries ≥ 1`，故 `spend === 0` ⟺ 单价为 0）。

> **一行的最终观感**（`spend == 0` 时）：`🔍 搜索 | searxng/search | — (tokens) | $0.00 未计价 | 2.4s` —— token 的 `—` 说「不适用」，spend 的 `$0.00 未计价` 说「是零，且我知道为什么是零」。两者合起来**不留任何「数据缺失」的解读空间**，这正是本 Stage 要的效果。`spend > 0` 时该行与 LLM 行在 spend 列上**无视觉差异**（只有 token 列的 `—` 和类型 badge 区分），这也是刻意的 —— 有金额就该像有金额。

> ⚠️ **不得暗示金额已校准**：缺省 `0.01` 是 OpenAI 对外牌价占位，通常比自建摊销成本高约 50 倍（Stage 136 §8.1）。本 Stage **不在 UI 上做任何「成本已准确」的表述**，`cost_per_query` 单价快照直接原样展示即可 —— 让部署方自己看出「这个单价是不是我配的」。

### 3.3 父↔子联动 UX —— **选「抽屉内双向跳转」**

三个候选与裁决：

| 候选 | 做法 | 裁决 |
|------|------|------|
| A. 可展开子行 | LLM 行左侧加 ▸，展开渲染其 N 条搜索行 | ❌ 否决。`components/ui/table.tsx` 无嵌套行能力；13 列宽度已靠 `overflow-x-auto`（`index.tsx:1410`）兜，再加展开列会挤压；且搜索行**已经**作为独立行在同一列表里 → 展开会**重复渲染同一行**，分页语义立刻崩（第 1 页的 LLM 行展开出第 2 页的搜索行） |
| **B. 抽屉内双向跳转（选中）** | 搜索行抽屉显示「父调用: `<parent_call_id>`」可点 → 切换抽屉到父行；LLM 行抽屉显示「本次搜索: N 次」徽标，可点 → 列表按 `parent_call_id` 过滤 | ✅ 采用。零新组件（抽屉已有 `setDetailRequestId` + `setSelectedLog` 机制，`index.tsx:1462-1466`）；与分页模型不冲突；父子都是一等公民行，符合 Stage 136 的数据模型 |
| C. 仅在列表加一个「父」图标列 | 第 14 列 | ❌ 否决。只增宽度不增信息；且 LLM 行**无法**知道自己有几个子行（`parent_call_id` 存在子行而非父行里）→ 父→子方向根本做不出来 |

**方案 B 的两个方向**：

| 方向 | 入口 | 数据来源 | 需要后端？ |
|------|------|---------|-----------|
| 子 → 父 | 搜索行抽屉 meta 区新增 `t("spendLogs.drawer.meta.parentCall")` + 截断 call_id + 复制按钮 + 点击切换抽屉 | `metadata.parent_call_id`，**已在列表 DTO 里**（`spend.rs:741` 输出 `metadata`） | **不需要**。点击即 `setDetailRequestId(parent_call_id)`，命中现成的 `/global/spend/logs/{call_id}` 详情端点（`spend.rs:352`） |
| 父 → 子 | LLM 行抽屉 pills 区新增「🔍 N」徽标，点击 → 设 `parentFilter` state → 列表请求带 `parent_call_id=` → 只显示该请求的搜索行；附「清除」胶囊（复用现有筛选区布局 `index.tsx:1244`） | 需要按 `metadata.parent_call_id` 过滤 | ⚠️ **需要后端新增**（§3.4）。徽标上的 N 由**同一次过滤请求的 `total_count`** 得出（不猜不算）；未点击时徽标不显示计数，只显示图标 —— 避免为了一个数字预取 N+1 次 |

### 3.4 后端最小改动：两个新 query 参数

> 这是本 Stage 唯一的后端改动。`call_type` 来自 §2.3 的缺口，`parent_call_id` 来自 §2.4 的缺口。

| 层 | 文件:锚点 | 改动 |
|---|----------|------|
| DTO | `crates/aigw-server/src/routes/spend.rs:38-52` | `SpendLogsQuery` 加 `pub call_type: Option<String>` + `pub parent_call_id: Option<String>` |
| handler | `spend.rs:662-674`（`global_spend_logs`）、`spend.rs:237`（`spend_logs`，同形） | 两个新参数透传给 DB 层 |
| DB | `crates/aigw-core/src/db.rs:8116-8128`（`query_spend_logs_with_status_filter`，11 → 13 参数；已带 `#[allow(clippy::too_many_arguments)]`） | 两个新 `conditions.push`：<br>① `call_type = '<escaped>'`（与 `db.rs:8141` 的 `model` 分支逐字同形）<br>② `parent_call_id` → **三方言分支的 JSON 条件**（见下） |

`parent_call_id` 的三方言 SQL（沿用 `ts_cast`（`db.rs:8134-8137`）已建立的「按 `match self` 出方言差异」模式）：

| driver | 条件 | 既有同类先例 |
|--------|------|------------|
| SQLite | `json_extract(metadata, '$.parent_call_id') = '<esc>'` | `db.rs:3966`（activity 的 cache 键提取） |
| MySQL | `JSON_UNQUOTE(JSON_EXTRACT(metadata, '$.parent_call_id')) = '<esc>'` | `db.rs:3996` 附近的 MySQL 分支 |
| PostgreSQL | `metadata->>'parent_call_id' = '<esc>'` | `db.rs:4029` 附近的 PG 分支 |

转义**必须**走与 `call_id` 分支（`db.rs:8163-8167`）同样的 `replace('\'', "''")`；由于是等值匹配而非 `LIKE`，不需要 `%` / `_` / `\` 的额外转义。无索引（JSON 表达式索引超出本期范围）—— 该过滤只在用户主动点徽标时触发、且总是叠加在时间窗条件之上，不是热路径。

`query_spend_logs_count`（`db.rs:3867-3874`，5 参数）**本期不改** —— 它已经与列表查询口径不一致（§2.5 末行），扩大它会牵动三方言实现（`db.rs:2669`/`:3118`/`:3532`）。父→子徽标的 N 改用**该过滤请求返回的 `data.length`**（搜索行数恒 ≤ 单请求搜索次数，设计 C 下恒为 1，不会跨页）。

### 3.5 抽屉对搜索行的区块裁剪与新增

| 区块 | 搜索行处理 | 锚点 |
|------|-----------|------|
| summary pills 的 `prompt↑/completion↓ · TTFT/duration` | 替换为 query/result/duration（§3.2） | `index.tsx:713-717` |
| image tokens 块 | 天然不渲染（`image_tokens == null`） | `index.tsx:718-745` |
| cache 块 | 天然不渲染（`extractCacheTokens` 返回 `null`） | `index.tsx:827-840` |
| `mcpTool` / `cacheHit` / `cacheKey` / `group` | 天然不渲染（全 `None`，均为 `log.x ? … : null`） | `index.tsx:769-877` |
| **新增** 搜索详情块 | provider / **命中实例 `api_base`** / query 文本 / 结果条数 / `cost_per_query` 单价快照（为 0 时按 §3.2a 附零值说明）/ **父调用跳转**，置于 meta 行之后、prompt/response tab 之前 | 新代码，紧邻 `index.tsx:877` |
| **新增** 命中实例行 | `api_base` 直接渲染（Stage 136 §3.4：该列记**本次真正打中的物理端点**，provider 多实例时各行不同）。标签走 `t("spendLogs.search.instance")`。**列表徽章仍只显 provider 名** —— 实例地址过长且是排查信息，不占列宽。多实例部署下据此可定位「哪台 SearXNG 在拖慢/报错」；**按实例聚合的图表不在本期**（§1 边界） | 同上块内 |
| prompt / response tab | 保留。`messages = {"query": ...}` 不是 messages 形状 → visual tab 落 `noPromptData` 空态（`index.tsx:930`）；**本期不写搜索专属 visual 渲染器**，搜索详情块已覆盖关键信息，raw tab 提供全文 | `index.tsx:900-965` |

### 3.6 筛选器与聚合口径修正

| 改动 | 落点 | 做法 |
|------|------|------|
| `call_type` 下拉 | `index.tsx:1290-1348` 的筛选行内，紧邻 status 下拉（`index.tsx:1324`） | 新 `callTypeFilter` state（默认 `"all"`），选项 `all` / `completion` / `responses` / `embedding` / `search`；进 `useQuery` 的 `queryKey`（`index.tsx:1126-1133`）与 URL 拼接（`index.tsx:1145-1150` 同形） |
| `parentFilter` 清除胶囊 | 同筛选行 | 仅当 `parentFilter` 非空时渲染，显示 `t("spendLogs.filters.parentCall")` + 截断 id + ✕ |
| Usage stat tile「Requests」 | `usage/index.tsx:483`（标题）/ `:492`（取值） | 见 §4.3 表「修正」列 |
| Usage 成功率 | `usage/index.tsx:572`（标题）/ `:581-582`（算式） | 同上 |
| Usage Top Keys 的 requests 排序 | `usage/index.tsx:777-793` | 同上 |
| Usage「Spend by Model Group」的 `'unknown'` 组 | `usage/index.tsx:990` 卡片标题区 | 同上 |
| Dashboard「Total Requests」 | `dashboard/index.tsx:258` | 同上 |

### 3.7 聚合修正的统一策略：**前端口径注脚，不改 SQL**

> **决策**：§4.3 的全部修正走「前端显式标注 + 必要处补充口径」，**不新增 `WHERE call_type != 'search'` 到任何聚合 SQL**。

理由：
1. 聚合 SQL 共 **8 个函数 × 3 方言 = 24 处**（§2.5 锚点），加同一条件的改动面远超收益，且每处都要补 `task bdd-real-{sqlite,pg,mysql}` 三驱动回归；
2. 排除搜索行会让 `SUM(spend)` **少算搜索费** —— 而那恰恰是 Stage 136 要让它可见的东西；对账等式（Stage 136 §7「`SUM(spend_logs.spend)` == `virtual_keys.spend` 增量」）会立刻失败；
3. 真正会错的只有**比值与计数**，不是金额。比值的正确修法是在**展示层说清分母是什么**，而不是偷偷换分母。

因此：`total_requests` 等计数类 tile 保持原值（它确实是「调用次数」），但在 tooltip/副标题里注明「含搜索调用」；对「token/请求」这类必然失真的派生指标，**本期直接不新增**（现有 UI 也没有这个 tile —— 已确认 `usage/index.tsx` 全文无 `avg` 相关计算，§4.3 逐项列出）。

---

## 4. TDD 计划

### 4.1 UT

**后端**（`crates/aigw-server/src/routes/spend.rs` 的 `mod tests`，与既有 `call_type: "completion"` fixture 同处 `:1546` / `:1673` / `:1814` …）：

| UT | 断言 |
|----|------|
| `global_spend_logs_filters_by_call_type_search` | 插 1 条 `completion` + 1 条 `search`，带 `?call_type=search` → `data.len() == 1` 且该行 `call_type == "search"` |
| `global_spend_logs_without_call_type_returns_both` | 不带参数 → 两行都在（**不改默认行为**） |
| `global_spend_logs_filters_by_parent_call_id` | 搜索行 `metadata.parent_call_id = "p1"`，带 `?parent_call_id=p1` → 命中 1 行 |
| `global_spend_logs_parent_call_id_no_match_returns_empty` | `?parent_call_id=nope` → `data` 空、不报错 |
| `global_spend_logs_parent_call_id_rejects_quote_injection` | 传 `p1' OR '1'='1` → 空结果（转义生效），不 500 |
| `global_spend_logs_call_type_and_status_combine` | `?call_type=search&status=success` 两条件 AND |
| `spend_logs_search_row_list_dto_has_metadata` | 搜索行经列表 DTO（`spend.rs:716-751`）后 `metadata.search_query_count` / `.parent_call_id` 仍在（防未来有人裁字段） |

**前端**：该 crate 无 vitest（`crates/aigw-frontend` 下仅 playwright-bdd），helper 的行为由 §4.2 的 BDD 覆盖。`extractSearchMeta` 的 `null` 分支通过「metadata 缺失的搜索行不崩」场景间接锁定。

### 4.2 BDD（`crates/aigw-frontend/tests/features/spend-logs.feature`，3 viewport 各跑一遍）

先在 `api-mocks.ts` 加 fixture（与 `EMB_SPEND_ROW:115-134` 同形）：

- `SEARCH_SPEND_ROW`（**非零 spend 形态 = 主流部署**，缺省 `0.01` 或自建摊销单价）：`call_id: "req-search-001"`、`call_type: "search"`、`model: "searxng/search"`、`custom_llm_provider: "searxng"`、`api_base: "http://searxng-a:9099"`、`model_group: null`、`total_tokens/prompt_tokens/completion_tokens: 0`、`spend: 0.01`、`ttft_ms: null`、`request_duration_ms: 2430`、`metadata: { search_query_count: 1, parent_call_id: "req-001", search_provider: "searxng", cost_per_query: 0.01 }`
- `SEARCH_ZERO_SPEND_ROW`（**零 spend 形态**，单价被显式置 0，§3.2a 的被测对象）：同上但 `call_id: "req-search-002"`、`spend: 0`、`metadata.cost_per_query: 0`、`api_base: "http://searxng-b:9099"`（顺带覆盖「同 provider 不同实例」）
- `SEARCH_ROW_ALT_PROVIDER`（**防 provider 名硬编码**）：`custom_llm_provider: "stubsearch"`、`model: "stubsearch/search"`、`spend: 0.008` —— 断言 badge/列文案随数据变化，而非写死 `searxng`
- `sampleDetailSearch`：+ `messages: {"query":"aigw rust gateway"}`、`response: {"result_count":5,"results":[…]}`；接入详情分派链（`api-mocks.ts:490-507`）
- `SEARCH_ROW_NO_META`：同上但 `metadata: {}` —— 容错场景用
- `/global/spend/logs` mock 需识别 `call_type=` 与 `parent_call_id=` 两个 query 参数并过滤

场景（全部走 `# ── Stage 137: web search call rendering ──` 分节注释，与 `spend-logs.feature:124` / `:143` 的分节风格一致）：

| # | Scenario | 关键断言 | 复用既有 step？ |
|---|----------|---------|---------------|
| 1 | `Spend log list type badge shows search call type` | 行含 `search` badge | ✅ `spend-logs.steps.ts:342` 原样复用 |
| 2 | `Search row renders em dash instead of zero tokens` | 该行 token 单元格文本为 `—`，且**不含** `0 / 0` | 新 step |
| 3 | `Search row token cell exposes a no-token tooltip` | hover → 出现 `cursor-help` 元素与提示文案 | 新 step（可参考 image-token tooltip 的渲染结构 `index.tsx:718-745`） |
| 4 | `Search row shows provider and spend in the list` | 行含 `searxng` 与 `$0.01`（**provider 名取自数据**） | 新 step（`:22` 的 `\$\d+\.\d+` 正则可借形） |
| 4a | `Zero spend search row renders a real zero amount` | `SEARCH_ZERO_SPEND_ROW` 行的 spend 单元格文本为 **`$0.00`**，**不是** `—`、不是空；同行 token 单元格**仍是** `—` —— 两列形状刻意不同（§3.2a） | 新 step |
| 4b | `Zero spend search row carries a not-priced affordance` | 该行含 `t("spendLogs.search.notPriced")` 徽标，其 `title`/tooltip 含零值解释文案；**非零 spend 行（`SEARCH_SPEND_ROW`）不含该徽标**，其 spend 单元格与 LLM 行形状一致 | 新 step |
| 4c | `Search row provider label is not hardcoded` | `SEARCH_ROW_ALT_PROVIDER` 行显示 `stubsearch` 而非 `searxng` —— 防本期单 provider 导致的硬编码 | 新 step |
| 5 | `Search detail drawer shows query text and result count` | 抽屉含 `aigw rust gateway` 与 `5` | 新 step |
| 5a | `Search detail drawer shows the instance that served the call` | 抽屉含 `http://searxng-a:9099`（命中实例 `api_base`，§3.5）；打开 `SEARCH_ZERO_SPEND_ROW` 抽屉时显示 `http://searxng-b:9099` —— **同 provider 不同实例可区分** | 新 step |
| 5b | `Zero spend drawer explains why the amount is zero` | `SEARCH_ZERO_SPEND_ROW` 抽屉含 `$0.00` 与零值说明文案；**不含** `—` 作为金额 | 新 step |
| 6 | `Search detail drawer shows parent call link` | 抽屉含父调用区块与 `req-001` 截断形 | 新 step |
| 7 | `Clicking parent call link switches the drawer to the LLM row` | 点击后抽屉标题/badge 变为 `req-001` | 新 step |
| 8 | `LLM detail drawer shows a search badge linking to its search rows` | `req-001` 抽屉含 🔍 徽标 | 新 step |
| 9 | `Clicking the search badge filters the list by parent call` | 请求 URL 含 `parent_call_id=req-001`，列表只剩搜索行 | 新 step（`:36` 的「query should include page_size=50」是现成的 URL 断言范式） |
| 10 | `Call type filter narrows the list to search calls` | 选 `search` → 请求含 `call_type=search`，列表无 `completion` 行 | 新 step |
| 11 | `Call type filter set to all shows both LLM and search rows` | 默认态两类都在（防回归） | 新 step |
| 12 | `Search row with empty metadata still renders without crashing` | `SEARCH_ROW_NO_META` 行可见、页面无 error boundary | 新 step |
| 13 | `Search detail drawer hides cache and TTFT blocks` | 抽屉**不含** `Cache` / `TTFT` 文案 | 新 step |
| 14 | `Mobile search card renders em dash tokens and provider` | `Given the viewport is mobile size 375x667` → 卡片含 `—`（token）与 `searxng`；**零 spend 卡片含 `$0.00` 而非 `—`** | 复用既有 viewport Given（`spend-logs.feature:34`） |

> 20 条 × 3 viewport = **60 个 playwright 用例**（原 14 条 + §3.2a/§3.5 新增 6 条）。场景 14 显式再设 mobile viewport（与 `spend-logs.feature:33-36` 同写法），因为 3 个 project 里只有一个是 mobile。

### 4.3 集成验证 —— 聚合口径逐项审计（本 Stage 的正确性核心）

对每一处做：① 构造「1 LLM 行 + 1 搜索行」的数据 → ② 读该端点 → ③ 判定是否失真 → ④ 按「修正」列处置。审计必须逐项在门禁里打勾。

| # | 口径 | 端点 / 前端锚点 | 失真判定 | 修正 |
|---|------|----------------|---------|------|
| 1 | Usage「Requests」tile | `/global/spend/activity` → `usage/index.tsx:492` | ✅ 失真：值 = 2（实际 1 次客户端请求） | **保留数值**（它确实是调用数）+ tile 加 tooltip `t("usage.cards.requestsHint")`：「含按次计费的搜索调用」。不改 SQL（§3.7） |
| 2 | Usage「Rate」成功率 | `usage/index.tsx:581-582` | ✅ 失真：分子分母同增，比值被拉向搜索成功率 | 同 tooltip 注明分母含搜索调用。**不改公式** —— 换分母要么漏掉搜索失败、要么与 tile 1 不自洽 |
| 3 | Usage「Tokens」tile | `usage/index.tsx:549` | ❌ 不失真（搜索 token 为 0，`SUM` 不变） | 无需改 |
| 4 | Usage「Spend」tile | `usage/index.tsx` spend card | ❌ 不失真且**正确**（搜索费本该入账）。显式置 0 的部署下搜索贡献 `+0.00`，总额不变 —— 仍正确 | 无需改 |
| 5 | Usage「OK」/「Failed」计数 | `query_activity_metadata` 的 `COUNT(CASE status…)` `db.rs:3961-3965` | ✅ 轻度失真（计数含搜索行） | 与 #1 同一 tooltip 覆盖 |
| 6 | Usage 趋势图 requests 系列 | `query_activity_daily:4058` / `_hourly:4163` → `usage/index.tsx:671` tooltip | ✅ 失真：曲线整体抬升 | 图表 tooltip 文案（`usage.chart.totalRequests`）加「含搜索」限定。**曲线形状本身是真实的调用量**，不拆分 |
| 7 | Usage「Spend by Model」 | `aggregate_spend_by_model` → `usage/index.tsx:292` | ⚠️ SQL 侧 `ORDER BY total_tokens DESC` 让搜索组垫底，但**前端会重新排序**：`globalChartMode` 默认 `"spend"`（`usage/index.tsx:256`），四处比较器（图/榜 × group/model：`:1259-1266`、`:1128-1134`、`:1329-1335`、以及 model 图 `:1260` 区）均在 `.slice(0, 5)` **之前**按当前 mode 重排 → **按 spend 时搜索组可正常进 Top 5** | ⚠️ **但 `.slice(0, 5)` 作用于已重排的数组，而数组本身是 SQL 按 token 降序返回的全量** —— 全量未截断故无遗漏。**本期无需改代码**，两种配置各验一次：**① 填了摊销单价**（`spend>0`）→ 构造「搜索 spend 高于某 LLM」的数据，确认 `searxng/search` 出现在按 spend 排序的 Top 5 中；**② 显式填 `0.0`**（`spend=0`，非默认配置）→ 该组 spend 与 token **双 0**，按 spend 排序必然垫底、不进 Top 5 → **这是正确行为，不是 bug**（花费确实为 0，没有理由占据 Top 5 名额），**无需补任何提示**。切到 `tokens` mode 时搜索组必然垫底，同样正确（它确实零 token） |
| 8 | Usage「Spend by Model Group」 | `aggregate_spend_by_model_group` → `usage/index.tsx:316` | ⚠️ **真失真**：搜索行 `model_group IS NULL` → 并入 `'unknown'`，污染该组口径 | **验证并记录**：`'unknown'` 组自此含搜索花费。两个选项择一：① 前端把 `'unknown'` 组文案改为 `t("usage.unknownGroupHint")` 并在 tooltip 说明构成；② Stage 136 改填 `model_group`（**已被 Stage 136 §3.4 明确否决** —— 会污染本图）。**本期取 ①** |
| 9 | Usage「Spend by Provider」 | `aggregate_spend_by_provider` → `usage/index.tsx:303` | ❌ 口径不失真（`searxng` 自成扇区，花费归属正确），但 ⚠️ **零值扇区的可见性是真问题**：显式填 `0.0` 时 `SUM(spend)=0` → donut 零面积（**非默认配置**：缺省 `0.01` 下扇区正常可见），且 `mergeSmallProviders`（`usage/index.tsx:201`）的 <1% 阈值几乎**必然**把它并进 `others` → 「搜索厂商花费独立可见」这一优点在默认配置下失效 | **不改 SQL、不改 `mergeSmallProviders` 阈值**（动阈值会影响所有真实低额 provider）。处置：**① 验证并记录**两种配置下的扇区行为（`spend>0` 时扇区正常出现；`spend=0` 时被并入 `others` 或零面积）；**② 把「0 花费」的解释放在它该在的地方 —— Spend Logs 页的行级 `$0.00 自建` 徽标（§3.2a）**，而不是往 donut 上硬塞一个零面积扇区的注脚（donut 的语义是花费占比，一个 0 占比条目本就无可展示）。**③ 结论写进验收记录**：「`searxng` 在 Spend by Provider 上不可见 ⟺ 该部署把单价显式置 0」（缺省 `0.01` 下可见）—— 这是**配置信号，不是 UI 缺陷**，并据此在 Stage 136 §8.1 的风险缓解里互相引用 |
| 10 | Usage「Top Virtual Keys」按 requests 排序 | `aggregate_spend_by_keys` `db.rs:4265` → `usage/index.tsx:777-793` | ✅ 失真：启用搜索的 key 请求数翻倍，**排名可能换序** | 排序选择器的 requests 选项加 `title` 注明含搜索调用。按 spend / tokens 排序不受影响（验证并记录） |
| 11 | Dashboard「Total Spend」 | `get_global_spend` `db.rs:2473` → `dashboard/index.tsx:128` | ❌ 不失真（应含搜索费） | 无需改 |
| 12 | Dashboard「Period Spend」 | 前端 `reduce` over `?limit=100` 的 `data`，`dashboard/index.tsx:168-172` | ✅ **双重失真**：搜索行挤占 100 条窗口 → 覆盖时间缩短 → 周期花费被低估 | **既有缺陷被搜索放大**（limit=100 的窗口本就不等于周期）。处置：登记 §8 遗留；本期在该卡片加「近 100 条调用」限定文案，不改抓取策略（改 limit 会放大 payload） |
| 13 | Dashboard「Total Requests」 | `logsData?.count ?? 0`，`dashboard/index.tsx:258` | ✅ **严重失真**：`count` 是**本页返回条数**（≤100），搜索行使它更快贴满 100 | **本期必须改**：改读 `logsData?.total_count`（后端已输出，`spend.rs:758` 的信封含 `total_count`）→ 顺手修掉一个既有 bug；并加「含搜索调用」文案 |
| 14 | Spend Logs 分页 `total_count` | `query_spend_logs_count` `db.rs:3867` → `PaginationBar` `index.tsx:1399` / `:1653` | ✅ 行数翻倍 → 页数翻倍 | 预期行为（搜索行是真行）。配合 §3.6 的 `call_type` 筛选让用户可排除；登记 §8 |
| 15 | `/spend/tags` 日聚合链路 | `spend.rs:598`；`daily_spend_queue.rs:96-161` 聚合键 | — | **本期不审计**：前端无页面消费（已 grep 确认 `usage` / `dashboard` / `spend-logs` 三页均无 `/spend/tags` 调用）。登记 §8 |

### 4.4 i18n

**新增键**（命名沿用既有层级：`spendLogs.{table,filters,drawer.meta,search,callType}` / `usage.{cards,chart}`）：

| key | en | zh-CN |
|-----|----|----|
| `spendLogs.callType.search` | `Search` | `搜索` |
| `spendLogs.callType.completion` | `Completion` | `对话` |
| `spendLogs.callType.responses` | `Responses` | `Responses` |
| `spendLogs.callType.embedding` | `Embedding` | `向量化` |
| `spendLogs.filters.callType` | `Call Type` | `调用类型` |
| `spendLogs.filters.callTypePlaceholder` | `Type` | `类型` |
| `spendLogs.filters.parentCall` | `Parent call` | `父调用` |
| `spendLogs.search.noTokens` | `—` | `—` |
| `spendLogs.search.noTokensHint` | `Search is billed per query and consumes no tokens` | `搜索按次计费，不消耗 token` |
| `spendLogs.search.queries` | `{{count}} queries` | `{{count}} 次查询` |
| `spendLogs.search.results` | `{{count}} results` | `{{count}} 条结果` |
| `spendLogs.search.provider` | `Search provider` | `搜索供应商` |
| `spendLogs.search.instance` | `Served by` | `命中实例` |
| `spendLogs.search.notPriced` | `Not priced` | `未计价` |
| `spendLogs.search.zeroSpendHint` | `Unit cost for this search backend is configured as 0; self-hosted infrastructure cost is not reflected here` | `该搜索后端的单位成本被配置为 0；自建实例的基础设施成本未计入账面` |
| `spendLogs.search.costPerQuery` | `Unit price / query` | `单价 / 次` |
| `spendLogs.search.childBadge` | `Search calls` | `搜索调用` |
| `spendLogs.search.viewChildren` | `View search calls for this request` | `查看该请求的搜索调用` |
| `spendLogs.search.spendNotAuthoritative` | `CNY-priced providers are converted by configuration; not for billing` | `以人民币计价的供应商由配置方换算，不可用于对外出账` |

> `spendLogs.search.spendNotAuthoritative` **本期键在、组件在、但无数据会触发它**（Bocha 未实现，§1 边界）—— 刻意预埋，使 Bocha 接入时前端零改动。渲染条件按 provider 名判定（`search_provider === "bocha"`），而非写死「所有搜索行都显示」。
| `spendLogs.drawer.meta.parentCall` | `Parent Call:` | `父调用：` |
| `spendLogs.drawer.searchDetail` | `Search Detail` | `搜索详情` |
| `usage.cards.requestsHint` | `Includes per-query search calls` | `含按次计费的搜索调用` |
| `usage.cards.rateHint` | `Denominator includes search calls` | `分母含搜索调用` |
| `usage.chart.requestsHint` | `Includes search calls` | `含搜索调用` |
| `usage.unknownGroupHint` | `Calls without a model group, including search calls` | `无模型组的调用，含搜索调用` |
| `dashboard.totalRequestsHint` | `Includes search calls` | `含搜索调用` |

`spendLogs.drawer.metadata` 复用既有键（已定义、源码零引用，§2.2 注）。两份 locale 的键集必须完全对称 —— `crates/aigw-frontend/src/i18n/resources.d.ts` 由 locale 推导类型，缺键即 `tsc -b` 报错（`task fe-lint` 内含 `npx tsc -b`，`Taskfile.yml:112`）。

---

## 5. 变更清单

| 文件 | 改动 |
|------|------|
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | `isSearchRow` / `extractSearchMeta` helper（紧邻 `:224` / `:254`）；类型 badge 分支（`:1487` / `:1581` / `:706`）；token 单元格 `—`+tooltip（`:1542` / `:713` / `:1630`）；抽屉搜索详情块 + 父调用跳转（`:877` 后）；LLM 抽屉子调用徽标；`callTypeFilter` / `parentFilter` state + 下拉 + URL 拼接（`:1050` / `:1126` / `:1145` / `:1324` 区） |
| `crates/aigw-frontend/src/pages/usage/index.tsx` | tile tooltip（`:483` requests / `:572` rate 成功率算式在 `:581-582`）；`'unknown'` model group 文案（`:990` 卡片标题区）；Top Keys requests 排序注脚（`:777-793`）。**排序逻辑本身不改** —— `globalChartMode` 默认已是 `"spend"`（`:256`） |
| `crates/aigw-frontend/src/pages/dashboard/index.tsx` | `logsData?.count` → `logsData?.total_count`（`:258`，顺带修既有 bug）；Period Spend 的「近 100 条」限定文案（`:231` 区） |
| `crates/aigw-frontend/src/i18n/locales/en.json` | §4.4 全部新键 |
| `crates/aigw-frontend/src/i18n/locales/zh-CN.json` | 同上，键集对称 |
| `crates/aigw-server/src/routes/spend.rs` | `SpendLogsQuery` 加 `call_type` / `parent_call_id`（`:38-52`）；`global_spend_logs`（`:662`）与 `spend_logs`（`:237`）透传；7 个新 UT |
| `crates/aigw-core/src/db.rs` | `query_spend_logs_with_status_filter`（`:8116`）加两个参数 + 两条 `conditions.push`（`parent_call_id` 三方言 JSON 分支） |
| `crates/aigw-frontend/tests/features/spend-logs.feature` | 20 条新场景（§4.2） |
| `crates/aigw-frontend/tests/steps/spend-logs.steps.ts` | 新 step 定义（场景 1 / 14 复用既有 step） |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | `SEARCH_SPEND_ROW` / `SEARCH_ZERO_SPEND_ROW` / `SEARCH_ROW_ALT_PROVIDER` / `sampleDetailSearch` / `SEARCH_ROW_NO_META`；详情分派链（`:490-507`）；列表 mock 识别 `call_type` / `parent_call_id` |
| `docs/12-technical-debt.md` | 登记 §8.2 各条 |
| `docs/stages/stage-roadmap.md` + `docs/11-next-steps.md` | Phase 53 进度回写 |
| **不改** | 8 个聚合函数 × 3 方言共 24 处 SQL（§3.7 决策）；`query_spend_logs_count`（`db.rs:3867`，§3.4 末）；`buildCSVHeaders`（`index.tsx:269`）；`components/ui/table.tsx`；任何 migration |

---

## 6. 回归验证

1. `task fe-lint` — `npm run lint` + `npx tsc -b` 全绿（i18n 键集对称由 `resources.d.ts` 把关）
2. `task fe-bdd` — 3 viewport 全绿；基线 15 feature 的既有场景**零回归**，特别是：
   - `spend-logs.feature:141` embedding type badge（§3.1 刻意不改非搜索 `call_type` 的渲染）
   - `spend-logs.feature:135-141` 的 embedding 抽屉断言（prompt_tokens / 向量维度）
   - `spend-logs.feature:117-121` multimodal marker（token 单元格改动不得影响 model 列的 🖼️）
   - `dashboard.feature` 全部（`total_count` 替换后数值变化）
3. `task test` — aigw-server UT 全绿（7 个新 UT）
4. `task bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` — 三驱动全绿（`parent_call_id` 的 JSON 条件**必须三方言各验一遍**，这是本 Stage 后端改动的唯一风险点）
5. `task fmt` / `task lint` green
6. **默认行为不变**：不带 `call_type` / `parent_call_id` 时 `/global/spend/logs` 与 `/spend/logs` 的响应与改动前一致（UT `..._without_call_type_returns_both` + 既有 BDD 守护）
7. **未启用搜索时**：`spend_logs` 无 `search` 行 → Spend Logs / Usage / Dashboard 三页渲染与改动前逐像素一致（除 Dashboard `total_count` 的有意修正）

---

## 7. 门禁

- [ ] **TDD 红绿（本 Stage 强制）** —— 用户 2026-10-08 决策：Stage 134/135/136 的「测试与实现同批编写」不再沿用，本 Stage **每个 UT/BDD 必须先跑红再写实现**。红绿过程与证据写进 §Implementation Notes


- [ ] 7 个新后端 UT 先 fail 后 pass（TDD 红绿），含注入转义与 `parent_call_id` 无命中两条
- [ ] 20 条新 BDD 场景 × 3 viewport = 60 用例全绿
- [ ] 既有 BDD 零回归（embedding badge / embedding 抽屉 / multimodal marker / dashboard 四组重点复验）
- [ ] 搜索行 token 单元格渲染为 `—` 且带可见 tooltip，**全站无 `0 / 0`**（含移动端卡片）
- [ ] **spend 双态验收**：`spend == 0` 渲染为 `$0.00` + `未计价` 徽标（**全站无把金额渲染成 `—` 的情形**）；`spend > 0` 的 spend 单元格与 LLM 行形状一致、无徽标（§3.2a，三处落点各验）
- [ ] 抽屉展示命中实例 `api_base`，同 provider 的两个实例在抽屉中可区分（§3.5）
- [ ] provider 名、`model`、金额**均取自数据不硬编码**（`SEARCH_ROW_ALT_PROVIDER` 场景为证）
- [ ] 父↔子双向跳转端到端可用（子→父切抽屉；父→子过滤列表）
- [ ] `call_type` 筛选五个取值均生效，默认 `all` 不改变既有行为
- [ ] §4.3 的 **15 项聚合口径逐项审计完成并逐项打勾**（不失真的也要显式标注「已验证不失真」）；**#7 / #9 两项须在 `spend>0` 与 `spend=0` 两种配置下各验一次**
- [ ] Dashboard「Total Requests」改读 `total_count`（既有 bug 一并修掉）
- [ ] en + zh-CN 键集对称，`npx tsc -b` 无 i18n 类型错
- [ ] `task fe-lint` / `task fe-bdd` / `task test` / `task fmt` / `task lint` 全绿
- [ ] `task bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` 三驱动全绿（JSON 条件方言验证）
- [ ] `docs/12-technical-debt.md` 登记 §8.2 各条
- [ ] `docs/stages/stage-roadmap.md` + `docs/11-next-steps.md` 回写
- [ ] git commit（精确 add；`--signoff`）

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| **搜索行使日志列表行数翻倍** | `query_spend_logs_count`（`db.rs:3867`）返回值翻倍 → 分页页数翻倍；默认 `page_size=30`（`spend.rs:657`）下，一屏能看到的**客户端请求数砍半**；Live tail（15s，`index.tsx:1042`）每轮刷到的有效信息密度下降 | §3.6 的 `call_type` 筛选让用户一键只看 LLM 调用（排除搜索）或只看搜索；**不**默认排除搜索行 —— 隐藏真实计费记录比列表变长更糟 |
| ⏸ **Bocha 以 CNY 计价而 `spend` 语义恒为 USD（Stage 136 §8.2 已降级为延后项）** | **本期不发生** —— Bocha 未实现（Phase 53 只有 SearXNG，USD 原生），UI 上不会出现任何 CNY 来源的金额。接入后风险依旧：配置方手工换算的汇率会随时间漂移，UI 上显示的 `$0.00507/次` 既不是人民币牌价也不是当日美元价 → **以 Bocha 为后端的搜索花费在 UI 上不可作为权威账目** | 本 Stage **预埋**提示组件与 i18n 键（`t("spendLogs.search.spendNotAuthoritative")`，§4.4），渲染条件按 `search_provider === "bocha"` 判定 —— **本期无数据触发，Bocha 接入时前端零改动即生效**。该提示在 Stage 136 §8.2 的三个候选（USD 单币种 / 固定汇率配置 / `currency` 字段 + 汇率快照）任一落地后移除。**在此之前不得把 Bocha 搜索花费导出为对外账单** |
| **`spend == 0` 被误读为「数据缺失」** | 单价显式置 0 的部署下，搜索行同时出现 token `—` 与 spend `$0.00`，是本页**唯一两个数值列都无实质内容**的行形态 → 运维第一眼易判为脏数据 | §3.2a 的刻意双向处置：`—` 专表「不适用」、`$0.00` + `未计价` 徽标专表「值为零且已知原因」，两者形状必须不同（场景 4a / 4b 为门禁）。**不得把 spend 也写成 `—`**，否则两种语义合并、且与 `spend>0` 部署形状漂移 |
| `parent_call_id` 的 JSON 条件三方言行为不一致 | SQLite 的 `json_extract` 对非法 JSON 返回 `NULL`（静默不匹配），MySQL 的 `JSON_EXTRACT` 对非法 JSON **报错**，PG 的 `metadata->>` 要求列为 `jsonb`/`json` 类型 → 某驱动上可能 500 | 门禁强制三驱动 BDD；UT 覆盖「`metadata` 为空 / 非对象」的行（`SEARCH_ROW_NO_META` fixture 的后端对偶） |
| `parent_call_id` 过滤无索引 | JSON 表达式扫全表；`spend_logs` 行数本就因搜索翻倍 | 该条件**总是**叠加在时间窗之上（前端永不单独发 `parent_call_id`，`index.tsx` 的 `start_date`/`end_date` 恒在），且只在用户点徽标时触发；JSON 表达式索引登记 §8.2 |
| 抽屉的 prompt visual tab 对搜索行落空态 | 用户看到 `noPromptData`（`index.tsx:930`）会以为数据丢了 | 搜索详情块（§3.5）已把 query/结果数前置渲染；raw tab 有全文。**本期不写搜索专属 visual 渲染器**（登记 §8.2） |
| 既有 BDD 对 token 文案的隐式依赖 | 若某条既有场景断言过 `0`（如 embedding 的 `completion_tokens: 0`），§3.2 的改动**仅**作用于 `isSearchRow` 为真的行，embedding 行不受影响 | 门禁第 3 条显式复验 embedding 两组场景 |

### 8.2 遗留（登记 TD）

| TD | 内容 |
|----|------|
| **搜索 provider/实例与定价的 DB 化与 UI 管理 → Stage 138** | 本期前端对搜索**只读展现日志**，provider / instances[] / `cost_per_query` 全部来自 `config.yaml`（改价需改配置 + 重启），**无任何管理界面**。Stage 138 将仿 `proxies` 表 / `027_proxies.sql` / Phase 50 全套，做 DB 表 + admin CRUD + 前端管理页。本 Stage 的 `metadata.cost_per_query` 单价快照渲染（§3.5）在 DB 化后价值更高 —— 运行时改价后历史行仍显示当时单价 |
| **首批待接入 provider 的前端影响（抽象已就位，接入是纯增量）** | Phase 53 只实现 SearXNG → provider badge 本期恒为 `searxng`。**Tavily** 与**博查 Bocha** 接入后前端**零改动**即可渲染（badge / `model` / provider 名全部取自数据，`SEARCH_ROW_ALT_PROVIDER` 场景已证），唯一需要启用的是 Bocha 的 `spendNotAuthoritative` 提示（已预埋，§8.1）。Tavily 带 `score` 字段但 Stage 135 §3.5 决定不渲染，故也无前端工作 |
| **按实例聚合的花费视图** | 本期命中实例（`api_base`）只在抽屉可见（§3.5）。多实例部署下若要回答「哪台 SearXNG 花了多少 / 承载了多少次」，需新增按 `api_base` 分组的聚合函数（三方言）+ 新图表 —— 远超本期范围。当前可用的替代：`call_type=search` 筛选 + 逐行看抽屉 |
| ⚠️ **Dashboard「Period Spend」的 `limit=100` 窗口语义错误（既有缺陷，被搜索放大）** | `dashboard/index.tsx:168-172` 对 `?limit=100` 的 `data` 做 `reduce` 求和，并把它当作「周期花费」。但这是**最近 100 条调用**的和，不是周期和 —— 周期内调用超过 100 条时就低估。搜索行让每个请求占 2 行，使天花板**提前一半**撞到。本期只加「近 100 条调用」限定文案；正确修法是新增一个服务端周期聚合端点（或复用 `/global/spend/activity` 的 `total_spend`，该字段已存在于 `ActivityMetadata`，`spend.rs:1126`）。超出本 Stage 范围 |
| 「Spend by Model Group」的 `'unknown'` 组语义混杂 | §4.3 #8：搜索行 `model_group IS NULL` 与「真的没打 group 的 LLM 调用」共享 `'unknown'` 组（`db.rs:2530` 的 `COALESCE(model_group,'unknown')`）。本期只加文案提示。彻底修法需要在聚合 SQL 里按 `call_type` 分拆 —— 牵动 3 方言 × 1 函数，且会与 §3.7「不改聚合 SQL」的决策冲突，留给「若搜索成为高频特性」时重做 |
| `/spend/tags` 与 `daily_*_spend` 链路未审计 | §4.3 #15：该链路前端无消费者（已 grep 三页确认），且 `daily_spend_queue.rs:96-161` 的 8 元组聚合键含 `model`（搜索行为 `"<provider>/search"`，自成一组）与 `mcp_namespaced_tool_name`（Stage 136 §3.4 刻意填 `None` 以免裂分组）。一旦前端加入「按 tag / 按日」的报表页，必须重做本审计 |
| `metadata.parent_call_id` 的 JSON 表达式索引 | §8.1 列出的全表扫风险。三方言各有写法（SQLite 生成列 + 索引 / MySQL functional index / PG `btree((metadata->>'parent_call_id'))`），需 migration。本期因该过滤非热路径而不做 |
| 搜索行的 prompt visual 渲染器 | `parseMessages`（`components/log-viewer/MessageViewer.tsx`）是围绕 chat messages 形状写的；`{"query": "..."}` 与 `{"result_count": M, "results": [...]}` 需要专属卡片（类似 Stage 111 为 embedding 做的向量渲染）。本期靠搜索详情块 + raw tab 兜 |
| 「平均 token/请求」类派生指标 | §3.7 决策：现有 UI 无此 tile（已 grep `usage/index.tsx` 确认无 `avg`/`per-request` 计算），故本期无需修正。**未来若新增此类指标，分母必须显式排除 `call_type='search'`** —— 否则上线即错。此条须写入 `docs/12-technical-debt.md` 作为前置约束 |
| 搜索行的 CSV 导出列 | `buildCSVHeaders`（`index.tsx:269-283`）的 13 列对搜索行导出 `0/0/0` token 与空 TTFT。若运营要按次对账，需加 `search_query_count` / `search_provider` 两列并同步 `exportToCSV`（`index.tsx:289`） |
| `SpendLogsQuery.session_id` 死参数 | `spend.rs:45` 定义但 `global_spend_logs`（`spend.rs:662-674`）与 `spend_logs`（`spend.rs:237`）都没传给 DB → 该 query 参数静默无效。搜索行与父行共享 `session_id`（Stage 136 §3.4），把它接通本可提供**第二条**父子线索（且是可索引的真列，比 JSON 过滤更优）。本期因 §3.3 已选定 `parent_call_id` 路线而不做，但这是比 JSON 索引更划算的后续优化 |
