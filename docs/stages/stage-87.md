# Stage 87: Spend Logs UI call_id/request_id 区分 + 双列模糊搜索

**Phase**: 34 — 售后对账链路收尾
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 5h
**前置**: Stage 85（spend_logs 同时有 `call_id` PK + 可空 `request_id`，前端 3 interface + 展示列 + CSV + 搜索已落地）；`docs/request-id-backfill-sop.md`（历史行回填 SQL SOP，与本 Stage 解耦）

---

## 核心预期

1. **列表列顺序**：Spend Logs 列表 `call_id` 放最左侧（日期列之前），便于一眼定位。
2. **抽屉双 id**：条目展开的抽屉同时显示 `call_id` 和 `request_id`，显著标志区分（网关调用 id vs 上游 provider id）。
3. **模糊搜索**：Spend Logs 筛选框可同时按 `call_id` 或 `request_id` 模糊匹配（输半段 id 能搜到）。

## 背景

Stage 85 已在前端落地了 `call_id` + `request_id` 的展示与搜索，但有三个实测缺口：

| 用户诉求 | 现状 | 缺口 |
|----------|------|------|
| (A) `call_id` 列表最左列 | `call_id` 是第 8 列（Time/Type/Model/Key/EndUser/IP/Status 之后），见 `index.tsx:639/669` | 要移到 `Time`（:632）之前 |
| (B) 抽屉显示双 id | 抽屉 header 只显示 `call_id`（`index.tsx:349-352`），`request_id` 未渲染 | 要加 `request_id` 且显著区分 |
| (C) 模糊搜索 | 搜索 `?request_id=` 后端走 `call_id = ? OR request_id = ?` 精确等值（db.rs:1521/1551/1828/1854/4243-4244） | 要改 `LIKE '%X%'` 模糊匹配 |

## ① 前端：列重排（crates/aigw-frontend/src/pages/spend-logs/index.tsx）

把 `Call ID` 列从当前位置移到最左（`Time` 之前）：

- **表头**：`<TableHead>` 的 `Call ID`（:639）从第 8 位移到第 1 位（:632 之前）。`Upstream ID`（:640）保持在 `Call ID` 右侧。
- **表体**：对应 `<TableCell>` 的 call_id（:669）/ request_id（:670）随之移到最左两列。
- **移动端 card 布局**（:680-707）：call_id 已在 :702，request_id 未渲染（:703 只有 call_id）。保持现状（移动端空间有限，只显示 call_id；抽屉里再看双 id）。CSV 顺序（:120）已是 `Call ID` 开头，不动。

新顺序：`Call ID | Upstream ID | Time | Type | Model | Key | End User | IP | Status | TTFT | Duration | Tokens | Cost`。

## ② 前端：抽屉双 id 显著区分（index.tsx:349-352）

`SheetDescription` 现在只显示 `call_id`。改为显示双 id，用 Badge + 标签显著区分：

- **`call_id` 行**：`<Badge variant="default">Call ID</Badge>` + `<code>{log.call_id}</code>` + 复制按钮。
- **`request_id` 行**：`<Badge variant="secondary">Upstream ID</Badge>` + `<code>{log.request_id ?? "—"}</code>` + 复制按钮（NULL 时灰色 `—`，无复制）。
- 颜色：default（深色）vs secondary（浅灰）视觉区分；标签文字「Call ID」/「Upstream ID」语义区分。
- 布局：两个 Badge + code 横排，`flex flex-wrap items-center gap-2`；request_id 为 NULL 时整行灰色提示「无上游 ID（失败/历史行）」。

## ③ 后端：双列模糊搜索（crates/aigw-core/src/db.rs，5 处 `=` → `LIKE`）

把 `call_id = ? OR request_id = ?` 精确匹配改为 `call_id LIKE ? OR request_id LIKE ?` 模糊匹配，bind `%{rid}%`：

| # | 函数 | 位置 | 现状 | 改为 |
|---|------|------|------|------|
| 1 | SQLite `query_spend_logs` | :1521 | `AND (call_id = ? OR request_id = ?)` | `AND (call_id LIKE ? OR request_id LIKE ?)`，bind `%{rid}%`（:1530 两处） |
| 2 | SQLite `query_spend_logs_count` | :1551 | 同上 | 同上（:1554+ 两处 bind） |
| 3 | PG `query_spend_logs` 内存过滤 | :1828 | `log.call_id != rid && log.request_id.as_deref() != Some(rid)` | `!log.call_id.contains(rid) && !log.request_id.map_or(false,\|r\| r.contains(rid))` |
| 4 | PG `query_spend_logs_count` | :1854 | `AND (call_id = ? OR request_id = ?)` | `AND (call_id LIKE ? OR request_id LIKE ?)`，bind `%{rid}%`（:1861 两处） |
| 5 | PG `query_spend_logs_with_status_filter` | :4243-4244 | `(call_id = '{}' OR request_id = '{}')` 字符串拼接 | `(call_id LIKE '%{}%' OR request_id LIKE '%{}%' ESCAPE '\')`，esc 需转义 `%`/`_` 通配符防注入 |

**LIKE 通配符转义**（防注入）：
- bind 参数方式（#1/#2/#4）：`%`/`_` 是用户输入的合法字符，sqlx bind 不转义 LIKE 通配符。但用户输半段 id（如 `req-00`）不含 `%`/`_`，匹配正常；若含，则需 `rid.replace('%', "\\%").replace('_', "\\_")` + `ESCAPE '\'`。保守起见统一加转义。
- 字符串拼接方式（#5）：esc 已做单引号 `''` 转义，但要加 LIKE 通配符转义 `esc.replace('%', "\\%").replace('_', "\\_")` + SQL 末尾 `ESCAPE '\'` 子句。

**MySQL 路径**：走 `query_spend_logs`（:2088 区域）内存过滤，同 #3 PG 内存路径改 `contains`。

**一致性**：5 处必须同时改，否则列表（query）与计数（count）不匹配，分页错乱。

## ④ BDD（crates/aigw-frontend/tests/）

### 新增 3 场景（features/spend-logs.feature）

1. **call_id 是最左列**：断言表头第一个 `<th>` 文本是 "Call ID"。
2. **抽屉显示双 id**：点击行后，抽屉同时出现 "Call ID" 和 "Upstream ID" 标签；mock 数据 `req-001` 的 `request_id: "chatcmpl-abc123"`（api-mocks.ts:27）可见。
3. **模糊搜索**：输入 `req-00`（call_id 前缀）→ 列表更新；输入 `chatcmpl`（request_id 子串）→ 列表也更新。

### mock 增强（steps/api-mocks.ts:195-197）

现状：`GET /global/spend/logs` mock 忽略 query param，总返回全部 sample logs。搜索场景无法真正验证过滤。改为：读取 `?request_id=` query param，对 `sampleSpendLogs` 按 `call_id.includes(q) || request_id.includes(q)` 过滤后返回（与后端 LIKE 语义一致）。

### steps（steps/spend-logs.steps.ts）

复用现有 `When("I type {string} into the call ID search", ...)`（:84-88）。新增断言：列表行数变化或网络请求 URL 含 `request_id=req-00`。

### real BDD

复用 `bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` task，验证后端 LIKE 三方言一致。现有 Stage 85 的「双列返回 + 双列搜索」2 场景改模糊后重跑应仍绿（精确值是模糊的子集）。

## TDD 红绿

- 后端单测（aigw-core）：新增 UT 验证 LIKE 改动——seed 3 行（call_id=`req-001`/`req-002`/`req-003`，request_id=`chatcmpl-abc`/`msg_xyz`/NULL），搜 `req-00` 返回 3 行，搜 `chatcmpl` 返回 1 行，搜 `xyz` 返回 1 行。三后端一致。
- 前端 BDD：3 新场景 × 3 viewports = 9 case，先红后绿。

## 交付清单

- [ ] `crates/aigw-frontend/src/pages/spend-logs/index.tsx`：列重排（Call ID 最左）+ 抽屉双 id Badge
- [ ] `crates/aigw-core/src/db.rs`：5 处 `=` → `LIKE '%X%'` + 通配符转义
- [ ] `crates/aigw-frontend/tests/features/spend-logs.feature`：3 新场景
- [ ] `crates/aigw-frontend/tests/steps/spend-logs.steps.ts`：新断言
- [ ] `crates/aigw-frontend/tests/steps/api-mocks.ts`：mock 按 query param 过滤
- [ ] `crates/aigw-core` UT：LIKE 模糊匹配三后端一致
- [ ] `docs/stages/stage-88.md`：本文档
- [ ] `docs/stages/stage-roadmap.md` + `docs/11-next-steps.md`：Phase 34 同步

## 风险与决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 列重排 | Call ID + Upstream ID 移到最左两列 | 用户明确要 call_id 在日期列之前 |
| 抽屉区分 | Badge default vs secondary + 文字标签 | 颜色 + 文字双重区分，避免混淆 |
| 模糊粒度 | 子串 `LIKE '%X%'` | 用户要「输半段能搜到」；前缀 `LIKE 'X%'` 不够 |
| LIKE 通配符转义 | 统一加 `ESCAPE '\'` | 防 `%`/`_` 注入；用户 id 含 `-` 不受影响 |
| 移动端 card | 只显示 call_id | 空间有限，双 id 在抽屉看 |
| 5 处一致性 | 必须同时改 | 列表/计数不匹配会分页错乱 |

## 不做的事（边界）

- ❌ 不改 CSV 顺序（已 Call ID 开头）
- ❌ 不改移动端 card 布局（只显示 call_id）
- ❌ 不做正则/全文搜索（LIKE 子串够用）
- ❌ 不改 `?request_id=` query param 名（保持兼容，后端语义已是双列）
- ❌ 不改 `GET /global/spend/logs/{call_id}` 详情端点（按 call_id 精确点查，无需模糊）
