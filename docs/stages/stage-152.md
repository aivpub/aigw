# Stage 152: MCP 请求/响应 body 留存与查看（Phase 58）

**所属**: Phase 58（MCP 日志与审计完善）
**预估**: ~18h（表 ×3 + 写入接线 + 保留期接入 + 详情端点 + 前端查看器 + 测试；冷存储经 §3.4 实测后不做）
**依赖**: Stage 150（保留期 worker 与配置骨架）
**状态**: ✅ 完成（2026-10-31）

---

## 1. 问题

`mcp_call_log` 只存字节数：

```
call_id / server_alias / tool_name / method / key_id / user_id /
token_owner / status / error_code / latency_ms /
request_bytes / response_bytes / created_at
```

`grep body crates/aigw-core/migrations/sqlite/031_mcp_call_log.sql` 零命中，MCP 路径也没接 `BodyArchiver`（`body_archiver: None` 只出现在测试 state）。后果：

- 用户报「调用 `zhiyan__QueryClickhouse` 返回了空结果」→ 日志显示 `status=success`、`response_bytes=312`，**312 字节里是什么看不见**。
- 上游报 `MCP error -32602: Invalid arguments for tool metadata: path [docid] Required`（生产实测）→ 只能看到 `error_code=-32602`，**看不懂是哪个参数缺了**，除非用户把上游的原始响应贴回来。

对比：LLM 请求的 body 是留存的（`spend_logs.messages` / `response`，超期后归档到 Parquet）。MCP 作为「工具调用审计」反而比聊天更该留 body —— 聊天出错用户自己看得见答案，工具调用出错只有日志。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | body 应存**进 `mcp_call_log` 旁边的新表**，不是给主表加列 | 主表是列表页的热路径（每页 30 行），加两个 TEXT 列会让 `SELECT *` 型查询把大字段一起捞出来；SQLite 上尤其明显 | 主表 SELECT 列清单都是显式的（`db.rs:7639`），不选新列；列表性能实测对比 |
| H2 | 默认**开启**，与 body_archive 的默认关闭相反 | MCP body 通常几百字节到几 KB（工具参数 + 结果），与 LLM 的 MB 级不可比；默认关闭会让这个功能在「已经出问题之后」才被打开，而那时历史已经没了 | 生产上 196 个工具、典型响应 < 2 KB；实测 body 大小分布后再定 |
| H3 | 超大 body 要截断而不是丢弃 | 丢弃会让「为什么这条没有 body」不可解释；截断至少保留前缀 + 明确标记 | 单条上限配置（默认 64 KiB），超限截断并置 `truncated=true` |
| H4 | 大 body 走冷存储（Parquet）与 LLM 路径共用基础设施 | 另写一套冷存储要重复 S3 配置、multipart、分片、压缩 | 复用 `body_archive::writer` 的 `write_parquet_shards` —— **§3.4 实测后否决**：`write_parquet_shards` 写死 `BodyRow`（LLM schema），复用要先把「行 → RecordBatch」抽出来，而生产量级下 90 天热存成本可忽略。本 Stage 不做冷存储，body 与日志同寿命删除 |
| H5 | 脱敏只做「已知敏感头」，不试图理解 body 语义；请求侧存规范化后的 `params` | 试图识别 body 里的密钥会误报/漏报，且 MCP 工具参数千奇百怪 | **实现取更简的一侧**：请求体按客户端原样字节存（`cut` 只截断，不改写），响应同理。规范化要解析 JSON 再重序列化，会把「客户端到底发了什么」这一审计事实改掉 |

---

## 2. 边界

**做**：
- 新表 `mcp_call_body`（三方言）：`call_id` PK（FK 语义但**不建外键**，与 `mcp_call_log` 同款理由）、`request_body`、`response_body`、`truncated`、`created_at`
- 写入：`mcp_relay::record` 与 `finish` 在写日志行之后写 body，**同一个 `await` 内完成**（见 §3.2）
- 单条上限 64 KiB（配置），超限截断
- 保留期接入 Stage 150 的 worker（同一 `mcp_log_retention` 配置，body 与日志同期删除）
- `GET /admin/mcp/calls/{call_id}/body` + `GET /v1/mcp/calls/{call_id}/body`：返回 `{call_id, request, response, truncated}`
- 前端：调用日志抽屉加「请求 / 响应」两个只读区（等宽 `<pre>` + 截断提示）
- 保留期同删：`mcp_call_body` 与 `mcp_call_log` 按同一个 `call_log_days` 删，不留孤儿

**不做**（明确排除）：
- **不做冷存储**（§3.4 决策：90 天热存，body 与日志同寿命删除；触发条件见 §8）
- **不做 body 的全文搜索** —— 需要 FTS/索引，成本远高于收益（排查靠 call_id 定位，不靠关键词搜）
- **不做 body 的编辑/重放** —— 重放工具调用是有副作用的高危操作
- **不记录 SSE 流式响应的逐帧内容** —— 只记录拼接后的全文；`tools/call` 的 SSE 极少见（生产上 5 个 server 全部返回 `application/json`），且逐帧记录会放大存储
- **不对 body 做加密** —— 与 `spend_logs` 的 body 同级机密（同一套访问控制：admin only）；加密会让冷存储无法按列读
- **不做 body 的导出**（CSV/JSON 下载）

---

## 3. 方案

### 3.1 表与大小

```sql
CREATE TABLE IF NOT EXISTS mcp_call_body (
    call_id       TEXT PRIMARY KEY,
    request_body  TEXT,
    response_body TEXT,
    truncated     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mcp_call_body_created ON mcp_call_body(created_at);
```

- `truncated` 三方言类型：SQLite `BOOLEAN`（存 0/1）、PG `BOOLEAN`、MySQL `TINYINT(1)` —— 与 `body_archived` 的既有做法一致（`body_archive` 的代码里就是 `body_archived = FALSE`）。
- MySQL 的 `request_body` / `response_body` 是 `MEDIUMTEXT`：`TEXT` 只有 65,535 **字节**，而默认上限正是 64 KiB，多字节字符下写在限内的 body 也会溢出。
- **不加 FK 到 `mcp_call_log`**：删除顺序（先删主表还是先删 body）会成为保留期实现的耦合点；且审计表本来就不建 FK（`031` 头注释已确立这条）。
- 两表用同一个 `call_id`（UUID v7，日志行与 body 行同源），保留期 worker 按 `created_at` 各自删。

### 3.2 写入路径

三处产生日志行：

| 位置 | 场景 | body 来源 |
|---|---|---|
| `finish`（`mcp_gateway.rs:379`） | 本地应答（discover/initialize/参数错/404/403） | `outcome.body`（已序列化）+ 调用方的 `body` |
| `relay_buffered`（`mcp_relay.rs:84`） | 上游返回 JSON | 入参 `body`（需为 `relay()` 加参数）+ 读到的 `bytes` |
| `relay_event_stream`（`mcp_relay.rs:135`） | 上游返回 SSE | 入参 `body` + 拼接后的全文（**上限 64 KiB，超出只留前缀**） |

`finish` 已经有 `outcome.body`（`Value`）与响应字节，但**没有请求体** —— `post_mcp` 的 `body: Bytes` 在解析后就被丢弃了。所以签名要加一个参数（`finish` 有 25 个调用点）或把 `request_bytes` 换成 `request_body: &[u8]`（长度可从中取）。选后者：一处改动替代「保留长度 + 另传 body」两个参数。

**写入时机**：与日志行同一处 `record_with_bodies` 里 `await` 完成（不 `spawn`）—— 一行 INSERT 的延迟远小于一次上游往返，且 `spawn` 会让「测试断言 body 存在」变成 flaky。写 body 失败只 `warn!`，不把一次成功的调用变成 500（与 `record` 同款约定）。

`limit: Option<usize>` 就是开关：`None` 表示 `record_bodies=false`，函数在写完日志行后直接返回，一行 body 都不写。把开关做成「`None` 而不是传整个配置」是为了让 SSE 那条后台任务也能持有它。

### 3.3 截断

```rust
pub fn cut(bytes: &[u8], limit: usize) -> (String, bool)
```

- 调用方传 `record_bodies.then(|| max_body_kib * 1024)`：`None` 表示不记录，直接不写 body 行。
- **按字节截断可能切断 UTF-8 多字节字符** → 先 `std::str::from_utf8`，超限时从 `limit` 往回退到最近的 `is_char_boundary`，得到 ≤ 上限的最大合法前缀。
- `truncated=true` 时前端显示「已截断（只保留前缀）」的独立提示条。
- **二进制 body**（非 UTF-8）：不存原文，存 `"[binary, N bytes]"` 占位并置 `truncated=true`。MCP 是 JSON-RPC，理论上不会有，但 SSE 里可能混入非 UTF-8 分片。

### 3.4 冷存储（实测后否决）

原计划把超过 `call_body_hot_days` 的 body 归档到 Parquet。实测后不做：

- 复用 `body_archive::writer::write_parquet_shards` 不可行——它写死 `BodyRow`（LLM schema），塞 MCP 字段会把两种归档混进同一套文件，而 `query.rs` 的读取端按列名取 `messages`/`response`。要复用就得先把「行 → RecordBatch」抽出来，这是本 Stage 最贵的一块。
- 按生产量级（5 个 server、日常调用是低频事件、body 通常 < 2 KB），90 天热存储的成本可以忽略。

| | 冷存储 | 同寿命删除（**已选**） |
|---|---|---|
| 实现成本 | 高（新 schema + 复用改造 + 三方言 mark） | 低（保留期 worker 多删一张表） |
| 存储成本 | 低 | 高（90 天 × 全量 body） |
| 历史可查 | 90 天全可查 | 90 天内都可查（热存储） |

**因此 `mcp_call_body` 不加 `archived` / `parquet_path` 两列**，body 与日志同寿命删除；触发条件见 §8。

### 3.5 详情端点与前端

`GET /admin/mcp/calls/{call_id}/body`（admin only）：

```json
{"call_id": "...", "request": "...", "response": "...", "truncated": false}
```

- 不存在 → 404 `body_not_recorded`（可能是配置关闭、或 Stage 150 的保留期已删）
- 用户侧：`/v1/mcp/calls/{call_id}/body` **也开**，但只能看自己的行（`user_id` 相等）。理由：用户排查自己的调用是最常见场景，且 body 里是**他自己的**参数。管理员能看到所有人的。
- 前端抽屉：「请求 / 响应」两个 `Tabs`（与 playground 的 curl/SDK 切换同款），`<pre>` + 等宽字体 + 最大高度滚动。**不做复制按钮**：`<pre>` 里的文本本来就能选中复制，`Sheet` 里再加一个按钮只是多一处要测的状态。不引 JSON 高亮库（`package.json` 现无高亮依赖，为这一处引入不值）。
- 三种状态互不混淆：加载中是 `Skeleton`；有数据时按 tab 显示原文（`null` 或空串显示「（空）」）；请求失败时按状态码分成两句 —— 404 说「这次调用没有保存 body」（策略事实），其它说「加载 body 失败」（故障）。`truncated` 是恒在正文之上的独立琥珀色提示条，不是静默截断。

### 3.6 配置

```yaml
mcp_log_retention:
  call_log_days: 90
  rejection_days: 30
  record_bodies: true          # Stage 152
  max_body_kib: 64
```

`record_bodies: false` 时完全不写 `mcp_call_body`（详情端点返回 404）。**默认 true**：见 H2。

---

## 4. 测试

### 4.1 core UT（≥5）

1. `cut()`：ASCII 超限 → 前缀 + `truncated=true`，长度 ≤ 上限
2. `cut()`：UTF-8 多字节字符跨边界 → 不产生非法字符串（断言 `String::from_utf8` 成功、且不回退到 0 长度）
3. `cut()`：二进制 → `"[binary, N bytes]"` 占位 + `truncated=true`
4. `insert_mcp_call_body` / `get_mcp_call_body` 往返（SQLite 内存）
5. 保留期 worker 同时删 `mcp_call_log` 与 `mcp_call_body`（body 不会成为孤儿）

### 4.2 handler UT（≥6）

1. 本地应答（`initialize`）→ body 行存在，`request` 含 `params.cursor` 之类的原文，`response` 含 `result`
2. 缓冲转发（mock 上游）→ `response` 等于上游返回的字节
3. `tools/call` 带大参数（> 64 KiB）→ `truncated=true` 且 `response_body` 长度 ≤ 上限
4. `record_bodies=false` → 不写 body，详情端点 404
5. `/admin/mcp/calls/{id}/body` 存在 → 200；不存在 → 404
6. `/v1/mcp/calls/{id}/body` 看别人的 call_id → 404（**不是 403**：不泄露「这条存在」）
7. 认证失败（401）→ **不写 body**（与 Stage 150 的拒绝审计分工明确：拒绝只看身份，不看 body）

### 4.3 BDD

- mock：抽屉里「请求 / 响应」两个 tab 都能切换到并显示内容；`truncated` 时有提示；没有 body 时说「没有保存」而不是空白
- real BDD：`036` 迁移三方言可应用（`mcp_log_retention_real.feature` 的「036 迁移在本方言上建出调用 body 表」），且保留期把 `mcp_call_body` 与 `mcp_call_log` 一起删（同一 feature 的三个清场场景都断言了 body 表的过期行已清、新鲜行仍在）

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/036_mcp_call_body.sql` | 新增（`035` 被 Stage 150 占用） |
| `crates/aigw-core/src/models.rs` | `McpCallBodyRow` |
| `crates/aigw-core/src/db.rs` | insert / get / delete-expired（三方言） |
| `crates/aigw-core/src/mcp/body.rs` | 新增：`cut()` 截断工具 + UT |
| `crates/aigw-core/src/config.rs` | `mcp_log_retention.record_bodies` / `max_body_kib` |
| `crates/aigw-core/src/retention.rs` | 删除范围加 `mcp_call_body` |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | `finish` 加 `request: &[u8]` 参数 + 写 body；25 个调用点 |
| `crates/aigw-server/src/routes/mcp_relay.rs` | 新增 `record_with_bodies` / `body_limit`；`relay()` / `relay_buffered` / `relay_event_stream` 接请求体并写 body |
| `crates/aigw-server/src/routes/mcp_rest.rs` | 两个 handler 序列化请求并写 body（REST 入口没有原始字节可取） |
| `crates/aigw-server/src/routes/keys.rs` | `AppState.mcp_log_retention`（41 处字面量补默认值） |
| `crates/aigw-server/src/routes/mcp.rs` / `mcp_user.rs` | `GET .../body` 两个端点 |
| `crates/aigw-server/src/main.rs` | 两条路由 |
| `crates/aigw-frontend/src/pages/mcp/CallLogTab.tsx` | 抽屉加请求/响应两个 tab（`BodyViewer`） |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | `McpCallBody` |
| i18n ×2 + `resources.d.ts` | 新 key（`resources.d.ts` 由 `scripts/fe-i18n-types` 生成，手改无效） |
| `crates/aigw-frontend/tests/**` | feature + steps + mock 路由（`MCP_CALL_BODIES` 两条种子：一条完整、一条 `truncated`） |

---

## 6. 回归验证

- `task test`（全量，门禁只认这个）
- `task bdd` / `task bdd-real-sqlite`
- `task test-db-up` → `task bdd-real-pg` / `bdd-real-mysql` → `task test-db-down`
- `task fe-bdd` / `task fe-lint` / `task fe-build`
- `task fmt` / `task lint` / `task doctor`

手工：生产上发一次 `iwiki__metadata` 带错误参数 → 抽屉里能看到 `MCP error -32602 ... path [docid] Required` 原文。

---

## 7. 门禁

- [x] 一次成功调用与一次失败调用都能在抽屉里看到请求与响应原文 —— 成功/失败两侧各有 UT：`a_local_answer_records_both_halves`（本地应答）、`a_buffered_answer_is_recorded_byte_for_byte`（上游原文逐字节相等）；前端 BDD `A recorded call body is readable in two halves` 在抽屉里切两个 tab 断言原文
- [x] 超过上限的 body 被截断且 `truncated` 可见（前端有明确提示，不是静默截断）—— `an_oversized_answer_is_truncated_and_flagged`（`max_body_kib=1`，存 ≤ 1024 字节且 `truncated=true`）；前端 `mcp-call-body-truncated` 是独立的琥珀色提示条，BDD `A truncated body is flagged instead of being silently cut`
- [x] UTF-8 边界截断不产生非法字符串（UT 覆盖多字节字符跨边界）—— `crates/aigw-core/src/mcp/body.rs` 的 `a_multi_byte_character_straddling_the_limit_is_dropped` 与 `a_limit_that_lands_exactly_on_a_boundary_keeps_the_character`（回退到合法边界后 `String::from_utf8` 成功，且不为空）
- [x] `record_bodies=false` 时一行 body 都不写，端点 404 —— `recording_off_writes_no_body_row`（日志行仍在、body 行不存在）；`a_call_without_a_body_is_a_404_that_says_so`（`error.type == "body_not_recorded"`）
- [x] 普通用户看别人的 body 得 404（不泄露存在性）—— `someone_elses_call_body_is_a_404_not_a_403`（mcp_user）；`an_anonymous_call_belongs_to_nobody`、`a_key_without_a_user_cannot_read_a_body` 覆盖另外两个边界
- [x] 认证失败的请求不写 body（拒绝审计在 Stage 150 独立承担）—— `a_rejected_request_records_no_body`（401，且 `mcp_call_log` 里一行都没有）
- [x] 保留期删除后 `mcp_call_body` 无孤儿行 —— `the_tick_sweeps_bodies_with_the_log`（一次 tick 同时删两类，`bodies_deleted == 2`）与 `deletes_only_rows_past_the_window`（`(calls, bodies, rejections) == (2, 2, 2)`）；`the_body_store_finds_and_deletes_only_rows_past_the_cutoff` 钉死 SQL 层
- [x] `task test` / `task bdd` / `task fe-bdd` / `task fmt` / `task lint` / `task doctor` 全绿 —— 见 §7.1

### 7.1 门禁证据（最终树复跑）

- `task test`：22 个 `test result` 块全部 `ok`，`FAILED` / `panicked` 行 0，合计 **1716 passed / 0 failed / 13 ignored**，末行 `test passed`，EXIT=0（`/tmp/aigw-test-152g.log`）。内嵌 mock BDD：**51 features / 383 scenarios（370 passed, 13 skipped）/ 2254 steps（2241 passed, 13 skipped）**；13 个 skip 全是既有 `body_archive_read/write.feature` 的缺陷（TD-028），非本 Stage 引入。本 Stage 新增的三条 `db.rs` body 测试（`a_call_body_round_trips_with_every_column_intact` / `a_call_with_no_body_row_is_indistinguishable_from_a_missing_one` / `the_body_store_finds_and_deletes_only_rows_past_the_cutoff`）都在该次运行的 `ok` 列表里。
- `task fe-bdd`：**702 passed / 3 skipped / 0 failed / 0 flaky（27.7m）**，EXIT=0（`/tmp/aigw-fe-bdd-152c.log`），含本 Stage 的三条新场景（两个 tab 能看到原文 / 截断有提示 / 老调用说没有 body）在 desktop 与 mobile 两个视口下的 6 次执行。
- `task fmt` / `task lint` / `task doctor`：EXIT=0（`/tmp/aigw-fmt-152d.log`、`/tmp/aigw-lint-152f.log`、`/tmp/aigw-doctor-152b.log`）。
- `task bdd-real-sqlite`：**92 scenarios（92 passed）/ 506 steps（506 passed）**，EXIT=0（`/tmp/aigw-bdd-real-sqlite-152.log`）。`036` 三方言迁移的建表断言与「body 与日志同删」在 `mcp_log_retention_real.feature` 里（五条场景：035 与 036 各一条建表 + 一趟删净 + `batch_size=1` 删净 + `call_log_days=0` 时 body 与日志同留），五条全 ✔。
- `task bdd-real-pg` / `task bdd-real-mysql`：两者均 **92 scenarios（92 passed）/ 506 steps（506 passed）**，EXIT=0（`/tmp/aigw-bdd-real-pg-152.log`、`/tmp/aigw-bdd-real-mysql-152.log`），同一份 feature、`AIGW_TEST_DB_DRIVER` 切换方言，五条保留期场景在三方言上全 ✔。

---

## 8. 风险与遗留

- **存储增长** —— 这是本 Phase 唯一真的会长期增长的数据。默认 90 天 × 每调用 2 条 body。触发条件：`mcp_call_body` 超过 1 GB 或日增超过 50 MB 时，做 §3.4 的冷存储归档。
- **body 里含敏感业务数据** —— 工具参数可能是内网文档 id、代码片段、日志查询语句。与 `spend_logs.messages` 同级，访问控制一致（admin 看全部、用户看自己的）。**不做内容级脱敏**，这是明确的取舍而非遗漏。
- **SSE 拼接的 body 是截断的** —— 长流式响应只保留前 64 KiB，末尾的错误信息可能被切掉。缓解：`error_code` 仍然完整记录（它来自 HTTP 状态与 JSON-RPC error 对象，不依赖 body 留存）。
- **`finish` 签名改动影响 25 个调用点** —— 每处都要传请求体。漏传一处会得到「body 为空但不报错」，UT 已覆盖三个入口（`/mcp`、`/mcp/{alias}`、`/mcp-rest`）。
- **不做冷存储是显式取舍** —— 若将来做，`mcp_call_body` 需要加 `archived` / `parquet_path` 两列（迁移一次），且 `body_archive` 的 writer 要为 MCP 的 schema 做抽取重构。写进 ADR，避免下一个人以为是漏了。
