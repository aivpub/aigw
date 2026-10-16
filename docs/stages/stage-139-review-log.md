# Stage 139 Review Log

**Review Type**: Design
**Review Date**: 2026-10-13
**Reviewer**: Claude (main) —— 逐项对照代码/实测核实（未采信设计文档的自述锚点）
**Stage**: Stage 139（搜索消费日志的结果可视化，Phase 54）

---

## Review Summary

设计主体成立：三处「存了没画」的洞经实测逐一确认（抽屉输出区确为空白、列表确显内部串、
抽屉信息块确为一行文本），投影纯函数的落点、边界、UT/BDD 计划可执行。

但设计里有 **1 处我写错的事实断言（High）** 与 **1 处整块遗漏（Medium）**，均在编码前修正。

**结论：需改设计**（F1 必修、F2 必补），改后放行 Gate 3。

---

## Findings

### F1 — 设计的列表方案建立在**错误事实**上：列表里连 `result_count` 都没有（High）

设计 §3.2 写「列表搜索行显 `{provider} · N 条结果`」，并在 H3 断言「列表接口不含 `messages`」。
**该断言只对了一半，而漏掉的那一半正好推翻方案。**

实测（`GET /global/spend/logs?call_type=search`，真实数据）：

```
返回 29 个键：api_base api_key call_id call_type custom_llm_provider metadata model …
含 messages: False      ← H3 说对了
含 response: False      ← ★ 设计没提，而 result_count 只存在 response 里
```

**即：列表里取不到结果数**，`{provider} · N 条结果` 的 `N` 无从取值。落库的
`response` 只在抽屉的 detail 接口里返回。

**进一步核实（这次是决定走哪条路的关键）**：DB 层的列表查询
（`db.rs:9058` 的 `query_spend_logs_with_status_filter`）**SELECT 列表里含 `response`**
（`end_user, requester_ip_address, messages, response,`）—— 即 handler 拿到手时
`log.response` **已在内存**，只是组装 DTO 时没往外放。

**处理（决策：两头都要）**：

1. **列表 DTO 为搜索行补 `result_count`** —— 从已在内存的 `log.response` 投影出这一个整数。
   **零 SQL 改动**（该列本就 SELECT 了）。用小写 `result_count`，与既有 `total_tokens` 等标量键平行，
   形状扁平、不改既有键名。
2. **列表第二行加「命中实例」** —— `api_base` 列表已有，**零后端成本**，且补上原设计漏掉的第三个可用字段。

**为什么不选「退化为 `{N} 次查询}`」**：`search_query_count` 是**发起了几次搜索**，
不是**搜到几条结果** —— 前者恒为 1（本期一次搜索一行），做成列表主信息等于没信息。

### F2 — 设计只做了 Output 侧，**Input 侧同样是空的**（Medium）

搜索行的 `messages` 是 `{"query": "say PONG"}`。抽屉「输入」标签页把它交给 `InputCard`
→ `parseMessages()`（`MessageViewer.tsx:33-66`），而该函数**只认数组**（裸数组 / `record.messages` 数组），
**其余一律 `return empty`**（`:61`）。

所以抽屉的「输入」区对搜索行**也是空白的** —— 与 Output 侧同一个病，设计却只治了 Output。

**处理**：`isSearchRow(log)` 时**不渲染 Input 标签区**，只渲染搜索详情块（§3.3）。
理由：同一段 query 不在两处出现；也避免再维护一条 `{query}` 形状的渲染分支（H6）。

### F3 — 列表的两个可用增强被漏掉（Low，已并入 F1 处理）

列表行已有 `api_base` 与 `metadata`，原设计只用了 metadata 里的 provider，
**没用 `api_base`**（命中实例）—— 本轮并入 §3.2 第二行。

另核对：搜索行的 `cache_hit` 为 null（搜索不走 exact-match 缓存），无需处理。

---

## 假设核实（H1–H6）

| # | 假设 | 结论 | 证据 |
|---|------|------|------|
| H1 | `response.results[]` 形状一致 | ✅ | 真实行 `01a133ec-…`：`{title,url,snippet,score,published_date}` 齐全 |
| H2 | `result_count` 与留存数会不一致 | ✅ | `api-mocks.ts` 的 `sampleDetailSearch` = 5 vs 2 |
| H3 | ~~列表不含 `messages`~~ → **改为：DTO 无 `response` 但 DB 已 SELECT** | ✅（修正后） | 实测 DTO 29 键；`db.rs:9058` SELECT 含 `response` |
| H4 | 不改 `OutputCard` 可行 | ✅ | `ParsedOutput` 三形状（choices/output/data）与搜索无重叠 |
| H5 | 投影可为纯函数 | ✅ | 数据全来自 `&SpendLog`，无 IO |
| H6 | 抽屉 Input 区对搜索行本就空白 | ✅ | `MessageViewer.tsx:61` 的 `return empty` 兜底 |

---

## AI Pre-Filter Results

无 AI 生成的外部 findings（本轮为自评审）。以下为**自查后主动否决**的候选方案：

| 候选 | 否决理由 |
|------|---------|
| 列表 SQL 加 `response` 列 | **不必要** —— `db.rs:9058` 已 SELECT 了 `response`（原以为要改 SQL，核实后发现不用） |
| 前端直读 `response.results[]`（不做投影） | 形状随 provider 变、限额/截断/白名单要在前端重写一遍、难单测 |
| 列表显 `{N} 次查询` 代替结果数 | 本期 `search_query_count` 恒为 1，等于无信息 |
| 抽屉 Input 区为搜索行加 `{query}` 渲染分支 | 与搜索详情块重复展示同一段 query；隐藏更省 |
| 复用 `OutputCard` 并为其加搜索分支 | 污染三个 surface 共用的通用组件 |

---

## Resolution Summary

| # | 严重度 | 处置 | 落点 |
|---|--------|------|------|
| F1 | High | **改设计**：H3 修正 + §3.2 改为「列表 DTO 补 `result_count`（零 SQL 改动）+ 第二行显命中实例」 | `stage-139.md` §1.5 H3 / §3.2 |
| F2 | Medium | **补设计**：§3.3 增加「搜索行隐藏 Input 区」+ 新增假设 H6 | `stage-139.md` §1.5 H6 / §3.3 |
| F3 | Low | 并入 F1（`api_base` 进列表第二行） | `stage-139.md` §3.2 |

**编码期必查项（Design Review 后置）**：

1. **F1**：列表 DTO 的 `result_count` 必须与抽屉 `search.result_count` **同源**
   （都从 `response.result_count` 取），否则列表与抽屉会显示不同的数。
2. **F2**：隐藏 Input 区后，既有 BDD 场景「搜索行 drawer 无 token」等断言不能回归 ——
   跑 `task fe-bdd` 全量确认。
3. **H1**：投影对 `title`/`url`/`snippet` 缺失的单条结果要**跳过该条**而非整块失败
   （`snippet` 实测 2/35 为空，见 Stage 134 §3.3.2）。
4. **URL 白名单**：`javascript:` / `data:` 必须不出链接 —— UT 覆盖。
5. **既有 testid 保真**：`search-result-count` / `search-query` / `parent-call-link` 一个都不能改。

---

## Gate 3 交割：编码期必查项逐条核对（2026-10-16）

| # | 必查项 | 结论 | 证据 |
|---|--------|------|------|
| 1 | 列表 `result_count` 与抽屉 `search.result_count` 同源 | ✅ | 两者都读 `response.result_count`；列表走 `result_count_of(log)`（带 call_type 守卫，UT `result_count_of_is_search_only`），抽屉走 `project_search_view`。**新增 1 条 UT 守住守卫**（原设计未列） |
| 2 | 隐藏 Input 区后既有 BDD 不回归 | ✅ | `task fe-bdd` **507 passed / 3 skipped**，0 回归 |
| 3 | 单条结果缺 `title`/`url`/`snippet` 时跳过该条而非整块失败 | ✅ | UT `project_skips_results_without_a_title`（**原设计 7 条 UT 中无此条，新增**） |
| 4 | `javascript:` / `data:` 不出链接 | ✅ | UT `project_drops_non_http_urls`（`url` 置 null、标题仍显）+ BDD 场景 × 3 viewport |
| 5 | 既有 testid 保真 | ✅ | `search-result-count` / `search-query` / `parent-call-link` / `search-detail` / `search-zero-note` 全部原文保留（`index.tsx:1135/1170/1177/1187/1203`） |

**必查项 1 的一个真实缺陷在 BDD 中被捕获**：`enrichedLog` 起初只从 detail 响应合并了 `messages` 与 `response`，
漏了 `search` —— 抽屉因此在 `result_count=5` 却读不到 `results[]` 时自拼了一个空视图，把「留存了结果」渲染成
「结果未随日志保留」。这是**产品 bug 而非测试伪影**，场景「Search drawer shows both the reported and the retained count」
（断言 `kept 3 of 5`）将其暴露。修法：`enrichedLog` 增加 `search: detailData.search`，**并删掉自拼空视图的兜底分支**
（见 `stage-139.md` §4.3 D1）。

**两条既有断言的偶然通过被顺带修掉**（评审 F1 同类风险，本 Stage 换掉内部串后暴露）：

- 类型徽标断言 `toContainText("search")` 命中的是 Model 列里的 `searxng/search`，不是徽标本身 ——
  Model 列改为 `searxng · 5 results` 后断言失去偶然来源。已加 `data-testid="call-type-badge"` 并收窄步骤。
- 命中实例指标若按设计截短为 host:port，会打掉 Stage 137 的 `http://searxng-a:9099` 全文断言 ——
  以既有断言为准，保留完整 `api_base`（`title` 同值）。

---

## 真实库复核（2026-10-16）—— 推翻了 F1 修正所依赖的一条假设

用 `sqlite3 "file:./aigw.db?mode=ro"` 核对 12 条真实 `call_type="search"` 行（36 条结果）。

**对本评审 F1 的直接冲击**：F1 的修正是「列表 DTO 补 `result_count`，与抽屉同源」，
其动因是「结果数在列表不可见」。但设计据此进一步推出的 H2
（**`result_count` 与 `results.len()` 会不一致**，故需 `keptOfTotal` 后缀）
**在真实库里不成立 —— 12/12 全部相等**。

原 H2 的证据只有 `api-mocks.ts` 的人造 5 vs 2 fixture，属**自证**（用自己造的用例
证明真实数据会有该特征）。真实 SearXNG 在结果数 ≤ `max_results` 时不会出现
「自报多于留存」。

**处置**：防御代码保留（换 provider 后仍可能不一致，且前缀仅在不等时出现，零成本），
但 §4.3/§3.4 的措辞由「已知会发生」改为「已实现的防御」，
该分支改由 mock **刻意构造**并标明「真实库未触发，仅 fixture 覆盖」。
`stage-139.md` §1.5 H2 / §3.4 / §6.1 已同步。
