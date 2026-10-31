# Stage 147: MCP 调用日志独立页（用户自助可见 + Alias/工具名/时间过滤，Phase 56）

**所属**: Phase 56（MCP Gateway 生产反馈迭代）
**预估**: ~14h（后端 user 维度过滤 + 两套端点 + 索引迁移 ×3 + 前端新 tab + BDD）
**依赖**: Stage 143（`mcp_call_log` 写入）、Stage 144（`/admin/mcp/calls` 与 `McpCallQueryStore`）、Stage 145（`CallLogDialog` 现状）、Stage 142/145（`/dash/mcp` tab 骨架）
**状态**: ✅ 完成（2026-10-31）
**提交**: `9a7e125`（与 Stage 148 合并提交，见该 commit message 的归属说明），`--signoff`，未 push。

---

## 1. 问题

2026-10-30 用户在生产网关上问「MCP 调用日志在哪看」，答案是 `/dash/mcp` → 某一行 → 「调用日志」按钮 → 一个只能看**该 server 最近 30 条**的对话框，且**只有 admin 能看**。三个实际缺陷：

1. **普通用户看不到自己的调用记录** —— `list_mcp_calls` 第一句就是 `require_admin(&auth)?`（`crates/aigw-server/src/routes/mcp.rs:1028`）。用户配了自己的 token 调失败了，没有任何自查手段，只能来问管理员。
2. **没有时间过滤、没有翻页** —— `CallLogDialog` 写死 `page_size=30`（`crates/aigw-frontend/src/pages/mcp/CallLogDialog.tsx:53`）且不渲染任何分页控件，超出 30 条的历史不可达；`McpCallQuery` 其实**已经支持** `from`/`to`（Stage 144 §3.5 定义，`mcp.rs:881`），只是 UI 没用。
3. **以 server 为入口而非以查询为入口** —— 想回答「今天哪些工具失败最多」必须逐 server 点开一次。对照消费日志页（`/dash/usage` 的 spend-logs）是一个带时间预设、多筛选、分页、详情抽屉的**整页**。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 普通用户需要看到自己的调用历史 | 「配了 token 还是 401」只能升级给管理员，自助率趋近于零；这也是 Stage 145「我的凭据」页的自然延续（配了之后得能验证） | 生产反馈直接来自用户提问「日志在哪看」 |
| H2 | `user_id` 是正确的归属维度，而不是 `key_id` | 一人多把 key（CLI / IDE / CI 各一把），按 key 划分会让「我今天的调用」散成几份，用户得先知道自己在用哪把 key | Stage 140 的用户凭据已按 `user_id` 绑定；`KeyIdentity.user_id` 是登录会话的稳定标识 |
| H3 | 时间范围 + tool 名 + alias + 状态四个过滤已足够 | 若不够，用户会回到逐 server 翻阅，本 Stage 白做 | 对照 spend-logs 的筛选集（时间/模型/状态），维度同构 |
| H4 | 现有的 `PaginationBar` 与 spend-logs 的时间预设函数可直接复用 | 否则要写第二套分页/时间控件，两页行为逐渐分叉 | `crates/aigw-frontend/src/components/ui/pagination.tsx` 已是共享组件；`presetRange`/`toDatetimeLocalValue` 目前只是内联在 spend-logs（`:158`/`:190`），纯搬移即可 |
| H5 | `CallLogDialog` 应被新页**取代**而非并存 | 两处渲染逻辑要同步维护，且筛选能力不一致（对话框永远缺时间过滤），用户会随机撞到能力较弱的那个入口 | 门禁要求 `rg CallLogDialog` 为空 |

---

## 2. 边界

**做**：
- `McpCallFilter` 加 `user_id: Option<String>`（`crates/aigw-core/src/db.rs:7589`）；三方言 `LIST_MCP_CALL_LOG_FILTERED_*` / `COUNT_MCP_CALL_LOG_*` 各加一个条件 `AND (?N IS NULL OR user_id = ?N)`，**列表与计数同步**（TD-025 的教训：两套条件不一致时，筛选后的分页条会显示全量条数）。
- `GET /admin/mcp/calls` 增加可选 `user_id` 查询参数（`McpCallQuery` 加字段，`mcp.rs:881`）：admin 可以按人筛，缺失即不筛，语义不变。
- **新增 `GET /v1/mcp/calls`**（`crates/aigw-server/src/routes/mcp_user.rs`）：身份只从 `KeyIdentity` 取，`user_id` 强制等于 `auth.user_id`，**不接受 `user_id` 入参**（同 Stage 144 的防越权代填原则）；只放行 `server_alias` / `tool_name` / `status` / `from` / `to` / `page` / `page_size`。
- `034_mcp_call_log_user_index.sql` × 三方言：`(user_id, created_at)` 索引 —— 用户侧查询恒带 `user_id` 等值 + `created_at` 范围/排序，现有 `idx_mcp_call_log_created_at` / `idx_mcp_call_log_alias_created`（`migrations/sqlite/031_mcp_call_log.sql`）都覆盖不了。
- 前端 `/dash/mcp` 新增第三个 tab「调用日志」（**两个角色都可见**）：筛选行 = Alias（`Input` + `<datalist>`，精确匹配）+ 工具名（`Input`，后端 `LIKE '%x%'` 子串）+ 时间范围（预设下拉 + 两个 `datetime-local`）+ 状态（`Select`：全部/成功/失败）；表格列 = 时间 / Alias / 工具 / 方法 / 状态 / 耗时 / 身份（server/user/anonymous）/ 大小；`PaginationBar` 带页大小；行点击开 `Sheet` 抽屉展示全部字段（`call_id` / `key_id` / `user_id` / `error_code` / 请求响应字节）。
- 时间预设与 `datetime-local` 互转的三个帮助函数目前**内联在** `crates/aigw-frontend/src/pages/spend-logs/index.tsx`（`presetRange` :158、`toDatetimeLocalValue` :190、`fromDatetimeLocalValue`）—— 抽到 `crates/aigw-frontend/src/lib/datetime.ts`，两页共用，spend-logs 改为 import（纯搬移，行为不变）。
- 管理员多一列「用户」（`user_id`，**空值显示「未绑定」**）；普通用户不显示该列（全是自己）。
- 删除 `CallLogDialog`，行内「调用日志」按钮改为切到「调用日志」tab 并预填 `server_alias`（单一渲染路径）。同步改写 stage-145 §4.1 #7 的 BDD 场景（`调用日志对话框`）为「行内按钮切到调用日志 tab 且带 alias 过滤」。

**不做**（明确排除）：
- 不做按工具/时间聚合的统计图表（Phase 54 遗留的独立需求）
- 不做导出 CSV / 实时刷新（轮询）
- 不做请求/响应 body 留存查看（`mcp_call_log` 只存字节数；body 属 body archive 另一个域）
- 不改 `call_status_filter` 对未知状态透传的行为（TD-026 #4）；新 UI 只送出两个合法值，透传路径不可达
- 不给 `mcp_call_log` 加客户端指纹（TD-027，另议）

---

## 3. 方案

### 3.1 两个端点而不是一个「角色分支」端点

```
GET /admin/mcp/calls?user_id=&server_alias=&tool_name=&status=&from=&to=&page=&page_size=
GET /v1/mcp/calls?server_alias=&tool_name=&status=&from=&to=&page=&page_size=
```

- `/admin/mcp/calls` 保持 `require_admin(&auth)?` 首句不变。它允许跨用户查询，若把用户路径塞进同一 handler，`user_id` 的取值就变成「admin 用参数 / 非 admin 用身份」的 if 分支，一处写错就是越权。
- `/v1/mcp/calls` 与 `/v1/mcp/servers` 同域同鉴权心智（`SpendAuth` + `user_id_of`）。
- **共用** `McpCallFilter` 与 `McpCallQueryStore`，不复制 SQL：两个 handler 只差「user_id 从哪来」。响应信封完全一致（`{object,data,count,total_count,page,page_size,total_pages}`），前端一个 `McpCallListResponse` 类型吃两条路径。

### 3.2 `user_id` 缺失时的行为

- `/v1/mcp/calls`：key 未绑定 user → 400 `key_without_user`，复用 `mcp_user.rs:57` 的 `user_id_of`。**不返回空列表** —— 空列表会把「我们不知道你是谁」谎报成「你没调用过」，两者的处置方式完全不同（前者去换 key，后者去查配置）。
- `/admin/mcp/calls`：`user_id` 是**可选筛选**，缺失即不筛，行为与 Stage 144 逐字节一致。

### 3.3 `user_id` 是等值过滤，工具名是 LIKE —— 这处不对称要写进注释

```
server_alias = ?        -- 等值：注册表里的标识
user_id      = ?        -- 等值：身份作用域，必须精确
status       = ?        -- 等值：枚举
tool_name LIKE '%?%'    -- 子串：给人找工具名（记不清全名）
created_at  >= / <= ?   -- 范围：RFC 3339 归一化后字符串比较
```

后来者可能想「统一成 LIKE 更宽容」——不行：`user_id` 放宽成 LIKE 会让 `u_1` 匹配到 `u_10`、`u_1x`，这是**越权**而不是宽容。Stage 144 的 `McpCallFilter` 注释说明「Each field is an AND」，本 Stage 追加一句差异说明。

### 3.4 三方言 SQL 与绑定

- SQLite / PG：`AND (?6 IS NULL OR user_id = ?6)` / `AND ($6::text IS NULL OR user_id = $6)`，`LIMIT/OFFSET` 顺延为 `?7/?8`、`$7/$8`。
- MySQL：**裸 `?` 重复绑定**（`db.rs:7838` 起），一个条件要 bind 两次 —— 加条件后 `list_mcp_call_log_filtered` 从 12 个 bind 变 14 个，`count_mcp_call_log` 从 10 个变 12 个。顺序错一位在 MySQL 上是**运行时静默错配**（类型都兼容），UT + real BDD 各兜一次。
- 六条 SQL（三方言 × list/count）必须同一次改动落地，漏一条就是 TD-025 的翻版。

### 3.5 索引

```sql
-- sqlite / postgres
CREATE INDEX IF NOT EXISTS idx_mcp_call_log_user_created ON mcp_call_log(user_id, created_at);
-- mysql（用例内联 KEY，需 ALTER）
ALTER TABLE mcp_call_log ADD INDEX idx_mcp_call_log_user_created (user_id, created_at);
```

`(user_id, created_at)` 而非 `(created_at, user_id)`：前导列必须是等值条件，`created_at` 才能走范围扫描并省掉一次排序。`created_at DESC` 不需要 `DESC` 索引 —— 反向扫描即可。`user_id` 为 VARCHAR(255)/TEXT，MySQL utf8mb4 下 255×4 = 1020 字节，无需前缀长度。

### 3.6 前端入口形状

```
/dash/mcp
├── [Server 管理] [我的凭据] [调用日志]   ← admin
└── [我的凭据]   [调用日志]              ← 非 admin
```

- **tab 而非独立路由**：`/dash/mcp` 已有 tab 容器与权限判定（`pages/mcp/index.tsx:114` `isAdmin = userRole === "proxy_admin"`）。非 admin 的「我的凭据」与「调用日志」是同一件事的两面（配令牌 → 验证是否生效），分到两个菜单项反而要解释「为什么我没有这个菜单」。
- **默认 tab 不变**：admin 进 Server 管理，非 admin 进我的凭据。定位日志是主动行为，不该顶掉首屏。
- 行内「调用日志」按钮（`:417`/`:533`）改为 `setTab("calls") + setCallsAlias(s.alias)`，alias 状态提升到 `McpPage`；不再渲染对话框。tab 顺序与 `TabsList` 的 `flex-wrap` 需同改（Stage 145 已踩过一次 320px 横向溢出）。

### 3.7 筛选与默认值

| 控件 | 形状 | 传参 | 备注 |
|---|---|---|---|
| Alias | `Input` + `<datalist>`（选项来自 `/admin/mcp/servers` 或 `/v1/mcp/servers` 的 alias） | `server_alias`，**精确** | 已删除的 alias 建议不到，手输仍可查（日志表刻意不建外键） |
| 工具名 | `Input` | `tool_name`（服务端 LIKE 子串） | 只在输入后 300ms 生效（`useDeferredValue` 或 debounce），避免每键一次请求 |
| 时间 | `Select`（1h/24h/7d/30d/自定义）+ 两个 `datetime-local` | `from`/`to`（RFC 3339） | **默认 24h**；spend-logs 是 4h |
| 状态 | `Select`：全部/成功/失败 | `status`（`success` / `failed`） | 失败在 UI 显示为「失败」，不上送 `error` 别名 |
| 分页 | `PaginationBar`（30/50/100） | `page`/`page_size` | 任一筛选变更时重置 `page=1` |

**默认 24h 而不是 4h**：MCP 调用是低频事件，4h 内常常一条都没有，空表会被误读成「功能坏了」。

### 3.8 抽屉

`Sheet` 右侧抽屉，展示一行全字段：`call_id`、时间（本地时区 + 原值）、alias、工具、方法、状态、`error_code`、身份、`key_id`、`user_id`、耗时、请求/响应字节。与 spend-logs 的详情抽屉一致，不新写页面。不用 Dialog 是因为 spend-logs 已建立「详情在右侧」的肌肉记忆，且 13 个字段的纵向列表在窄屏下 Dialog 会撑高。

---

## 4. 测试

### 4.1 core UT（≥5）

`crates/aigw-core/src/db.rs` 测试模块，SQLite 内存库（沿用既有 `make_test_*` 构造先例）：

1. `list_mcp_call_log_filtered` 带 `user_id=A` 只返回 A 的行（造 A/B 各 2 行）。
2. `count_mcp_call_log` 同 filter 得 2；`user_id` 换成不存在的用户得 0。
3. `user_id + server_alias + tool_name + 时间范围` 四条件组合，命中 1 行且计数 1。
4. `user_id=None` 时行为与 Stage 144 一致（返回全部）。
5. **计数与列表同条件**：带 `user_id + from/to` 时 `total_count == data.len()`（数据量小于一页）；分页到第二页时 `total_count` 不变而 `data.len()` 变小。

### 4.2 handler UT（≥6）

`crates/aigw-server/src/routes/mcp_user.rs` / `mcp.rs` 的 `#[cfg(test)]`：

1. 普通用户 key 调 `/v1/mcp/calls` 只看到自己（造两个 user 的行）。
2. **篡改无效**：查询串带 `user_id=<别人>`，响应仍只含自己的行（服务端不接受该参数，等价于忽略）。
3. key 无 `user_id`（master key 直连）→ 400 `key_without_user`，不是空列表。
4. `server_alias` / `tool_name` / `status` / `from` / `to` 在 `/v1/mcp/calls` 上各自生效（至少 alias 与 status 两条）。
5. `/admin/mcp/calls?user_id=x` 生效；不带 `user_id` 时与 144 行为一致。
6. 信封：`page`/`page_size` 回显、`total_pages` 为 `ceil(total_count/page_size)`、`total_count>0 && data 为空`（越界页）时 `total_pages` 仍正确。
7. 非法 `from`（非 RFC 3339）→ 400 `invalid_time_range`（复用 `call_time_bound`）。

### 4.3 BDD

**mock（≥6 场景 × 3 viewport）**，`crates/aigw-frontend/tests/features/mcp.feature` + 新 `mcp-call-log.feature`：

1. 非 admin 进 `/dash/mcp` 能看到「调用日志」tab，切过去只看到自己的行，且**没有「用户」列**。
2. admin 的表格有「用户」列，空 `user_id` 显示「未绑定」（**不是「匿名」** —— 用户列说的是「这把 key 有没有绑定 `user_id`」，而「匿名」徽章说的是「出站有没有带凭据」，两件事不能共用一个词）。
3. 时间预设切到 1h / 7d → 断言请求 URL 的 `from` 变化（读 `/v1/mcp/calls` 的 `searchParams`）。
4. Alias 输入框聚焦后 `<datalist>` 给出注册表中的 alias；选中后请求带 `server_alias=<alias>`。
5. 工具名输入 `sea` → 请求带 `tool_name=sea`（子串语义由后端负责，前端不做展开）。
6. 行内「调用日志」按钮 → tab 切到调用日志且 alias 输入框已预填该 server。
7. 空结果与请求失败各有可读文案（不是空白表格）。
8. 分页：翻到第二页请求带 `page=2`，`PaginationBar` 文案的 `total` 来自 `total_count`。

**real BDD（三方言 ≥2 场景）**，`tests/bdd_real/`：`034` 索引迁移可应用（迁移后 `mcp_call_log` 上存在 `idx_mcp_call_log_user_created`）；写入两行不同 `user_id` 后各查一次，结果行数各为 1。MySQL 额外断言索引确实被 `EXPLAIN` 用上（`key` 列等于索引名）。

### 4.4 过滤条件不可用的组合要能被发现

列表与计数共用同一个 `McpCallFilter` —— 这是**唯一**防回归手段，因此 §4.1 #5 是必做项而非加分项：任何一条 SQL 只改了一半（列表加了 `user_id`、计数没加），分页条就会显示全量条数而表格只显示子集。UT 必须显式断言 `total_count` 与 `data.len()` 的关系，而不是只断言「返回了行」。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/034_mcp_call_log_user_index.sql` | 新增（`033` 已被 Stage 146 占用） |
| `crates/aigw-core/src/db.rs` | `McpCallFilter` 加 `user_id`；六条 SQL（三方言 × list/count）加条件；三方言绑定各 +2（MySQL）+1（PG/SQLite），`LIMIT/OFFSET` 编号顺延 |
| `crates/aigw-server/src/routes/mcp.rs` | `McpCallQuery` 加 `user_id`；`list_mcp_calls` 构造 filter 时带入 |
| `crates/aigw-server/src/routes/mcp_user.rs` | 新增 `list_my_calls`（`user_id_of` + 复用 `McpCallFilter`/`McpCallQueryStore`）；`call_time_bound` 若在 `mcp.rs` 内为 private 则需提升可见性或搬至共享模块 |
| `crates/aigw-server/src/main.rs` | `.route("/v1/mcp/calls", get(mcp_user::list_my_calls))`（紧随 `:809` 的 `/v1/mcp/servers`） |
| `crates/aigw-frontend/src/lib/datetime.ts` | 新增（`presetRange` / `toDatetimeLocalValue` / `fromDatetimeLocalValue` 从 spend-logs 抽出） |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 删内联帮助函数，改 import（纯搬移） |
| `crates/aigw-frontend/src/pages/mcp/CallLogTab.tsx` | 新增（筛选行 + 表格 + `PaginationBar` + `Sheet`） |
| `crates/aigw-frontend/src/pages/mcp/CallLogDialog.tsx` | **删除** |
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | 第三个 tab；`callsAlias` 状态；两个行内按钮改切 tab；移除 `CallLogDialog` 渲染（`:666`） |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | `McpCallListResponse` 复用；补 `McpCallQueryParams` 之类参数类型（若需要） |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 补 key（时间预设、列头、抽屉字段、「用户」列、空/错态）；原 `mcp.calls.*` 大部分可复用 |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 重生成 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 加 `/v1/mcp/calls` 路由（按 `role` 返回不同 user 的 fixture）；`/admin/mcp/calls` 支持 `user_id` 查询参数 |
| `crates/aigw-frontend/tests/features/mcp.feature` + `tests/steps/mcp.steps.ts` | 改写 stage-145 #7 场景；追加新场景 |

---

## 6. 回归验证

全部通过 `task`，禁止裸 `cargo`：

- `task test`（含新增 core/handler UT）
- `task bdd`
- `task bdd-real-sqlite`
- `task test-db-up` → `task bdd-real-pg` → `task bdd-real-mysql` → `task test-db-down`
- `task fe-bdd` / `task fe-lint` / `task fe-build`
- `task fmt` / `task lint` / `task doctor`

手工：生产网关上用普通用户 key 调 `/v1/mcp/calls`，确认只见自己的行（用 Claude Code 走一次 `/mcp` 的 `tools/list` + `tools/call` 后查），并对照 `/admin/mcp/calls?user_id=<该用户>` 的行数是否一致。

---

## 7. 门禁

- [x] 普通用户看不到别人任何一行 —— handler UT 造两个 user 的行，断言响应中不存在他人 `user_id`；且查询串带 `user_id=<他人>` 时结果不变
- [x] key 未绑定 user 时 `/v1/mcp/calls` 返回 400 `key_without_user`（不是 200 空列表）
- [x] `/admin/mcp/calls?user_id=x` 生效，且不带 `user_id` 时与 Stage 144 行为逐字段一致（`:779`/`795` 路由未改）
- [x] 时间 / alias / 工具名三个过滤各有一条 BDD 断言到**请求参数**（不是只断言表格变化）
- [x] 计数与列表同条件：UT 断言 `user_id + 时间范围` 下 `total_count` 与 `data.len()` 关系正确（TD-025 防回归）
- [x] `CallLogDialog` 已无引用 —— `rg CallLogDialog crates/` 为空，文件已删
- [x] `en.json` / `zh-CN.json` key 集合一致（`task fe-lint` 内 `check-i18n-keys.sh` 输出 `✅`）
- [x] 320px 视口无横向滚动 —— 三个 tab 的 `TabsList` 与筛选行实测 `scrollWidth <= clientWidth`
- [x] 三方言 real BDD 全绿，含 `034` 索引存在且 `user_id` 查询命中断言 —— `bdd-real-{sqlite,postgres,mysql}` 各 **87 scenarios (87 passed) / 490 steps (490 passed)**（`/tmp/p56-evidence/s147-real-{sqlite,postgres,mysql}-r2.log`，EXIT=0）。`mcp_calls_real.feature` 的索引场景读的是 `pg_indexes` / `information_schema.statistics` / `pragma_index_info` 的**列序**（`user_id` 必须前于 `created_at`），MySQL 另加 `FORCE INDEX` 的 `EXPLAIN` 断言
- [x] `task test` / `task bdd` / `task fmt` / `task lint` / `task doctor` 全绿，证据落盘到 `/tmp` —— 最终树（Stage 148 Gate 4 修正之后）复跑：`task test` **1620 passed / 0 failed**（22 个 test result 块全部 `ok`，`/tmp/p56-evidence/s148-test-final.log`，`FAILED` 行数 0）；其中内嵌的 BDD 半程 **378 scenarios（365 passed / 13 skipped）· 2238 steps（2225 passed / 13 skipped）**；独立 `task bdd` 复跑同数字（`/tmp/p56-evidence/s148-bdd-r2.log`）—— 13 个 skip 全部来自 `body_archive_read.feature`（7 步）与 `body_archive_write.feature`（6 步）的既有缺陷（TD-028，step expr 与 feature 文本不匹配导致 cucumber-rs 静默跳过，非本 Stage 引入）；`task fmt` / `task lint` EXIT=0（`/tmp/p56-evidence/s148-{fmt,lint}-final.log`）、`task doctor` EXIT=0（`/tmp/p56-evidence/s148-doctor-r4.log`）。**门禁之后又按用户反馈改了用户列的空值措辞**（「匿名」→「未绑定」，key 从 `anonymousUser` 改名 `unboundUser`，见 §8 末条），改动只涉及前端文案与一条 BDD 断言：`task fe-lint` EXIT=0、`task fe-bdd` 复跑 **639 passed / 3 skipped / 0 failed**（`/tmp/s148-anon-accept/febdd-full.log`），后端无改动故 `task test` / `task bdd` 数字不变

---

## 8. 风险与遗留

- **`mcp_call_log` 无界增长** —— 本 Stage 不做保留期/分区/清理。日志越积越多，本 Stage 的索引只是让「按 user + 时间」这一条查询保持可用；数据量到千万级后仍需要归档策略，登记为已知，不在本 Stage 解决。
- **`user_id` 为 NULL 的行用户侧永远看不到** —— master key 直连、以及未绑定用户的 key 产生的调用，`/v1/mcp/calls` 一律不计入（`user_id = ?` 等值匹配天然排除 NULL），只有 admin 能看到。这是正确的（无主的调用不该归属给某个用户），但意味着「用户说没记录、admin 说有一条」的对话会重复出现，UI 上无法自我解释，因此 §3.2 的 400 语义必须明确。
- **用户列的空值措辞（stage 收尾后按用户反馈修正）** —— 初版渲染成「匿名」/「Anonymous」，与同表左侧的凭据徽章（`owner.anonymous`，语义是「出站没有带凭据」）撞词。用户列回答的是「**这把 key 有没有绑定 `user_id`**」（master key 直连、或未绑定用户的 key），与「出站凭据来源」是两个正交的维度；同一张表里两个「匿名」会让运维把「key 没绑用户」误读成「有人在未授权访问」。已改为「未绑定」/「Not bound」，key 也从 `anonymousUser` 改名为 `unboundUser`（避免下一个人照着 key 名把它改回去）。
- **Alias 的 `<datalist>` 建议值来自注册表** —— 已删除的 alias 建议不到但手输仍可查。日志表刻意不建外键到 `mcp_servers`（`031` 头注释已说明：审计日志要活过它描述的注册项），这是有意的取舍而非缺陷。
- **tab 数量增长后的 320px 挤压** —— 从两个长标签变三/两个，`TabsList` 在 320px 下会溢出（Stage 145 已踩过一次 328px > 320px）。必须复测 `overflow-x-auto` / `flex-wrap`，且筛选行（4 个控件）在窄屏下要换行而非横向滚动。
- **删 `CallLogDialog` 会让 stage-145 的一条 BDD 场景文本失效** —— 改写断言（仍然验证「行内按钮能到达日志且带 alias 过滤」），不是删除断言；stage-145 §4.1 #7 与 §7 门禁里对应的描述需在本 Stage 收尾时注记。
- **工具名过滤是子串匹配，且大小写不敏感（`LOWER`）** —— 对 `LOWER` 的函数调用会让 `tool_name` 上任何索引失效；但 `tool_name` 目前无索引、且始终与其他条件组合查询，暂可接受。若将来 `tool_name` 成为主查询维度，需要改成前缀匹配或表达式索引。

---

## 9. 对后续 Stage 的影响

- **Stage 148（探测进度）不依赖本 Stage**，两者可并行。
- 本 Stage 的「按 `user_id` 作用域」范式（身份来自 `KeyIdentity`、不接受身份入参、共用 filter/store 只换来源）可被将来的「我的开销 / 我的用量」类页面直接复用。
- 若 MCP 后续计费化，日志页要加「金额」列与按金额排序 —— 当前表没有金额字段，届时是 `mcp_call_log` 加列 + 迁移，而非前端改动。
- 时间帮助函数抽到 `lib/datetime.ts` 后，将来任何带时间预设的页面（审计、告警）都不必再复制一份 `presetRange`。
