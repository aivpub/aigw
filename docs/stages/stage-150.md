# Stage 150: MCP 认证拒绝审计 + 日志保留期（Phase 58）

**所属**: Phase 58（MCP 日志与审计完善）
**预估**: ~16h（新表 ×3 + 提取器审计 + 保留期 worker + 端点 + 前端 + 测试）
**依赖**: Stage 149（同一张日志页与同一套名称补全）
**状态**: ✅ 完成（2026-10-31）

---

## 1. 问题

### 1.1 401/403 在数据库里完全不可见

2026-10-31 生产实测：先取 `/admin/mcp/calls` 的 `total_count`，再连发 14 次必然失败的请求（7 次无 `Authorization`、7 次假 token），再取一次：

```
total_count: 51 → 51
```

一条都没进 `mcp_call_log`。原因在分流位置：401 由 `SpendAuth` 提取器（`crates/aigw-server/src/routes/spend.rs:107`）在 **handler 之前**抛出，`post_mcp` 第一句 `call_row(...)` 根本没执行。连 `x-call-id` 响应头都没有（响应只有 `content-type` / `date`）。

现状账目：

| 事件 | `mcp_call_log` |
|---|---|
| 带合法凭据的调用（成功/失败） | ✅ |
| 别名不存在（404） | ✅（`alias=nonexistent-alias status=failed`） |
| **无 token / 假 token（401）** | ❌ |
| **被 block 的 key（403）** | ❌ |

后果：未授权的探测尝试只能去翻服务器的 tracing 日志，且没有 IP、没有频次、没有可查询的时间窗口。MCP 端点是公网可达的（`9.135.87.221:4001`），「有没有人在扫」这个问题目前**无法回答**。

### 1.2 `mcp_call_log` 无保留期，会无界增长

`docs/12-technical-debt.md` TD-030 已登记：日志每调用一行，没有任何清理路径。单机 SQLite 上它迟早变成最大的表。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 拒绝审计必须是**独立表**，不能塞进 `mcp_call_log` | `mcp_call_log` 的 `key_id`/`user_id` 语义是「谁在调用」，被拒绝的请求恰恰没有身份；强行塞 NULL 会让 `/v1/mcp/calls` 的等值过滤与已有 BDD 全部失真 | 表结构对比 + 现有查询不改 |
| H2 | 记录「呈现的 token 的哈希」是安全的 | 若某个用户把真 key 误发到错误端点，日志里出现该 key 的哈希 → 管理员能对应到具体 key。这是**想要的**（这正是排查「谁发错了」的手段），且哈希不可逆、key 高熵 | 写入的是 `hash_token`，与 `virtual_keys.token` 同格式；不存明文、不存前缀以外的任何片段 |
| H3 | 只有在 MCP 路径上拒绝才值得落库 | 全路由落库会把 chat/keys 的所有 401 也写进来，量级与噪声都失控 | 提取器只在 `/mcp`、`/mcp/{alias}`、`/mcp-rest/*` 上使用 |
| H4 | 保留期用配置而不是硬编码 | 不同部署的合规要求不同；硬编码值要么太短（查不到）要么太长（无界） | 两个 config key + 默认值；默认值写进 `config.yaml` 注释 |
| H5 | 清理走既有的 `AsyncTask` 引擎，不新建调度 | 再写一套 cron 就会有两套时钟、两套取消语义 | 与 `BodyArchiver` / `BudgetResetter` 同一注册点（`main.rs:464`） |

---

## 2. 边界

**做**：
- 新表 `mcp_auth_rejection`（三方言）：`id`（UUID v7 文本 PK）、`created_at`、`path`、`http_method`、`reason`（`missing_header` / `invalid_format` / `invalid_key`）、`token_hash`（可空；无头时为 NULL）、`client_ip`（可空）、`user_agent`（可空，截断 512）
- 新提取器 `McpAuditAuth`（`mcp_gateway` 内），包装 `SpendAuth`：失败时记录一行**再**返回同样的 `AuthError`
- `GET /admin/mcp/rejections`（admin only）：时间范围 + reason 过滤 + 分页，信封与 `/admin/mcp/calls` 一致
- 保留期 worker `McpLogRetention`（`AsyncTask`）：按配置删 `mcp_call_log` 与 `mcp_auth_rejection` 的过期行，分批删
- 配置：`mcp_log_retention: { call_log_days: 90, rejection_days: 30, batch_size: 5000, check_interval_secs: 3600 }`（`0` = 不删）
- 前端：`/dash/mcp` 第 4 个 tab「认证拒绝」（admin only），列 = 时间 / reason / 路径 / key 指纹（前 8 位）/ IP
- TD-030 标记 Resolved

**不做**（明确排除）：
- **不做告警/封禁**（阈值告警、按 IP 限流是另一个域；本 Stage 只让数据可见）
- **不记录请求体**（被拒绝的请求体没有排查价值，且有被用来塞垃圾的风险）
- **不给普通用户看**（没有身份可归属，只有 admin 有查询意义）
- **不做 `mcp_call_log` 的归档到冷存储**（保留期内删掉就是删掉；要长期留存得走 body archive 那套，另议）
- **不统一 `TokenExpired` / `TokenBlocked`**（`SpendAuth` 目前根本不产生这两个变体，见 `middleware/mod.rs:56`；本 Stage 只忠实记录实际可达的三种）

---

## 3. 方案

### 3.1 表

```sql
CREATE TABLE IF NOT EXISTS mcp_auth_rejection (
    id           TEXT PRIMARY KEY,
    created_at   TEXT NOT NULL,
    path         TEXT NOT NULL,
    http_method  TEXT NOT NULL,
    reason       TEXT NOT NULL,
    token_hash   TEXT,
    client_ip    TEXT,
    user_agent   TEXT
);
CREATE INDEX IF NOT EXISTS idx_mcp_auth_rejection_created ON mcp_auth_rejection(created_at);
```

与 `mcp_call_log` 一样**不建外键**、不存凭据，只存 hash。`path` 存原始请求路径（含 alias），便于回答「哪个 server 被扫」。

### 3.2 提取器而不是中间件

中间件（`from_fn_with_state` + `route_layer`）也能做，但要重新解析 Authorization 头、重新判断「这个请求最终会不会被 handler 拒绝」——那等于把 `SpendAuth` 的逻辑复制一份。

提取器方案的形状：

```rust
pub struct McpAuditAuth(pub KeyIdentity);

impl<S> FromRequestParts<S> for McpAuditAuth
where S: Send + Sync, SharedState: FromRef<S>
{
    type Rejection = AuthError;
    async fn from_request_parts(parts, state) -> Result<Self, AuthError> {
        let state: SharedState = FromRef::from_ref(state);
        match SpendAuth::from_request_parts(parts, &state).await {
            Ok(SpendAuth(identity)) => Ok(McpAuditAuth(identity)),
            Err(e) => {
                record_rejection(&state, parts, &e).await;   // best-effort
                Err(e)
            }
        }
    }
}
```

落点：`post_mcp`、`post_mcp_aggregate`、`get_mcp`、`delete_mcp`、`mcp_rest::tools_list`、`mcp_rest::tools_call`、`mcp_rest::health` 的提取器类型从 `SpendAuth` 换成 `McpAuditAuth`。

**记录失败不得掩盖原错误**：`record_rejection` 内部吞掉 DB 错误并 `warn!`，与 `mcp_relay::record` 的既有约定一致（审计写不进去不能把 401 变成 500）。

**body 未被消费**：`FromRequestParts` 不碰 body，所以把它放在 `Bytes` 提取器之前是安全的。

**顺带修掉 `SpendAuth` 的错误来源**：原实现在「头里有 token」但校验失败时，会继续尝试 cookie JWT，并把**cookie 那次**的错误返回（cookie 路径必然也是失败，因为它根本没有 cookie）；于是带着一把假 token 的请求被记成 `missing_header`，审计账上「没人试过」与「有人猜错了」变成同一行。改成两条路径都失败时返回**头路径**的错误（它与请求实际携带的凭据对应），审计才记得对。改动在 `spend.rs` 的 `from_request_parts`，`reason` 的三种取值不变。

### 3.3 client_ip

复用 `routes/ip_extractor.rs:19` 的 `OptionalClientIp`（`RightmostXForwardedFor`）。提取器里可以直接读 `parts.headers` 的 `x-forwarded-for`；但既有提取器已经封装了「取最右一跳」的规则，不要在审计路径里另写一套解析——**同一份数据两个解析器迟早不一致**。因此在 `record_rejection` 里调用同一模块暴露的解析函数（若目前只是 axum extractor trait，则把解析逻辑提成 `pub(crate) fn parse_client_ip(&HeaderMap) -> Option<String>`，extractor 改为调用它）。

### 3.4 保留期 worker

```rust
impl AsyncTask for McpLogRetention {
    fn step_type(&self) -> &'static str { "mcp_log_retention" }
    fn tick_interval(&self) -> Duration { self.config.check_interval_secs }
    async fn tick(&self, db) -> Result<Option<Vec<NewStep>>> {
        // 每小时一个 step，payload 带两个 cutoff；无过期行则 None
    }
    async fn execute(&self, db, step) -> Result<StepOutput> {
        // 分批 DELETE ... WHERE created_at < cutoff LIMIT batch_size
        // 循环直到删不动或达到单步上限，返回 {"calls_deleted":n,"rejections_deleted":m}
    }
}
```

- **分批删**：一次 `DELETE ... LIMIT` 在 SQLite/PG/MySQL 三种方言的支持不同（PG 没有 `LIMIT`）。用「先 `SELECT id ... LIMIT n` 再 `DELETE WHERE id IN (...)`」，三方言通用。
- **`0` = 不删**：显式语义，避免「配置写成 0 就删光」。
- **`created_at` 是 TEXT（RFC 3339）**：与既有 `from`/`to` 过滤一致，字符串比较安全（统一 UTC、`+00:00` 后缀）。不改成 TIMESTAMP——SQLite 上没有收益，且会让 Stage 147 的时间过滤换实现。
- 注册在 `main.rs:464` 之后；`mcp_log_retention` 未配置时用默认值（不关闭），**与 `body_archive.auto_archive` 默认关闭的做法相反**：删数据是危险的，所以默认值必须是保守的长周期（90/30 天），而不是「默认打开删除」。

### 3.5 端点与前端

`GET /admin/mcp/rejections?reason=&from=&to=&page=&page_size=` → `{object,data,count,total_count,page,page_size,total_pages}`，与 `call_log_response` 同一信封。`reason` 是等值枚举。

前端 tab：

```
/dash/mcp（admin） [Server 管理] [我的凭据] [调用日志] [认证拒绝]
                      （非 admin）[我的凭据] [调用日志]
```

四 tab 在 320px 下会挤压（Stage 145 踩过一次 328px > 320px）。改为：**认证拒绝并入「调用日志」页的一个筛选**？不 —— 两张表的行结构不同（拒绝行没有 alias 之外的字段），混排会让列定义变成并集。

取第三种：tab 标签用短词（「拒绝」）并把 `TabsList` 改为 `flex-wrap`，320px 下折成两行。门禁里必须有 320px 断言。

---

## 4. 测试

### 4.1 core UT（≥5）

`crates/aivpub/aigw-core/src/db.rs`（SQLite 内存库）：
1. 插入 → `list_mcp_rejections` 按时间倒序返回，信封字段齐全
2. `reason` 过滤生效；`from`/`to` 边界闭区间
3. 保留期删除：造 3 行（2 行过期、1 行新鲜）→ 只删 2 行，返回删除计数
4. `batch_size=1` 时多轮删除：5 行过期、batch 1 → 全部删完（**证明循环而不是只删一批**）
5. `days=0` → 一行都不删

### 4.2 handler UT（≥5）

1. 无 `Authorization` 调 `/mcp/iwiki` → 401，且 `mcp_auth_rejection` 多一行，`reason=missing_header`
2. 假 token → 401 `reason=invalid_key`，`token_hash` 等于该假 token 的 SHA256
3. `Authorization: Basic xxx` → 401 `reason=invalid_format`
4. 合法 key → 200，**不**产生拒绝行
5. DB 写失败（用一个不可写的库构造）不影响 401 返回码（审计失败不掩盖原错误）
6. `/mcp-rest/tools/list` 无凭据 → 同样落一行（**证明三个入口都接上了**）

### 4.3 BDD / real BDD

- mock BDD ≥2 场景 × 3 viewport：admin 能看到「认证拒绝」tab 与行；非 admin 看不到该 tab
- real BDD（三方言）≥2 场景：`035` 迁移可应用、索引存在；保留期 worker 对三方言各删一次（PG 无 `DELETE LIMIT`，这条是重点）

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/035_mcp_auth_rejection.sql` | 新增（`034` 已被 Stage 147 占用，`035` 本 Stage） |
| `crates/aigw-core/src/models.rs` | `McpAuthRejectionRow` |
| `crates/aigw-core/src/db.rs` | insert / list / count / delete-expired（三方言） |
| `crates/aigw-core/src/retention.rs` | 新增：`McpLogRetention`（`AsyncTask`）+ `McpLogRetentionConfig` |
| `crates/aigw-core/src/config.rs` | `mcp_log_retention` |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | `McpAuditAuth` 提取器 + 六个 handler 换用 |
| `crates/aigw-server/src/routes/mcp.rs` | `list_mcp_rejections` handler |
| `crates/aigw-server/src/routes/ip_extractor.rs` | 提取 `parse_client_ip` |
| `crates/aigw-server/src/main.rs` | 路由 `/admin/mcp/rejections`；注册 retention worker |
| `crates/aigw-frontend/src/pages/mcp/RejectionsTab.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | 第 4 个 tab（admin only）+ `flex-wrap` |
| i18n ×2 + `resources.d.ts` | 新 key |
| `crates/aigw-frontend/tests/**` | feature + steps + mock 路由 |

---

## 6. 回归验证

- `task test`（全量，门禁只认这个）
- `task bdd` / `task bdd-real-sqlite`
- `task test-db-up` → `task bdd-real-pg` / `bdd-real-mysql` → `task test-db-down`
- `task fe-bdd` / `task fe-lint` / `task fe-build`
- `task fmt` / `task lint` / `task doctor`

手工：生产上发 3 次无凭据请求 → `/admin/mcp/rejections` 应见 3 行且 IP 正确。

---

## 7. 门禁

- [x] 401 之后能看到一行拒绝记录（三个 MCP 入口各有 UT）—— `every_mcp_entry_point_records_its_own_rejection`（`/mcp`、`/mcp/{alias}`、`/mcp-rest/*` 三条路径各断言 401 + 一行），单入口细节见 `a_missing_authorization_leaves_a_rejection_row`；子路径 `/mcp/{alias}` 走 `McpAuditAuth`（`mcp_gateway.rs:502`）、`mcp_rest.rs` 三个 handler 全部换掉 `SpendAuth`
- [x] 记录的是 token 的哈希，**不是**明文；无头时 `token_hash` 为 NULL —— `a_bogus_token_is_recorded_as_a_hash_never_the_token`（同时断言序列化结果里不含明文）、`a_missing_authorization_leaves_a_rejection_row`（`token_hash == None`）、`a_non_bearer_authorization_is_recorded_as_invalid_format`（无 Bearer 可哈希 → None）
- [x] 审计写失败不影响 401 的返回码与响应体 —— `a_broken_audit_table_still_answers_401`（断表后仍是 401，且 `error.type == "auth_error"`）
- [x] `reason` 三种取值各有 UT（`missing_header` / `invalid_format` / `invalid_key`）—— 三条分别在 `a_missing_authorization_leaves_a_rejection_row`、`a_non_bearer_authorization_is_recorded_as_invalid_format`、`a_bogus_token_is_recorded_as_a_hash_never_the_token`；`rejection_reason`（`mcp_gateway.rs:77`）的 match 是全量的，`TokenExpired`/`TokenBlocked` 归 `invalid_key`
- [x] 保留期 worker 三方言都能删（PG 上没有 `DELETE ... LIMIT`）—— `stale_ids_{sqlite,mysql,pg}` 都是 `SELECT key ... ORDER BY created_at LIMIT ?`，再 `delete_ids_*` 按 id 列表删；三方言共用同一个「先选后删」形状，不依赖 `DELETE ... LIMIT`
- [x] `days=0` 不删任何行 —— `zero_days_means_never_delete`（两个配置都为 0 时 `tick` 返回 `None`，即不排任务而不是把全部行判过期）；`a_configured_zero_leaves_that_table_alone` 钉死「只关一张表」
- [x] `mcp_call_log` 也受保留期约束（TD-030 Resolved）—— `deletes_only_rows_past_the_window`（两张表各删 2 行、各留 1 行）、`a_small_batch_still_drains_the_whole_backlog`（`batch_size=1` 删完 5 行积压）；TD-030 已标 Resolved
- [x] 320px 下四个 tab 不横向溢出 —— `TabsList` 两个分支都是 `h-auto min-h-9 max-w-full flex-wrap`（`mcp/index.tsx:298`/`:327`），四个 tab 走同一套；`task fe-bdd` 的 320px 视口全绿见 §7.1
- [x] `task test` 全量通过 —— 见 §7.1

### 7.1 门禁证据（最终树复跑）

- `task test`：22 个 `test result` 块全部 `ok`，`FAILED` 行 0，合计 **1683 passed / 0 failed / 13 ignored**，末行 `test passed`，EXIT=0（`/tmp/aigw-test-150f.log`）。内嵌 mock BDD：**51 features / 382 scenarios（369 passed, 13 skipped）/ 2252 steps（2239 passed, 13 skipped）**；13 个 skip 全是既有 `body_archive_read/write.feature` 的缺陷（TD-028）。
- `task bdd-real-pg`：**22 features / 91 scenarios（90 passed, 1 failed）/ 500 steps（499 passed, 1 failed）**（`/tmp/aigw-bdd-real-pg-150b.log`）。唯一失败是 `real_mcp_aggregate_steps.rs:292` 的 `hyper::Error(IncompleteMessage)` —— `tracing-subscriber` span-guard flake（TD-028），日志里紧邻两行 `sharded.rs` panic，与本 Stage 无关（MCP 代码零 span guard）。本 Stage 新增的四条保留期 real 场景（035 建表、一批删净、`batch_size=1` 删净积压、`call_log_days=0` 只删拒绝表）全部 ✔。**Stage 152 之后**该 feature 扩成五条（多了 036 建表；三个清场场景的断言从两张表扩到 `mcp_call_body`），数字以 `stage-152.md` §7.1 为准。
- `task fmt` / `task lint` / `task doctor` 均 EXIT=0（`/tmp/aigw-fmt-150.log`、`/tmp/aigw-lint-150.log`、`/tmp/aigw-doctor-150.log`）。
- 前端门禁（`task fe-bdd` / `fe-lint` / `fe-build`）在 Stage 151 的最终树上统一复跑，数字记在 `stage-151.md` §7.1 —— Stage 151 改的是同一批 tab/日志页文件，分开跑的两份数字里只有后一份对最终树有效。
- `bdd-real-sqlite` 在本 Stage 的树上未单独跑（`bdd-real-pg` 与 mock `task bdd` 已覆盖同一套 SQL 构造）；最终树上补跑，数字见下一行。
- 上述 `task test` 的 1683 是本 Stage 那棵树上的数字；Stage 152 又加了 body 相关的 UT，最终树的合计是 **1716 passed / 0 failed / 13 ignored**（`stage-152.md` §7.1）。
- **Stage 152 之后**三方言 real BDD（含本 Stage 的 `035` 建表断言与本 feature 扩成的五条保留期场景）在最终树上各跑一次：**sqlite / pg / mysql 均 92 scenarios（92 passed）/ 506 steps（506 passed）**，EXIT=0（`/tmp/aigw-bdd-real-sqlite-152.log`、`/tmp/aigw-bdd-real-pg-152.log`、`/tmp/aigw-bdd-real-mysql-152.log`）。

---

## 8. 风险与遗留

- **`token_hash` 让「谁发错了 key」可被追溯** —— 这是特性也是隐私面：管理员能看到某把 key 被用于错误端点。仅 admin 可见，且不存明文。
- **拒绝行会被扫描器灌满** —— 公网端点每小时的随机扫描可能产生大量行。缓解：保留期 30 天 + 分批删；若量级失控，下一步是「同 IP 同 reason 在 N 分钟内合并计数」，那是新的聚合表，本 Stage 不做。
- **`client_ip` 依赖 `X-Forwarded-For`** —— 直连时为空；有反代时取最右一跳（既有规则）。伪造 `X-Forwarded-For` 的请求会写下假 IP，因此该列**不能作为封禁依据**。
- **保留期删除是不可逆的** —— 配置写小会丢历史。README/config 注释必须写明「删除后不可恢复，长期留存请先归档」。
- **`reason` 是自由字符串（DB 无 CHECK 约束）** —— 与 `token_owner` / `status` 的既有做法一致，靠写入侧常量与 UT 保证取值。
