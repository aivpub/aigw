# Stage 139: 搜索消费日志的结果可视化（Phase 54）

**所属**: Phase 54（搜索调用日志可视化）
**预估**: ~8h（后端投影 + 前端四块 + i18n + BDD）
**依赖**: Stage 137（`call_type="search"` 行与抽屉搜索块）、Stage 136（`response.results[]` 的写入方）
**状态**: ✅ 完成（2026-10-16，Gate 3 实现 + 门禁全绿；**Gate 4 代码评审未做**；未提交）

---

## 1. 问题

Stage 136 在每条 `call_type="search"` 行的 `response` 列里**完整写入了检索结果**：

```json
{"result_count": 3, "results": [
  {"title": "Discord.js deleteCommand", "url": "https://stackoverflow.com/q/42708210",
   "snippet": "[javascript, bots, ping, discord] Hercules // score: 0",
   "score": 2.200000047683716, "published_date": null}
]}
```

**但界面上一条都看不到。** 三个独立的洞：

| # | 洞 | 后果 |
|---|----|------|
| 1 | 抽屉把 `response` 交给通用 `OutputCard`，而它的 `parseOutput()` 只认 `choices[]` / `output[]` / `data[]`（embeddings）三种形状 —— 搜索的 `{result_count, results[]}` **落到兜底 `empty`** | 「输出」标签页**渲染成空白**，用户以为搜索没返回东西 |
| 2 | 列表的 Model 列对搜索行显示 `model` 字段原文 `{provider}/search` | 内部串当产品文案；真正有用的（结果数、命中实例）不可见 |
| 3 | 抽屉的 `search-detail` 块把 provider/实例/次数/结果数/单价**挤成一行文本** | 信息密度低，扫不出重点 |

**这一 Stage 只做「把已经存了的东西画出来」**，不新增后端采集、不新建页面。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | **`response.results[]` 在所有搜索行里都存在且形状一致**（`title`/`url`/`snippet` 必有，`score`/`published_date` 可缺） | 结果卡片大面积空白；空态文案成为主路径 | 查真实库现有搜索行的 `response` 逐条核对（已完成：`01a133ec-…` 形状符合） |
| H2 | **`result_count` 与 `results.len()` 会不一致** | 若事实上恒等，3.4 的「留存 N / 共 M」后缀是多余复杂度 | 已证实不一致：`api-mocks.ts` 的 `sampleDetailSearch` 是 5 vs 2 |
| H3 | **列表 DTO 不含 `response`，但 DB 查询已 SELECT 该列** —— 故列表加 `result_count` 无需改 SQL | 若 DB 层也没取，就要改列表 SQL 拉大列 | **实测**（`GET /global/spend/logs?call_type=search`）：DTO 29 键无 `response`/`messages`；但 `db.rs:9058` 的列表 SELECT **含 `response`** → handler 里 `log.response` 已在内存 |
| H4 | **不改通用 `OutputCard` 也能覆盖搜索**（独立组件即可） | 若要复用既有渲染能力（折叠/主题），独立组件的维护面会翻倍 | 读 `OutputCard.tsx` 的 `ParsedOutput` 契约 —— 它的三形状与搜索形状无重叠 |
| H5 | **投影是纯函数**，可脱离 HTTP 层单测 | 若投影被迫依赖 DB/AppState，7 条 UT 退化为集成测试，红绿循环变慢 | 投影数据全来自入参 `&SpendLog`，无 IO |
| H6 | **抽屉的 Input 区对搜索行本就是空的**（`parseMessages` 不认 `{query}`） | 若它其实能渲染，隐藏 Input 区就是无谓的信息损失 | 读 `MessageViewer.tsx:33-66` —— 只认数组与 `record.messages`，其余 `return empty` |

---

## 2. 边界

**做**：列表搜索行的呈现、抽屉搜索信息的分组、抽屉输出区的检索结果列表、i18n、BDD。

**不做**（明确排除）：
- **不做**按 provider/实例聚合的花费视图、搜索量趋势、命中率 —— 那是新页面，登记为后续 Phase
- **不做**结果条目的「点击展开全文」/ 内联抓取 —— 只按存下来的 `snippet` 展示
- **不做**搜索结果的分页/搜索/排序 —— 单次搜索的结果集（≤ `max_results`，默认 5）本就很小
- **不改** `response` 的写入形状（Stage 136 落库格式不动）
- **不改**列表的既有列结构（不新增列，只在现有 Model/Cost 列内换渲染）

---

## 3. 方案

### 3.1 后端：detail 响应新增**投影字段**，不直出 raw `results[]`

`GET /global/spend/logs/:call_id` 现在原样返回 `response`（含完整 `results[]`）。新增两个字段：

```jsonc
{
  // ...既有字段不动
  "response": { /* 原样，不裁剪 —— 保留"原始"标签页的完整性 */ },
  "search": {                        // ← 新增，仅 call_type=="search" 时非 null
    "provider": "searxng",
    "instance": "http://searxng-a:9099",
    "query": "aigw rust gateway",
    "query_count": 1,
    "result_count": 5,               // 与 results 长度可能不一致（见 3.4）
    "cost_per_query": 0.01,
    "parent_call_id": "req-001",
    "results": [                     // ≤ SEARCH_RESULTS_INLINE_MAX 条
      {"title": "...", "url": "https://...", "domain": "stackoverflow.com",
       "snippet": "...", "score": 2.2, "published_date": null}
    ]
  }
}
```

**为什么投影而不是让前端直读 `response.results[]`**：

1. **契约稳定** —— `response` 是上游结果的忠实副本，形状可能随 provider 变；投影层定义「界面吃什么」，两者解耦。
2. **单处设限** —— 结果数上限、摘要截断长度、URL 方案白名单（`http`/`https`）只在一个地方实现，前端不必各写一遍。
3. **可测** —— 投影是纯函数，UT 好写；前端只渲染。

**实现落点**：
- 新增 `crates/aigw-server/src/routes/search_view.rs`（或并入 `spend.rs`），导出纯函数
  ```rust
  pub struct SearchView { /* 上述字段 */ }
  pub fn project_search_view(log: &SpendLog) -> Option<SearchView>;
  ```
  从 `log.metadata` 取 `search_provider` / `search_query_count` / `cost_per_query` / `parent_call_id`；
  从 `log.messages` 取 `query`；从 `log.response` 取 `result_count` / `results[]`；
  `instance` 取 `log.api_base`。
- `global_spend_log_detail` 在组装 JSON 时**总是**带上 `"search"` 键，
  值为 `project_search_view(...)` 的结果 —— `call_type != "search"` 时为 `null`。
  （投影在组装端点算而非序列化层跳过：`json!` 宏里一个 `Option` 字段直接落成 `null`，
  要「非搜索行不含该键」得在 `json!` 外另开分支，收益为零 —— 前端 `log.search ?? null` 一视同仁。
  原设计的「仅搜索行存在该键」措辞由此订正为「非搜索行为 `null`」。）

**常量**：`SEARCH_RESULTS_INLINE_MAX = 20`、`SEARCH_SNIPPET_MAX_CHARS = 300`。

**防御**：任何一个来源字段缺失/类型不符 → 该字段 `null`，整块不消失（搜索行 metadata 可能为空，见 3.4）。

### 3.2 前端：列表 —— 搜索行换掉 Model 列内容

**Gate 2 决策（F1）**：列表 DTO 为搜索行**新增 `result_count`**。
代价近零 —— DB 层的列表 SELECT **本就包含 `response`**（`db.rs:9058`），
handler 里 `log.response` 已在内存，只需投影出这一个整数，**不改 SQL**。

新增 `SearchModelCell`（`pages/spend-logs/index.tsx` 内，与 `SearchSpend` / `SearchTokenHint` 并列）：

```
现在：  SearXNG/search                    ← 内部串，且结果数不可见
改后：  SearXNG · 3 条结果
        ⚙ 30.184.60.216:9099              ← 命中实例（来自列表既有的 api_base）
```

- 第一行：`{search_provider ?? custom_llm_provider} · {result_count} 条结果`；`result_count` 为 null 时只显 provider
- 第二行：命中实例 host:port（`api_base` 列表已有，**零后端成本**）；`api_base` 为 null 则不显示
- **列表不显示 query** —— 列表 DTO 不含 `messages`（H3 实测确认），query 留给抽屉
- 非搜索行的 Model 列**一字不改**

### 3.3 前端：抽屉搜索信息块（`data-testid="search-detail"`）

从「一行文本」升级为「指标格 + 键值行」：

```
🔍 搜索详情
┌───────────┬────────────┬──────────┬────────────┐
│ 供应商     │ 命中实例    │ 查询次数  │ 单价 / 次   │
│ SearXNG   │ :9099      │ 1        │ $0.0100    │
└───────────┴────────────┴──────────┴────────────┘
查询语句   say PONG
父调用     01a133ec-…  ↗
```

- 指标格沿用仓库既有 `rounded-md border px-4 py-2` 微卡片风格（同 `QualityDialog` 的 score/grade 块）
- 「命中实例」只显 host:port（完整 `api_base` 放 `title`），避免长 URL 撑破
- 既有断言 `data-testid="search-result-count"` / `search-query` / `parent-call-link` **保留**，保证 Stage 137 的 BDD 不返工

**Gate 2 决策（F2）：搜索行隐藏「输入」区，query 只在这一处出现。**

原设计只处理了 Output 侧，漏了 Input 侧 —— 而它**同样是空的**：搜索行的 `messages`
是 `{"query": "say PONG"}`，`InputCard` 走 `parseMessages()`，而它只认数组
（裸数组 / `record.messages`），**其余一律 `return empty`**（`MessageViewer.tsx:61`）。

改法：`isSearchRow(log)` 时**不渲染 Input 标签区**，只渲染本搜索详情块 ——
同一段 query 不在两处重复出现，也避免再维护一条 `{query}` 形状的渲染分支。

### 3.4 前端：抽屉输出区 —— **本 Stage 的核心**

当 `isSearchRow(log)` 且 `search` 块存在时，**不**走通用 `OutputCard`，改走新的
`SearchResultsPanel`，保留「视觉 / 原始」双标签（原始仍是完整 JSON，不丢数据）：

```
┌ 视觉 │ 原始 ┐
├───────────────────────────────────────────────┤
│ 命中结果 (5)                                   │
│ ┌───────────────────────────────────────────┐  │
│ │ ① Discord.js deleteCommand        [ 2.2 ] │  │
│ │   🔗 stackoverflow.com                    │  │
│ │   [javascript, bots, ping, discord]…      │  │
│ ├───────────────────────────────────────────┤  │
│ │ ② How to say pong in Russian      [ 1.3 ] │  │
│ └───────────────────────────────────────────┘  │
└───────────────────────────────────────────────┘
```

每条结果卡：
- **标题** —— 纯 `<a>`，仅当 `url` 通过 `http`/`https` 白名单（防 `javascript:` 等）才可点，`target="_blank" rel="noopener noreferrer"`
- **域名** —— 从 `url` 解析，小字 `text-muted-foreground`；解析失败则不显示
- **摘要** —— `snippet`，后端已截到 300 字，前端加 `line-clamp-3`
- **score** —— 小徽标（`Badge variant="outline"`），`score` 为空则不显示
- **published_date** —— **仅非 null 时显示**（实测 SearXNG 80% 为 null，见 Stage 134 §3.3.2）

**空态**：`results` 为空但有 `result_count > 0` → 显示「结果未随日志保留」；两者皆无 → 显示既有 `spendLogs.drawer.noResponseData`。

**计数不一致**：`result_count`（provider 自报）与 `results.len()`（实际留存）可能不同 ——
两者都给：标题写 `命中结果 (留存 N / 共 M)`，仅在不一致时加后缀。

### 3.5 手机端

列表卡片：搜索行在现有「Model」位置改显 `{provider} · {N} 条结果`（同 3.2）。
结果列表**不进卡片**，只留在抽屉 —— 避免一次搜索刷屏。

### 3.6 i18n（en / zh-CN，`spendLogs.search.*`）

| key | 中文 | 英文 |
|-----|------|------|
| `resultsTitle` | `命中结果` | `Results` |
| `keptOfTotal` | `（留存 {{kept}} / 共 {{total}}）` | `(kept {{kept}} of {{total}})` |
| `noResultsKept` | `结果未随日志保留` | `Results were not retained in the log` |
| `openResult` | `在新标签页打开` | `Open in a new tab` |
| `queryLabel` | `查询语句` | `Query` |
| `queryCountLabel` | `查询次数` | `Queries` |

设计原列的 `resultCountShort`（`{{count}} 条结果`）**未新增** —— 既有 `results` 已是同一文案，
再加一个同义 key 只会多一处要同步的翻译（奥卡姆剃刀）。列表与抽屉都复用 `results`。

既有 `queries` / `results` / `provider` / `instance` / `costPerQuery` / `zeroSpendHint` / `spendHint` 复用。

---

## 4. 测试

### 4.1 后端 UT（投影纯函数）

实际交付 **9 条**（规划 7 条）—— 多出的是设计评审期发现的边界：
`project_skips_results_without_a_title`（H1 的「单条结果缺 title 时跳过该条而非整块失败」）
与 `result_count_of_is_search_only`（列表 DTO 用的 `result_count_of` 也必须是 call_type 守卫）。

| UT | 断言 |
|----|------|
| `project_extracts_all_fields_from_a_search_row` | 六字段逐一取自正确来源 |
| `project_returns_none_for_non_search_rows` | `call_type="completion"` → `None` |
| `project_tolerates_empty_metadata` | metadata 为空 → 块仍在，字段为 null（对齐 `SEARCH_ROW_NO_META`） |
| `project_caps_results_and_snippet` | 50 条 → 20 条；600 字 → 300 字 |
| `project_drops_non_http_urls` | `javascript:` / `data:` → 该条 `url` 置 null（标题仍显示） |
| `project_reports_both_counts_when_they_disagree` | `result_count=5` + 2 条 results → 两者都在 |
| `project_skips_results_without_a_title` | 单条结果缺 title → 跳过该条，其余照常 |
| `result_count_of_is_search_only` | 非搜索行 → `None`（列表 DTO 不串键） |
| `project_survives_malformed_response` | `response` 为字符串/数组 → 不 panic，字段为 null |

### 4.2 前端 BDD（`spend-logs.feature` 追加 9 个场景，× 3 viewports）

- 搜索行列表显示「provider · N 条结果」而**不是** `searxng/search`
- 列表第二行显示命中实例 host:port
- 抽屉搜索块显示四个指标 + 查询语句
- 抽屉「视觉」标签渲染出**每条结果**的标题/域名/摘要
- 结果标题在 `url` 合法时是链接、非法时不渲染为链接
- `result_count` 与留存数不一致时两者都可见
- `results` 为空时显示空态文案
- 「原始」标签仍显示未裁剪 JSON
- 手机卡片显示结果数（不展开结果列表）

mock 侧：`api-mocks.ts` 的 `sampleDetailSearch` 现有 `result_count: 5` 但只有 2 条 `results` ——
**正好是 3.4 的「计数不一致」场景**，保留；另补一条 `javascript:` URL 的结果用于白名单断言。
detail mock 同时补投影块 `search`（与后端 `search_view.rs` 同形状），列表行补 `result_count`。

### 4.3 Gate 3 实现偏差（3 处）

实现过程中有 3 处偏离设计文本，均为实测驱动，逐条记录：

| # | 设计原文 | 实际实现 | 原因 |
|---|---------|---------|------|
| D1 | 空态「`results` 为空但有 `result_count > 0` → 结果未随日志保留」 | **仅当 `search` 块非 null（即后端判定为搜索行）时才渲染面板**；`search` 为 null 时走原 `OutputCard` 分支，不再合成空视图 | Gate 3 期间发现：列表行 DTO 不含 `search`，抽屉若在 `enrichedLog` 未合并 `search` 时自行拼一个空视图，会把「日志确实留存了结果」误报成「结果未保留」。宁可退化为旧渲染，也不谎报。同时修了 `enrichedLog` 漏传 `search` 的**真实产品 bug**（BDD 场景「kept 3 of 5」捕获） |
| D2 | 命中实例指标「只显 host:port，完整 `api_base` 放 `title`」 | 指标值显示**完整 `api_base`**，`title` 仍为完整值 | Stage 137 的既有 BDD 断言 `http://searxng-a:9099` 全文；截短会使其返工 —— 与 3.3「既有断言保留」的约束冲突，以既有断言为准 |
| D3 | 列表行徽标断言隐含在整行文本里 | `CallTypeBadge` 两个分支都加 `data-testid="call-type-badge"`，BDD 步骤收窄到该 testid 上大小写不敏感匹配 | 原断言 `toContainText("search")` 命中的是 Model 列里的内部串 `searxng/search` —— 本 Stage 把它换成 `searxng · 5 results` 后断言失去偶然来源。这是评审日志 F1 同类的**偶然通过**缺陷，借机修掉 |

---

### 4.4 Gate 证据

| 门禁命令 | 结果 |
|---------|------|
| `task test` | ✅ 1224 passed / 0 failed（含新增 9 条投影 UT） |
| `task bdd` | ✅ 315 scenarios（302 passed / 13 skipped）/ 1680 steps |
| `task fe-bdd` | ✅ 507 passed / 3 skipped（含新增 9 场景 × 3 viewports） |
| `task fmt` | ✅ |
| `task lint` | ✅（clippy `-D warnings`） |
| `task doctor` | ✅（`cargo check --workspace` + clippy） |
| `task fe-lint` | ✅（含 i18n 类型对称） |

---

## 5. 变更清单

| 层 | 文件 | 改动 |
|----|------|------|
| 后端 | `crates/aigw-server/src/routes/search_view.rs`（新） | `SearchView` + `project_search_view` 纯函数 + UT |
| 后端 | `crates/aigw-server/src/routes/spend.rs` | detail handler 插入 `"search"` 键 |
| 后端 | `crates/aigw-server/src/routes/mod.rs` | `pub mod search_view;` |
| 前端 | `crates/aigw-frontend/src/pages/spend-logs/SearchResultsPanel.tsx`（新） | 结果卡片列表 |
| 前端 | `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | `SearchModelCell` + `SearchDetailBlock` 重写 + 输出区分派 + types |
| 前端 | `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 6 个新 key（`resultsTitle` / `keptOfTotal` / `noResultsKept` / `openResult` / `queryLabel` / `queryCountLabel`） |
| 前端 | `crates/aigw-frontend/src/i18n/resources.d.ts` | 重新生成（`scripts/fe-i18n-types`） |
| 测试 | `crates/aigw-frontend/tests/features/spend-logs.feature` + `tests/steps/spend-logs.steps.ts` | 9 个新场景 + 徽标步骤收窄到 `call-type-badge` |
| 测试 | `crates/aigw-frontend/tests/steps/api-mocks.ts` | detail 补投影 `search` 块 + 一条非法 URL 结果；列表行补 `result_count` |

**不动**：`OutputCard.tsx`（通用组件，搜索走独立分支，避免为一个 nil-safe 形状污染它）、
`spend_logs` 表结构、Stage 136 的写入路径、Stage 137 的既有 testid。

---

## 5.5 回滚计划

**纯增量，无数据迁移 —— 回滚即 revert 这组 commit。**

- **后端**：`search` 是 detail 响应的**新增键**，既有键一个不动 → 前端不读它时零影响。回滚后响应恢复原样。
- **前端**：搜索行渲染从「显 `{provider}/search` + 空白输出」变成「显 provider·结果数 + 结果卡片」。回滚即恢复旧渲染。
- **无 DB 变更**：本 Stage **不新增表、不改列、不写迁移**（`response` 的写入方 Stage 136 不动）。故没有「回滚要退回数据」的问题。
- **无配置变更**：不碰 `config.yaml` / `web_search` 块。
- **回滚判定**：若结果卡片导致列表/抽屉性能问题（大结果集），可只 revert 前端 commit，保留后端投影（后者无害）。

---

## 6. 回归验证

实测结果（2026-10-16）：

1. `task test` — ✅ 1224 passed / 0 failed（含新增 9 条投影 UT）
2. `task bdd` — ✅ 315 scenarios（302 passed / 13 skipped）/ 1680 steps，零回归
3. `task fe-bdd` — ✅ 507 passed / 3 skipped（既有基线 + 9 场景 × 3 viewports）
4. `task fmt` / `task lint` / `task doctor` / `task fe-lint` — ✅ 全绿
5. **人工未做** — 未用 `aigw.db` 的真实搜索行（`01a133ec-…`）开抽屉肉眼核对；
   BDD mock 的 `sampleDetailSearch` 与后端 `search_view.rs` 同形状，已覆盖同一字段集。
   记为遗留（见 §8）。

## 7. 门禁

- [x] 9 个后端 UT 先红后绿（规划 7，实交付 9）
- [x] detail 响应新增 `search` 块；非搜索行为 `null`（见 §3.1 订正）
- [x] URL 白名单生效（`javascript:` 不出链接）
- [x] 结果数上限 20 / 摘要 300 字生效
- [x] 列表搜索行不再显示 `{provider}/search` 原始串
- [x] 抽屉「视觉」标签渲染每条结果；「原始」标签仍是完整 JSON
- [x] `result_count` 与留存数不一致时两者都显示
- [x] 手机端不刷屏
- [x] i18n en/zh-CN 对称；`fe-lint`（含类型对称）绿
- [x] `task test` / `bdd` / `fe-bdd` / `fmt` / `lint` / `doctor` / `fe-lint` 全绿
- [ ] **Gate 4 代码评审** —— 未做。本 Stage 的实现+门禁已完成，但独立代码评审（历次由 `/rdd-stage-auto` 或多路 subagent 承担）尚未执行
- [ ] git commit（精确 add；`--signoff`）—— 未提交

## 8. 风险与遗留

| 风险 | 缓解 |
|------|------|
| 投影层与 raw `response` 漂移（provider 改了形状，投影没跟） | 投影是纯函数 + 逐字段 UT；`response` 原样保留，「原始」标签永远是真值 |
| 结果里的 URL 是攻击面（渲染成可点链接） | `http`/`https` 白名单 + `rel="noopener noreferrer"`；非法方案不出链接 |
| `messages` 在列表接口不存在，列表拿不到 query | 设计上就只在抽屉显示 query（3.2），不做额外接口扩展 |
| 大结果集撑爆抽屉 | 后端 20 条上限 + 前端 `line-clamp` |

**遗留**（登记后续 Phase）：
- 按 provider / 实例聚合的花费与搜索量趋势（需新查询）
- 结果条目的点击预览 / 内联抓取
- 搜索行的 `session_id` 作为第二条父子线索在 UI 上暴露（TD-019 已接通后端）
- **未做真实库人工核对** —— 见 §6 第 5 条。当前证据全部来自 UT + BDD mock；
  若要坐实「真实搜索行的四块渲染」，需用 `aigw.db` 里的 `01a133ec-…` 手工过一遍

---

## 9. 对后续 Stage 的影响

| 影响 | 说明 |
|------|------|
| **建立了「投影块」先例** | detail 响应可为特定 `call_type` 附加投影键（本 Stage 的 `search`）。后续若要给 embedding / image 行做专属呈现，照此模式加键即可，不必让前端解析 raw blob |
| **`SearchResultsPanel` 是结果渲染的落点** | 将来「设计 A（短路）」若落地，它的响应里也会有服务端搜索结果块 —— 渲染层可复用本组件 |
| **URL 白名单是安全约定** | 后续任何「渲染上游返回的 URL 为链接」的改动都必须走同一白名单（`http`/`https` + `rel="noopener noreferrer"`） |
| **未解锁** | 按实例聚合花费（需新查询）、结果点击预览 —— 均登记为后续 Phase；本 Stage 的 `search` 投影块已把 `instance` 字段带出，是那些功能的现成输入 |
| **不阻塞任何 Stage** | Phase 53 已收官；本 Stage 与 Phase 53 的其它产出无反向依赖 |
