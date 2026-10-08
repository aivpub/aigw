# Stage 137 Review Log

**Review Type**: Design
**Review Date**: 2026-10-08
**Reviewer**: Claude (Opus, main) + 1× general-purpose subagent (adversarial)
**Stage**: Stage 137（搜索调用在管理控制台的展现与聚合口径修正）

## Review Summary

设计对仓库现状的测绘质量很高：**抽查的 12 处 `file:line` 锚点全部准确**（`index.tsx:224` / `:254` / `:269` / `:1413-1454`，`usage/index.tsx:256` / `:778-785`，`spend.rs:38-52` / `:237` / `:248` / `:661`，`db.rs:8116` / `:3867`）。四处「核心决策」经代码验证成立：

- `fmtTokens(0)` 确实走 `v.toString()` 返回 `"0"`（`lib/format.ts:7-13`）→ 搜索行当前渲染成 `0 / 0`
- `logsData?.count` 确实是本页条数（`spend.rs:341` 的 `"count": data.len()`），而 `total_count` 是总数（`:757`）→ §4.3 #13 的「既有 bug」判定正确
- `spendLogs.drawer.metadata` i18n 键存在（`en.json:716`）且**源码零引用** → 可直接复用
- 可复用 step `the spend log row with call id {string} should show the {string} type badge` 存在且签名一致（`spend-logs.steps.ts:342`）
- §4.3 #7 的「前端会重排，SQL 的 `ORDER BY total_tokens` 不影响 Top 5」论断成立（`usage/index.tsx:256` 默认 `"spend"`，`:778` 的 `.sort()` 在 `:785` 的 `.slice(0, 5)` **之前**）

**发现 2 项 High，均须先改设计** —— 两者都集中在同一处：**新增的 `call_type` / `parent_call_id` 过滤只改了列表查询，没同步计数查询**，会让筛选后的分页条直接显示错误总数；且设计内部对「子调用徽标的 N 从哪来」自相矛盾。

## Findings

### F1 — 新增过滤不同步到 `query_spend_logs_count` → 筛选后分页条显示全量条数（High）

§3.4 只改 `query_spend_logs_with_status_filter`（11 参数），并明确「`query_spend_logs_count` 本期不改」。但两个查询的**参数集本就不一致**：

| 函数 | 参数 | 锚点 |
|------|------|------|
| `query_spend_logs_with_status_filter` | `api_key` / `model` / `provider` / `start_date` / `end_date` / `call_id` / `status` / `min_tokens` / `max_tokens` / `limit` / `offset`（11） | `db.rs:8116-8128` |
| `query_spend_logs_count` | `api_key` / `model` / `start_date` / `end_date` / `call_id`（**5**） | `db.rs:3867-3874` |

handler 侧 `tokio::try_join!` 并发取两者（`spend.rs:247-262` / `:660-680`），`total_pages` 由 `total_count` 算出（`:276-277` / `:689-690`）。

**失败场景**：用户选 `call_type=search` → 列表只剩 N 条搜索行（正确），但 `total_count` 仍是**全量**（未过滤）→ 分页条显示「共 500 条 / 17 页」，用户翻到第 2 页得到空列表。`parent_call_id` 过滤（父→子跳转）同理。

**这不是「既有小缺陷被曝光」，而是本 Stage 新引入的用户可见错误**：此前 5 参 vs 11 参的差距体现在 `status` / `min_tokens` / `max_tokens` 三个筛选上（确实早已存在），但本 Stage 新增的两个筛选**全部**落在这个缺失集里，而 §3.6 又把它们做成了主要交互入口（筛选下拉 + 跳转徽标）。

**处理**：§3.4 补一行 —— `query_spend_logs_count` 同步加 `call_type` / `parent_call_id` 两参数（三方言 × 3 处实现：`db.rs:2669` / `:3118` / `:3532`），与列表查询**同步过滤**。若判定本期不做，则**必须在 §3.6 明确禁止**「筛选时展示分页条」并给出替代（如筛选态隐藏分页、显示「已过滤」徽标）—— 不能两个都不做。

### F2 — 「子调用徽标的 N」来源自相矛盾（High）

同一份设计的两节给出**两个不同的来源**：

| 位置 | 原文 |
|------|------|
| §3.3 表（父→子行） | 「徽标上的 N 由**同一次过滤请求的 `total_count`** 得出（不猜不算）」 |
| §3.4 末 | 「父→子徽标的 N 改用该过滤请求返回的 **`data.length`**」 |

两者互斥：用 `total_count` 就**必须**改 `query_spend_logs_count`（与 §3.4 的「本期不改」冲突）；用 `data.length` 则 N 受 `page_size` 截断（默认 30）。

**处理**：二选一写死。建议取 `data.length`（与 F1 的「count 不改」一致），并补一句约束：`N = min(实际子行数, page_size)`，设计 C 下恒为 1，因此该截断**当期不可观测**；但若未来引入多轮搜索（设计 B），此约束必须重审。同时在徽标 tooltip 注明该数是「本次返回的条数」。

### F3 — `spend === 0` ⟺ 「单价被显式置 0」不严谨（Medium）

§3.2a 的判别式：`isSearchRow(log) && log.spend === 0` ⟹ 「该部署把 `cost_per_query` 显式置成了 0」。但 `calc_search_spend`（`chat.rs:131-137`）有**四条**归零路径：

```rust
if queries <= 0 || !unit.is_finite() || unit <= 0.0 { return 0.0; }
```

`unit` 为 `NaN` / `±inf` / **负数**时同样归零 —— 这些是**脏配置**，不是「主动置 0」。UI 会把它们一并渲染成 `$0.00 未计价`（文案：「单位成本被配置为 0」），即**错误归因**。Stage 136 §8.1 自己把「配置方填了脏单价」列为已知风险。

**处理**：§3.2a 的措辞从「⟺ 单价为 0」放宽为「`spend === 0` 意味着**本次搜索的记录金额为 0**」，`未计价` 徽标文案保持中性（不声称配置）—— 或直接在 §8.2 登记「脏单价与 0 单价在 UI 上不可区分」，本期接受。**不要**在 UI 上写「配置为 0」这种断言式文案。

### F4 — 列表视图展示缺省 `$0.01` 而**无任何提示**，与 §3.2a 自己的约束张力（Medium）

§3.2a 末尾明确：「**不得暗示金额已校准**：缺省 `0.01` 是 OpenAI 对外牌价占位，通常比自建摊销成本高约 50 倍」。但按 §3.2a 的表格，`spend > 0` 时：

- **列表** spend 单元格 = `fmtSpend(log.spend)`，**无 badge、无 tooltip**
- 只有**抽屉**的搜索详情块才展示 `cost_per_query` 单价快照

即：列表上会出现一整屏 `$0.01`，且**没有任何信号**提示该数字是牌价而非真实成本。这与「不得暗示已校准」的自我约束直接冲突 —— 恰恰是列表（最常看的视图）在暗示。

**处理**：二选一，写进 §3.2a：(a) 列表的 spend 单元格也挂一个极轻的 tooltip（复用 §3.2 的 token tooltip 机制，文案指向单价快照）；(b) 明确接受该张力并写明理由（「列表只报金额、不报来源；单价快照必须在抽屉查看」）。

### F5 — SQLite 的 `metadata` 是 **BLOB**，是唯一能存非法 JSON 的方言（Low）

三方言列类型不一致（`002_spend_logs.sql:23`）：SQLite `BLOB` / MySQL `JSON` / PostgreSQL `JSONB`。

实测三种条件（本地容器）：

| 方言 | 条件 | 合法 JSON | `metadata IS NULL` | 非法 JSON |
|------|------|-----------|-------------------|-----------|
| SQLite | `json_extract(metadata,'$.parent_call_id')` | ✅ 返回 `p1`，`= 'p1'` 为真 | ✅ 返回 `NULL`，不匹配 | ⚠️ **`malformed JSON` 报错** |
| MySQL | `JSON_UNQUOTE(JSON_EXTRACT(m,'$.k'))` | ✅ | ✅ `IS NULL` 为真 | 不可达（列类型强制） |
| PG | `metadata->>'parent_call_id'` | ✅ | ✅ `IS NULL` 为真 | 不可达（列类型强制） |

**→ 设计的三方言 SQL 表本身正确**（SQLite 的 `json_extract` 对 BLOB 里的合法 JSON 正常工作，无需 `CAST`）。唯一风险面是「SQLite + 非合法 JSON 值」，而 MySQL/PG 因列类型**结构上不可能**出现该情况 —— 设计未指出这一点，容易让人以为三方言风险对称。

**处理**：§3.4 的三方言表补一列「非法 JSON 行为」，并说明：生产写入路径（`insert_spend_log`）绑定的是 `Option<Value>`，序列化后必为合法 JSON，故该风险实际不可达；但**测试 fixture 若手工塞裸字符串**会炸 —— UT 的 `SEARCH_ROW_NO_META` 后端对偶（`metadata = NULL`）已覆盖合法边界，另需一条「metadata 为非 JSON 文本」的容错断言（仅 SQLite）或明确记为不可达。

### F6 — 场景计数笔误（Low）

§4.2 表格实列 **19 行**（1 / 2 / 3 / 4 / 4a / 4b / 4c / 5 / 5a / 5b / 6 / 7 / 8 / 9 / 10 / 11 / 12 / 13 / 14），正文写「20 条 × 3 viewport = **60 个 playwright 用例**」。实际 19 × 3 = 57。门禁第 2 条也跟着写了 60。

**处理**：改正文与门禁为 19 条 / 57 用例，或补足场景数。

### F7 — 复核通过：§4.3 #7 的「前端重排」论断成立（无需改）

子 agent 提出「SQL 的 `ORDER BY total_tokens DESC` 会不会让搜索组进不了 Top 5」——**不成立**：`usage/index.tsx:778` 的 `.sort()` 按 `globalChartMode` 重排，:785 才 `.slice(0, 5)`，且 `globalChartMode` 默认 `"spend"`（`:256`）。全量数据未在 SQL 侧截断，故按 spend 时搜索组可正常进 Top 5。设计 §4.3 #7 的判断准确。

## AI Pre-Filter Results

- 子 agent 的「`parent_call_id` 的 JSON 条件在 SQLite 上必须 `CAST(metadata AS TEXT)`」——**过滤（不成立）**：实测 `json_extract(BLOB,…)` 对合法 JSON 正常工作（SQLite 的 JSON1 接受 TEXT 或 BLOB）。
- 「§3.4 漏了两个 caller」——**过滤（不成立）**：全仓仅 2 处 caller（`spend.rs:248` / `:661`），且都属 §5 明列的 `spend_logs`（`:237`）与 `global_spend_logs`（`:662`）两个 handler。
- 「新增字段会漏改 `SpendLogsQuery` 的 `#[allow(dead_code)]`」——**过滤（不适用）**：该属性只是既有习惯，加字段不影响。

## Rule Filtering

- **Scope creep**：§4.3 #13 修 Dashboard「Total Requests」是**顺带修既有 bug**（已确认为真 bug：`count` 是页内条数）。该改动超出「搜索展现」的标题范围，但 §1 边界与 §5 变更清单均已列明，属**显式扩权**而非静默扩张 —— 接受，登记进 ADR。
- **Memory bias**：F1/F2 属「把列表查询的改动当成完整改动」，未追查并行的计数查询与文档内部一致性。
- **Logical fallacy**：§3.7「不改聚合 SQL」的论证（改了会让 `SUM(spend)` 少算搜索费、破坏对账等式）**成立且有力**，是本次设计最强的一处判断。

## Resolution Summary

| # | 严重度 | 处置 |
|---|-------|------|
| F1 | High | **改设计**：§3.4 增 `query_spend_logs_count` 同步加两参数（或 §3.6 禁止筛选态分页条） |
| F2 | High | **改设计**：统一徽标 N 的来源（取 `data.length`），删去 §3.3 的 `total_count` 说法 |
| F3 | Medium | **改设计**：§3.2a 措辞放宽为「本次金额为 0」，文案去掉「配置为 0」断言 |
| F4 | Medium | **改设计**：§3.2a 补列表视图的单价可见性处置（tooltip 或显式接受） |
| F5 | Low | **改设计**：§3.4 三方言表补「非法 JSON 行为」列 + 可达性说明 |
| F6 | Low | **改设计**：场景数 19 / 用例 57 |
| F7 | — | 复核通过，无需改 |

**All Critical Fixed**: 是（无 Critical）
**All High Priority Addressed**: 是（F1/F2 已改设计）

## 编码期必查项（Design Review 后置）

1. F1 的处置若取「同步改 count」，须验证三方言 `query_spend_logs_count` 实现对 `parent_call_id` 的 JSON 条件与列表查询**逐字一致**（两处 SQL 分叉是本 Stage 最大的 DRY 风险）
2. F2 的 N 取值须由 BDD 场景 8/9 的真实请求捕获（断言 URL 含 `parent_call_id=` 且徽标文本符合选定语义）
3. F5 的「非 JSON 文本 metadata」在 SQLite 上的行为须有明确结论（UT 断言容错，或记为不可达并说明依据）

---

## 编码期发现的额外缺陷（Gate 3，TDD 红绿期间）

### X1 — MySQL 上仅做引号翻倍的转义可被反斜杠绕过（Critical，已修）

**发现方式**：Gate 3 阶段核实「§3.4 的单引号转义是否充分」时，实测 MySQL 默认 `sql_mode`（`ONLY_FULL_GROUP_BY,STRICT_TRANS_TABLES,…`，**无 `NO_BACKSLASH_ESCAPES`**）下反斜杠是字符串转义字符。

**复现**（本地 `aigw-mysql-1`）：

```sql
-- 输入 parent_call_id =  x + \ + ' + " OR 1=1 --"   （即 x\' OR 1=1 --）
-- 引号翻倍后成为：  x\'' OR 1=1 --
SELECT COUNT(*) FROM t
 WHERE JSON_UNQUOTE(JSON_EXTRACT(m,'$.parent_call_id')) = 'x\'' OR 1=1 -- ';
-- → 3   （返回全部行，注入成立）
```

该载荷中的反斜杠把紧随的引号转义掉，**翻倍后的第二个引号反而闭合了字面量**，`OR 1=1` 逃逸成功。

**影响面**：`call_type` 与 `parent_call_id` 两个**本 Stage 新增**的筛选参数都走 `format!` 拼接（沿用既有 `model` / `api_key` 分支的写法），且二者由客户端直接控制 → 本 Stage 把既有模式的可利用面**从「管理员自填的 model 名」扩大到「任意用户可传的筛选参数」**。

**修复**：新增模块级 `sql_literal(db, raw)`：

```rust
fn sql_literal(db: &Database, raw: &str) -> String {
    let quotes = raw.replace('\'', "''");
    match db {
        Database::Mysql(_) => quotes.replace('\\', "\\\\"),
        _ => quotes,
    }
}
```

—— MySQL 加反斜杠翻倍，**SQLite/PG 不动**（PG 的 `standard_conforming_strings=on` 与 SQLite 都把反斜杠当普通字符，逃逸它反而会改坏值）。筛选方法经它转义；`query_spend_logs_count` 在 `MySqlPool` 的 impl 块内联同样的双重替换。

**验证**：三驱动 real BDD 61/61 × 3 全绿；UT `global_spend_logs_parent_call_id_rejects_quote_injection` 覆盖引号注入。

**遗留（登记 §8.2）**：既有分支（`model` / `provider` / `api_key` 的等值条件）**仍只有引号翻倍**，未走新 escaper。修正它们需要逐条评估值域（管理员填写 vs 客户端可控），超出本 Stage 范围 —— 但这是同一类洞，须登记。
