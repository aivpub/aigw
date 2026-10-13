# Stage 139: 搜索消费日志的结果可视化（Phase 54）

**所属**: Phase 54（搜索调用日志可视化）
**预估**: ~8h（后端投影 + 前端四块 + i18n + BDD）
**依赖**: Stage 137（`call_type="search"` 行与抽屉搜索块）、Stage 136（`response.results[]` 的写入方）
**状态**: 📋 设计待批（Gate 1）

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
  "search": {                        // ← 新增，仅 call_type=="search" 时存在
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
- `global_spend_log_detail` 在组装 JSON 时，若 `log.call_type == "search"` 则插入 `"search"` 键。

**常量**：`SEARCH_RESULTS_INLINE_MAX = 20`、`SEARCH_SNIPPET_MAX_CHARS = 300`。

**防御**：任何一个来源字段缺失/类型不符 → 该字段 `null`，整块不消失（搜索行 metadata 可能为空，见 3.4）。

### 3.2 前端：列表 —— 搜索行换掉 Model 列内容

新增 `SearchModelCell`（在 `pages/spend-logs/index.tsx` 内，与既有 `SearchSpend` / `SearchTokenHint` 并列）：

```
现在：  SearXNG/search
提议：  SearXNG · 3 条结果
        ↗ say PONG              ← 次行，truncate + title 全文
```

- 第一行：`{search_provider ?? custom_llm_provider} · {result_count} 条结果`
- 第二行：查询词（来自 `messages.query`；列表接口**不含** `messages`，故列表里查不到 query）
  → **列表只显示 provider + 结果数**；query 留给抽屉（避免为一个次行把 `messages` 拉进列表接口）
- `result_count` 未知时只显示 provider

非搜索行的 Model 列**一字不改**。

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
| `resultCountShort` | `{{count}} 条结果` | `{{count}} results` |
| `keptOfTotal` | `（留存 {{kept}} / 共 {{total}}）` | `(kept {{kept}} of {{total}})` |
| `resultsTitle` | `命中结果` | `Results` |
| `noResultsKept` | `结果未随日志保留` | `Results were not retained in the log` |
| `openResult` | `在新标签页打开` | `Open in a new tab` |

既有 `queries` / `results` / `provider` / `instance` / `costPerQuery` 复用。

---

## 4. 测试

### 4.1 后端 UT（投影纯函数）—— 先红后绿

| UT | 断言 |
|----|------|
| `project_extracts_all_fields_from_a_search_row` | 六字段逐一取自正确来源 |
| `project_returns_none_for_non_search_rows` | `call_type="completion"` → `None` |
| `project_tolerates_empty_metadata` | metadata 为空 → 块仍在，字段为 null（对齐 `SEARCH_ROW_NO_META`） |
| `project_caps_results_and_snippet` | 50 条 → 20 条；600 字 → 300 字 |
| `project_drops_non_http_urls` | `javascript:` / `data:` → 该条 `url` 置 null（标题仍显示） |
| `project_reports_both_counts_when_they_disagree` | `result_count=5` + 2 条 results → 两者都在 |
| `project_survives_malformed_response` | `response` 为字符串/数组 → 不 panic，字段为 null |

### 4.2 前端 BDD（`spend-logs.feature` 追加，× 3 viewports）

- 搜索行列表显示「provider · N 条结果」而**不是** `searxng/search`
- 抽屉搜索块显示四个指标 + 查询语句
- 抽屉「视觉」标签渲染出**每条结果**的标题/域名/摘要
- 结果标题在 `url` 合法时是链接、非法时不渲染为链接
- `result_count` 与留存数不一致时两者都可见
- `results` 为空时显示空态文案
- 「原始」标签仍显示未裁剪 JSON
- 手机卡片显示结果数（不展开结果列表）

mock 侧：`api-mocks.ts` 的 `sampleDetailSearch` 现有 `result_count: 5` 但只有 2 条 `results` ——
**正好是 3.4 的「计数不一致」场景**，保留；另补一条 `javascript:` URL 的结果用于白名单断言。

---

## 5. 变更清单

| 层 | 文件 | 改动 |
|----|------|------|
| 后端 | `crates/aigw-server/src/routes/search_view.rs`（新） | `SearchView` + `project_search_view` 纯函数 + UT |
| 后端 | `crates/aigw-server/src/routes/spend.rs` | detail handler 插入 `"search"` 键 |
| 后端 | `crates/aigw-server/src/routes/mod.rs` | `pub mod search_view;` |
| 前端 | `crates/aigw-frontend/src/pages/spend-logs/SearchResultsPanel.tsx`（新） | 结果卡片列表 |
| 前端 | `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | `SearchModelCell` + `SearchDetailBlock` 重写 + 输出区分派 + types |
| 前端 | `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 5 个新 key |
| 前端 | `crates/aigw-frontend/src/i18n/resources.d.ts` | 重新生成（`scripts/fe-i18n-types`） |
| 测试 | `crates/aigw-frontend/tests/features/spend-logs.feature` + `tests/steps/spend-logs.steps.ts` | 8 个新场景 |
| 测试 | `crates/aigw-frontend/tests/steps/api-mocks.ts` | 补一条非法 URL 的结果 |

**不动**：`OutputCard.tsx`（通用组件，搜索走独立分支，避免为一个 nil-safe 形状污染它）、
`spend_logs` 表结构、Stage 136 的写入路径、Stage 137 的既有 testid。

---

## 6. 回归验证

1. `task test` — 新增 7 个后端 UT
2. `task bdd` — 既有 315 场景不回归
3. `task fe-bdd` — 既有 480 passed 不回归 + 新增 8 × 3
4. `task fmt` / `task lint` / `task doctor` / `task fe-lint` / `task fe-build`
5. **人工**：用 `aigw.db` 里已有的真实搜索行（`01a133ec-…`，query `say PONG`，3 条结果）开抽屉核对四块

## 7. 门禁

- [ ] 7 个后端 UT 先红后绿
- [ ] detail 响应新增 `search` 块；非搜索行不含该键
- [ ] URL 白名单生效（`javascript:` 不出链接）
- [ ] 结果数上限 20 / 摘要 300 字生效
- [ ] 列表搜索行不再显示 `{provider}/search` 原始串
- [ ] 抽屉「视觉」标签渲染每条结果；「原始」标签仍是完整 JSON
- [ ] `result_count` 与留存数不一致时两者都显示
- [ ] 手机端不刷屏
- [ ] i18n en/zh-CN 对称；`fe-lint`（含类型对称）绿
- [ ] `task test` / `bdd` / `fe-bdd` / `fmt` / `lint` / `doctor` / `fe-lint` 全绿
- [ ] git commit（精确 add；`--signoff`）

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
