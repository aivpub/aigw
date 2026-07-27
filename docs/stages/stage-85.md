# Stage 85: request_id → call_id 改名 + 上游对账链路打通

**Phase**: 32 — 对账链路打通
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: Stage 84（Phase 31 Body Archive 生产化已完成；本 Stage 与 Body Archive 解耦，独立于 spend_logs 表）

**设计文档**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md`（v5，已评审对齐核心预期）

---

## 核心预期（v5 对齐）

**任意一条 SpendLog 记录都能用上游 `request_id` 去 provider 侧对上账，无论成功还是 4xx/5xx 失败。**

这是本 Stage 唯一业务目标（详见设计文档 §1.3）。改名 `call_id`、流式提取、失败路径提取均为支撑项。

## 背景

当前 aigw 把自身生成的 UUID v7 存在 `spend_logs.request_id`（PK，语义=网关调用标识），但行业惯例（含 litellm）中 `request_id` 指上游 provider 返回的请求 ID。导致两个问题：

- **语义混淆**：aigw 的 `request_id` 是网关 ID 还是上游 ID，代码无法区分
- **对账断链**：SpendLog 未存上游 provider 的请求 ID，售后退款/问题排查时无法与 provider 对账

v5 设计文档已评审定稿（5 轮迭代），核验代码后修正了 migrate 映射机制描述（虚构 `SpendLog{...}` 构造处，实际走列名批量映射）、补了对外协议字段边界（§6.3）、可观测性影响（§10）、失败路径 4xx/5xx 提取（v5 增量）。

## 目标

| 目标 | 说明 | 与核心预期的关系 |
|------|------|------------------|
| **打通对账链路** | SpendLog 存储上游 `request_id`（含成功 + 4xx/5xx 失败路径），可直接与 provider 对账 | **核心本体** |
| **消除语义混淆** | 网关调用 ID 改名 `call_id`，`request_id` 回归上游 ID 语义 | 支撑项 |
| **存量 litellm 兼容** | migrate 时 `call_id` = litellm 的 `request_id` | 支撑项 |

## 内部执行节奏（不拆 Stage，仅分阶段）

强耦合串行：DB schema 改名是路由层编译前提，路由层是前端/migrate 前提。Stage 内分三阶段，subagent 在阶段②内部并发：

```
① DB schema + 模型/DB 层（串行前置，~1.5h）
   022 迁移 pg/mysql/sqlite（双重条件探测幂等）+ models.rs（SpendLog + Tag）+ db.rs（~86 处 + 上游 id 写入方法）+ body_archive（~48 处 + parquet reader 兼容）+ daily_spend_queue UNIQUE 列串
        ↓ 编译前提
② 路由层 + 上游 id 全路径提取 + 前端 + migrate（subagent 并行，~3h）
   chat.rs/v1_messages.rs 字段改名 + 流式 chunk id 提取 + 4xx/5xx 失败路径提取 + Phase 2 UPDATE 调用补 upstream_id + spend.rs/openapi/main.rs span 字段 + 前端（3 interface + 展示列 + CSV + 搜索）+ migrate override
        ↓
③ 三端联调 + 全测试 + 端到端（~3.5h）
   10 BDD + 5 非 BDD 单测 + cargo test + 端到端 ①-⑥
```

## 关键实现要点（对照设计文档 v5）

### ① DB schema + 模型层（设计文档 §3-§4.1-§4.4）

- **022 迁移**（`022_rename_request_id_to_call_id.sql`，pg/mysql/sqlite 三份）：
  - Phase 1: `spend_logs.request_id` RENAME → `call_id`（双重条件 `EXISTS(request_id) AND NOT EXISTS(call_id)`）
  - Phase 2: 新增 `spend_logs.request_id TEXT`（上游 ID，可空）
  - Phase 3: `daily_tag_spend.request_id` RENAME → `call_id`
  - Phase 4: `CREATE INDEX idx_spend_logs_request_id ON spend_logs(request_id)`
  - MySQL 全用 `INFORMATION_SCHEMA + PREPARE`（原生不支持 `ADD COLUMN IF NOT EXISTS`）
  - 002/015 原迁移不动，统一由 022 收敛
- **models.rs**：`SpendLog.request_id` → `call_id` + 新增 `request_id: Option<String>`；`Tag { tag, request_id }` → `Tag { tag, call_id }`（:185）
- **db.rs**（三套 Sqlite/Mysql/Postgres + Database 转发层，~86 处）：SQL 列名改名 + 方法名（`get_spend_log_by_request_id` → `get_spend_log_by_call_id`）+ `update_spend_log` 扩展 `upstream_request_id: Option<&str>` 参数，UPDATE 用 `COALESCE($new, request_id)`（没提取到不覆盖）
- **body_archive**（~48 处）：`BodyRow.request_id` → `call_id`；parquet reader 加列名兼容（读到旧 `request_id` 列映射为 `call_id`）；上线前清空开发/测试环境存量 parquet
- **daily_spend_queue.rs:196**：`daily_tag_spend` UNIQUE 列串 `"request_id, tag, ..."` → `"call_id, tag, ..."`

### ② 路由层 + 上游 id 提取（设计文档 §4.3 / §4.5 / §5 / §10）

- **HTTP 层不改**（§2.2 边界）：`main.rs` 的 `tower_http::request_id::{RequestId, MakeRequestId, SetRequestIdLayer}`、`chat.rs:24` 的 `use`、`chat.rs:677/928/1073/1075/1076` 的局部变量 `let request_id = extensions.get(...)` —— 变量名保留 `request_id`，值赋给 `call_id` 字段
- **chat.rs / v1_messages.rs**：`SpendLog{...}` 字段名改名（含失败路径 ×3 + Phase 1 INSERT + Phase 2 UPDATE）
- **上游 id 提取 — 成功路径**：
  - 非流式：`response_json.get("id")` 取上游 id
  - 流式 OpenAI：在现有 `chunk_jsons` 收集循环里顺手取首 chunk 的 `id`（不新开循环）
  - 流式 Anthropic：取 `message_start` 事件的 `message.id`
- **上游 id 提取 — 失败路径（v5 增量，核心预期关键）**：
  - OpenAI 4xx/5xx：从 `error_body`（已是字符串，`chat.rs:1093`）解析取 `id`，fallback 上游响应头 `x-request-id`（复用 `:1067` 已有逻辑）
  - Anthropic 4xx/5xx：从 error body 取 `request_id` 字段（协议字段名，值=上游 id），fallback 响应头 `request-id`/`x-request-id`
  - 流式部分成功后失败：已由 `chunk_jsons` 收集循环提取 + `COALESCE` 保护
  - 连接/超时/aigw 侧失败：无 body，留 NULL（不可避免，非缺陷）
- **Phase 2 UPDATE 调用**：方案 A 扩展 `update_spend_log` 签名后，所有调用点补 `upstream_id` 参数（失败路径 ×3 + 流式 Phase 2 UPDATE），漏了编译失败兜底
- **main.rs:126** tracing span 字段 `request_id` → `call_id`（变量名不改，§10 方案 A）
- **spend.rs / openapi.rs:255**：API JSON 拆成 `call_id`(required) + `request_id`(nullable)；URL `/{request_id}` → `/{call_id}`
- **对外协议字段不改**（§6.3 边界）：`v1_messages.rs:48/141/165/179/213` + chat.rs 响应 body 的 `request_id` 字段名保留（Anthropic/OpenAI 契约），值= call_id
- **migrate**（§4.5）：`remote_import.rs:546` 注入 override `request_id → call_id`（源端 litellm 的 request_id 灌入目标 PK，目标上游 request_id 列置 NULL）；`remote_export.rs` 反向 override；aigw 侧测试夹具改列名（`remote_import.rs:1224`、`remote_export.rs:547`）；litellm 源端 SQL/夹具不改
- **前端**（§5）：3 个独立 interface（`spend-logs/index.tsx:35` SpendLog、`:47` SpendLogDetail、`dashboard/index.tsx:43`）的 `request_id` → `call_id` + 新增 `request_id?`；列表第一列 "Request ID" → "Call ID" + 新增 "Upstream ID" 列；CSV headers；搜索框 placeholder 保留

### ③ 测试 + 联调

- **10 个 BDD**（`crates/aigw-server/tests/`，含补的 `common_steps.rs`）字段名/mock/step 同步
- **5 个非 BDD 单测**（`integration_test.rs:220`、`body_archive/query.rs` 7 处、`body_archive/writer.rs` 2 处、`db.rs:4599`、`spend.rs` 5 处）字段名同步
- 三端迁移联调：pg/mysql/sqlite 各跑「存量库 / 新装库 / 重跑库」三路径

## TDD 红绿流程

本 Stage 强制走严格 TDD 红绿循环（对齐项目规范）。核心增量（失败路径 4xx/5xx 提取）必须先写 BDD 跑红：

### Red 阶段（先写失败测试）

1. **BDD 失败 4xx 提取**：上游返回 4xx error body 含 `id`/`request_id` → DB `spend_logs.request_id` 非空、`call_id` 非空、status=failure
   - 当前失败：失败路径未提取上游 id
2. **BDD 失败 5xx 提取**：上游返回 5xx + 响应头带 `x-request-id` → DB `request_id` 取响应头值
3. **BDD 流式部分成功后失败**：收首 chunk 后断开 → DB `request_id` 非空（首 chunk 已提取）
4. **BDD 连接超时**：上游连接超时 → DB `request_id` NULL（不可避免）、`call_id` 非空
5. **BDD 对账点查**：`SELECT * FROM spend_logs WHERE request_id = 'msg_xxx'` 能查到行
6. **BDD 搜索双列**：搜索框输入 `call_id` 与 `request_id` 都能命中

### Green 阶段（实现至通过）

按内部三阶段执行，逐条使测试转绿。发现的错误及时修复并重跑。

## 验收标准（核心预期三判据）

- [ ] **正向对账**：`SELECT * FROM spend_logs WHERE request_id = 'msg_xxx'` 查到行，EXPLAIN 走 `idx_spend_logs_request_id`
- [ ] **反向追溯**：给定 `call_id`，能拿到对应上游 `request_id`（非流式/流式/4xx-5xx 四路径非空，连接超时除外）
- [ ] **覆盖盲区**：失败请求（4xx/5xx）的 `request_id` 非空（从 error body/响应头提取）
- [ ] 三端迁移：pg/mysql/sqlite 各跑存量/新装/重跑三路径全过
- [ ] `cargo check` + `cargo test` 全绿（lib 单测 + 10 BDD + 5 非 BDD 单测）
- [ ] 端到端 ①-⑥ 全过（设计文档 §7 步骤 17）
- [ ] migrate 导入存量：litellm `request_id` 正确落到 `call_id`(PK)，无 NULL 插入失败
- [ ] 对外 API：响应 body 的 `request_id` 字段名仍在、客户端不受影响
- [ ] 日志：tracing span 字段显示为 `call_id`

## 关键文件

| 文件 | 操作 |
|------|------|
| `migrations/022_rename_request_id_to_call_id.sql`（pg/mysql/sqlite 三份） | 新增：RENAME + ADD COLUMN + INDEX，双重条件幂等 |
| `crates/aigw-core/src/models.rs` | `SpendLog.request_id` → `call_id` + 新增 `request_id: Option<String>` + `Tag` 变体改名 |
| `crates/aigw-core/src/db.rs` | SQL 列名 + 方法名 + 参数名（三套 ~86 处）+ 扩展 `update_spend_log` 加 `upstream_request_id` |
| `crates/aigw-core/src/body_archive/{mod,writer,query}.rs` | 字段 + SQL + 函数参数改名（~48 处）+ parquet reader 列名兼容 |
| `crates/aigw-core/src/daily_spend_queue.rs:196` | `daily_tag_spend` UNIQUE 列串改名 |
| `crates/aigw-server/src/routes/chat.rs` | 字段名 + 流式 OpenAI chunk id 提取 + 4xx/5xx 失败路径提取 + Phase 2 UPDATE 补参数 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 字段名 + 流式 Anthropic message_start.id 提取 + 4xx/5xx 失败路径提取 |
| `crates/aigw-server/src/routes/spend.rs` | API JSON + SQL + URL 参数 |
| `crates/aigw-server/src/main.rs` | 路由路径参数（:379）+ tracing span 字段（:126，§10 方案 A）|
| `crates/aigw-server/src/openapi.rs:255` | 拆 `call_id` + `request_id` 两字段 |
| `crates/aigw-migrate/src/remote_import.rs` | 注入 `request_id→call_id` override + aigw 测试夹具改名 |
| `crates/aigw-migrate/src/remote_export.rs` | 反向 override + aigw 测试夹具改名 |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 3 interface + 展示列 + CSV + 搜索 |
| `crates/aigw-frontend/src/pages/dashboard/index.tsx` | interface + 行 key |
| 测试：10 BDD + 5 非 BDD 单测 | 字段名 + mock 数据同步 |

## 不改动清单（务必跳过，否则破坏功能）

| 文件 / 位置 | 原因 |
|------------|------|
| `main.rs:55/102/110/116/124` `tower_http::request_id::*` | HTTP 中间件层（§2.2） |
| `chat.rs:24/677/928/1073/1075/1076` HTTP 层变量与头 | HTTP 层（§2.2） |
| `v1_messages.rs:48/141/165/179/213` + chat.rs 响应 body `request_id` | Anthropic/OpenAI 协议字段（§6.3） |
| `v1_messages.rs:1424/1428` 测试断言上游错误响应 `request_id` | 上游协议字段断言（§6.3） |
| `aigw-migrate/src/native.rs` litellm 源端 SQL/列探测 | litellm 源表 schema（§4.5） |
| `remote_import.rs:865/1023/1041/1200/1211` + `remote_export.rs:573/574` litellm 夹具 | litellm 源/目标表 schema（§4.5） |

## 测试要求

- **TDD**：Red 先写失败测试（6 个 BDD 场景）→ Green 实现至通过 → 错误及时修复重跑
- **BDD**：10 个 BDD feature + step 全绿（含失败路径 4xx/5xx/流式部分成功/连接超时 4 场景）
- **非 BDD 单测**：5 个文件字段名同步，编译通过
- **三端联调**：pg/mysql/sqlite × 存量/新装/重跑 = 9 路径全过
- **real BDD**：端到端 ①-⑥ 全过

## 风险提示（对照设计文档 §8）

- migrate 源端 `request_id` 必须 override 到 `call_id`，否则存量导入 PK 为 NULL 失败（§4.5）
- 对外协议响应体 `request_id` 误改成 `call_id` 会破坏客户端契约（§6.3）
- 流式上游 id 必须从 chunk 提取，非 body（§4.3）
- 022 迁移双重条件探测保证幂等（§3.1）

---

> 本 Stage 完成后，任意 SpendLog 都能通过上游 `request_id` 与 provider 对账，无论成功还是失败。这是 aigw 对账能力的核心补齐。
