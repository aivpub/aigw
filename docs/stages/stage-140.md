# Stage 140: MCP Gateway 后端地基（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~14h（迁移 ×3 方言 + store ×3 后端 + 凭据解析纯函数 + 校验）
**依赖**: 无（Phase 55 首个 Stage）
**状态**: ✅ 完成（2026-10-29）

**提交**: `d61ef2c`（core：三方言迁移 029-032 + `mcp/*` 模块）→ `a5fad16`（Phase 55 收官回写），均 `--signoff`，未 push。

---

## 1. 问题

MCP 网关需要持久化四类事实：注册了哪些上游 server、每个 server 的凭据是什么、探测到的工具有哪些、谁调用了什么。这四类事实当前都不存在——仓库里只有全局的 `credentials` 加密桶（无 owner 维度），和 `proxies` / `web_search_providers` 这种单层资源。

本 Stage **只做数据层与纯逻辑**，不接任何 HTTP 路由。理由：凭据解析是整个功能里最需要被穷举测试的部分（三态 × 有无用户 token × 有无共享 token = 六种组合），把它做成纯函数可以在毫秒级红绿循环里跑完，不必等 handler 和 DB。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 上游凭据的方向是 **aigw → MCP server**，客户端带来的 `Authorization` 是 aigw 自己的 virtual key，绝不转发 | 若误把客户端 token 透传给上游，等于把网关 key 泄漏给每个上游 server（MCP 规范明确 MUST NOT） | 已在设计文档 §8 钉死；本 Stage 的 `resolve_outbound_token` **只接受 token 表的两行**，签名上就不给客户端凭据留位置 |
| H2 | `is_default` 的「同一 owner 至多一条」能用部分唯一索引在 SQLite / PostgreSQL 表达 | 若不能，只能退到应用层事务，并发下可能出现两条默认 | SQLite 支持 `CREATE UNIQUE INDEX ... WHERE is_default = 1`；PG 同。**MySQL 不支持**，退化为事务（§3.4） |
| H3 | `KeyIdentity.user_id` 在 master key 与未绑用户的 key 上是 `None` | 若它恒为 `Some`，`user_required` 的「无个人 token」分支会被误判成「有用户但没配 token」 | 读 `crates/aigw-core/src/middleware/mod.rs:36-45` + `spend.rs:146-157`（master key 构造时所有 id 为 `None`） |
| H4 | 已解密的 token 明文**永不落库**（存的是 `v2:gcm:` 密文） | 若是明文，任何一次 DB 读权限泄漏都等于上游凭据全泄 | 复用 `encrypt_litellm_value_gcm` / `decrypt_litellm_value`（`crypto.rs:178` / `:50`），写入路径强制加密 |
| H5 | 两张表能在三个方言上用同一套列名与语义表达 | 若某方言表达不了（如 MySQL 的部分索引），就要在 store 层分支 | 逐方言核对 DDL（§3.2 / §3.3），MySQL 的差异点只有 `is_default` 唯一性 |

---

## 2. 边界

**做**：
- `029_mcp_gateway.sql` × 三方言（`sqlite/` / `postgres/` / `mysql/`），首批 `mcp_servers` 与 `mcp_server_tokens` 两张表
- 本 Stage 只建**这两张表**；`mcp_tool_catalog` / `mcp_call_log` / `mcp_tool_allowlist` 由 141 / 143 / 144 各自的迁移追加（见 §9）
- `McpServerRow` / `McpServerTokenRow` model + 请求/响应 DTO
- `McpStore` trait + SQLite / PostgreSQL / MySQL 三份 impl + `Database` 分发
- 凭据解析纯函数 `resolve_outbound_token` + `TokenSource` 枚举
- `alias` 校验（唯一 / 禁 `__` / 禁下划线结尾 / 长度）
- token 加解密封装 + 脱敏
- `is_default` 的事务化置位（MySQL 必需，SQLite/PG 也可统一走它）

**不做**（明确排除）：
- **不写任何 HTTP handler** —— 141 负责
- **不建 catalog / allowlist / call_log 三表** —— 它们的消费者在 143 / 144，提前建表等于让 Stage 141 的探测写入一棵还没定形的树
- **不做上游探测** —— 141
- **不做并发控制 / 限流** —— 不在本 Phase 范围

---

## 3. 方案

### 3.1 迁移文件

`crates/aigw-core/migrations/{sqlite,postgres,mysql}/029_mcp_gateway.sql`，文件名三方言一致。

### 3.2 `mcp_servers`

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | 自增主键 | SQLite `INTEGER PRIMARY KEY AUTOINCREMENT` / PG `BIGSERIAL` / MySQL `BIGINT AUTO_INCREMENT` |
| `alias` | 文本，唯一 | 工具名前缀，**禁含 `__`、禁以下划线结尾**，长度 ≤ 32 |
| `display_name` | 文本 | 界面展示名 |
| `url` | 文本 | 上游 Streamable HTTP 端点 |
| `transport` | 文本，默认 `streamable_http` | 预留；MVP 只有这一个值 |
| `credential_scope` | 文本，默认 `shared_or_user` | `shared` / `shared_or_user` / `user_required` |
| `enabled` | 布尔，默认真 | MySQL `TINYINT(1)` |
| `protocol_version` | 文本，可空 | 探测协商到的版本，探测前为 `NULL` |
| `created_at` / `updated_at` | 文本，默认 `datetime('now')` | **MySQL 用 `VARCHAR(64)`**（sqlx 无法把 `DATETIME` 解成 `String`，`028_web_search_providers.sql` 已踩过） |

索引：`idx_mcp_servers_alias`（唯一）、`idx_mcp_servers_enabled`。

### 3.3 `mcp_server_tokens`

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | 自增主键 | 同 §3.2 |
| `server_id` | 外键 → `mcp_servers(id)` `ON DELETE CASCADE` | **MySQL 必须写表级 `FOREIGN KEY`**，列级 `REFERENCES` 被 InnoDB 忽略（`028` 已踩过） |
| `owner_type` | 文本 | `server` / `user` |
| `owner_id` | 文本，默认 `''` | `owner_type=server` 时为 `''`；`user` 时为 `user_id` |
| `token_name` | 文本 | 轮换时辨识，如 `svc-account-2026` |
| `token_value` | 文本 | `v2:gcm:` 密文 |
| `is_default` | 布尔，默认假 | 同 owner 至多一条为真 |
| `enabled` | 布尔，默认真 | 停用而不删，便于回滚 |
| `expires_at` | 文本，可空 | 过期后视同不存在 |
| `created_by` / `created_at` / `updated_at` | 文本 | |

唯一约束：`uq_mcp_server_tokens_owner (server_id, owner_type, owner_id, token_name)`。

默认 token 的唯一性：
```sql
-- SQLite / PostgreSQL
CREATE UNIQUE INDEX idx_mcp_server_tokens_default
    ON mcp_server_tokens(server_id, owner_type, owner_id)
    WHERE is_default = 1;
-- MySQL：无部分唯一索引，见 §3.4
```

索引：`idx_mcp_server_tokens_server (server_id)`。

### 3.4 `is_default` 的置位（MySQL 差异）

```rust
// 同一 owner 内先清零再置 1，包在一个事务里
async fn set_default_token(&self, server_id, owner_type, owner_id, token_id) -> Result<(), String>
```

- SQLite / PostgreSQL：部分唯一索引兜底，即使走到并发竞争也只会有一条成功，另一条报唯一冲突 → 转成 `409`。
- MySQL：靠事务 + 先 `UPDATE ... SET is_default = 0 WHERE server_id=? AND owner_type=? AND owner_id=?` 再置 1。并发下可能两个事务各置一条（MySQL 默认 REPEATABLE READ 下 `UPDATE ... WHERE` 会加行锁，实际能串行化，但**不依赖这个假设**——读侧一律 `ORDER BY is_default DESC, id DESC LIMIT 1`，即使真出现两条也只取一条）。

**统一走事务路径**，不按方言分支——SQLite/PG 多执行一次清零 UPDATE 的代价可以忽略，换来的是三方言行为一致、只有一处代码要测。

**ADR-044（2026-10-29）收紧**：清零与置位两条 UPDATE 都带 `(server_id, owner_type, owner_id)` 全作用域，置位后检查 `rows_affected()`，为 0 则回滚并返回 `NotFound`（否则「清空了 A 的默认、却给 B 的行打了默认」会留下两个 default）。同一决定里 `is_default` 收敛为单一写者：`create_mcp_token` 与 `update_mcp_token` 都不再写这一列，只能经 `set_default_mcp_token`。

### 3.5 凭据解析（本 Stage 的核心）

```rust
pub enum TokenSource { User, Server }

pub struct ResolvedToken {
    pub source: TokenSource,
    pub token_id: i64,
    pub expires_at: Option<String>,
}

pub fn resolve_outbound_token(
    scope: CredentialScope,
    user_token: Option<&McpServerTokenRow>,
    server_token: Option<&McpServerTokenRow>,
) -> Option<TokenSource>;
```

**签名里没有任何「客户端凭据」入参** —— 这是对 H1 的编译期保证，不是注释。

解析表（与设计文档 §8.3 一致）：

| `scope` | 用户 token 可用 | 共享 token 可用 | 结果 |
|---|:---:|:---:|---|
| `shared` | — | 有 | `Server` |
| `shared` | — | 无 | `None`（匿名出站） |
| `shared_or_user` | 有 | — | `User` |
| `shared_or_user` | 无 | 有 | `Server` |
| `shared_or_user` | 无 | 无 | `None` |
| `user_required` | 有 | — | `User` |
| `user_required` | 无 | 有 | `None`（**不回落**） |
| `user_required` | 无 | 无 | `None` |

「可用」= `enabled = 1` 且（`expires_at` 为 `NULL` 或晚于当前时间）。

**`user_required` 绝不回落共享 token** —— 这一档的存在意义就是「权限严格按人区分」，回落等于把它降级成 `shared_or_user`，是安全语义错误。

### 3.6 `alias` 校验

```rust
pub fn validate_alias(alias: &str) -> Result<(), &'static str>
```

拒绝：空串、长度 > 32、含 `__`、以下划线结尾、含 `__` 之外的字符白名单（`A-Za-z0-9_.-`）之外。

**为什么禁 `__`**：工具名形如 `{alias}__{tool}`，拆分只找**第一个** `__`。alias 自身含 `__` 会让 `a__b__tool` 有两种拆法（`a` + `b__tool`，或 `a__b` + `tool`），只能靠「注册时禁止」消除歧义。

**为什么禁下划线结尾**：`a___tool` 按第一个 `__` 拆是 `a` + `_tool`，看着对；但 `a__` + `tool` 也成立。同样靠注册时禁止消除。

---

## 4. 测试

### 4.1 凭据解析（≥6 UT，表驱动）

| # | 用例 | 期望 |
|---|---|---|
| 1 | `shared` + 共享 token | `Some(Server)` |
| 2 | `shared` + 无任何 token | `None` |
| 3 | `shared_or_user` + 两者都有 | `Some(User)` |
| 4 | `shared_or_user` + 只有共享 | `Some(Server)` |
| 5 | `shared_or_user` + 都没有 | `None` |
| 6 | `user_required` + 两者都有 | `Some(User)` |
| 7 | **`user_required` + 只有共享** | **`None`（不回落）** |
| 8 | `user_required` + 都没有 | `None` |
| 9 | token `enabled=0` | 视为不存在 |
| 10 | token `expires_at` 已过期 | 视为不存在 |

### 4.2 `alias` 校验（≥5 UT）

合法：`github` / `my-wiki` / `a.b`。非法：`a__b` / `a_` / `""` / 33 字符 / `a b`。

### 4.3 store（≥3 UT / 方言）

建 server → 建 server 级 token → 建两条 user 级 token → 置第二条为默认 → 断言第一条 `is_default=0` → 级联删除 server → 断言 token 全清。

### 4.4 BDD

本 Stage **不加 BDD 场景** —— 没有 HTTP 端点就没有可驱动的行为。BDD 从 141 起。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/sqlite/029_mcp_gateway.sql` | 新增 |
| `crates/aigw-core/migrations/postgres/029_mcp_gateway.sql` | 新增 |
| `crates/aigw-core/migrations/mysql/029_mcp_gateway.sql` | 新增 |
| `crates/aigw-core/src/models.rs` | `McpServerRow` / `McpServerTokenRow` / `CredentialScope` / 请求响应 DTO |
| `crates/aigw-core/src/mcp/mod.rs` | 新增模块 |
| `crates/aigw-core/src/mcp/credential.rs` | `resolve_outbound_token` / `TokenSource` / `seal_token` / `redact_token` |
| `crates/aigw-core/src/mcp/alias.rs` | `validate_alias` |
| `crates/aigw-core/src/db.rs` | `McpStore` trait + 三份 impl + `Database` 分发 |
| `crates/aigw-core/src/lib.rs` | 挂 `pub mod mcp;` |

---

## 6. 回归验证

- `task test` —— 全量单元测试，新增 UT 全绿且**无回归**
- `task test-db-up` + `task bdd-real-pg` / `task bdd-real-mysql` —— 三方言迁移必须实际应用成功（历史教训：`028` 迁移首跑在 PG/MySQL 上暴露 4 处方言类型缺陷，mock 层看不出来）
- `task fmt` / `task lint` / `task doctor`

---

## 7. 门禁

- [x] 三方言迁移在真实库应用成功（sqlite / pg / mysql 各一次）—— 029 在 SQLite / PG / MySQL 真库均应用成功；PG `/tmp/final-pg.log`、MySQL `/tmp/final-mysql.log` 各 19 features / 80 scenarios (80 passed) / 452 steps 全过（`@real_api` 场景走真库真 server）
- [x] 凭据解析 10 条 UT 全绿，含 `user_required` 不回落 —— `resolve_outbound_token_table`（`credential.rs:132`）是 13 条 case 的表驱动断言，其中 `user_required + shared only must not fall back`（`:196`）即不回落；另有 disabled / expired / 未来过期各两条
- [x] `resolve_outbound_token` 的签名不含任何客户端凭据入参 —— `credential.rs:64-68`：`(scope, user_token: Option<&McpServerTokenRow>, server_token: Option<&McpServerTokenRow>)`，三者都来自 DB 行，没有任何形参能落客户端 header
- [x] token 落库值为 `v2:gcm:` 密文（DB 直读断言）—— `mcp.rs:1801` `a_stored_token_is_sealed_not_plaintext` 直读 DB 行断言不含明文；`mcp_user.rs:469-473` 断言前缀 `v2:gcm:` 且不含明文，并 `unseal_token` 还原验证
- [x] `task test` 全量通过 —— `/tmp/stage143-test-final.log` 1497 passed / 0 failed / 22 个 test result（EXIT=0，Phase 55 收口后的全量复跑）
- [x] `task fmt` / `task lint` green —— `/tmp/stage143-lint.log` `lint passed`、`task fmt` `fmt passed`

---

## 8. 风险与遗留

- **MySQL 无部分唯一索引** —— 已在 §3.4 用「统一事务 + 读侧 `ORDER BY`」兜住，不按方言分支。若将来出现真并发竞争，读侧的 `LIMIT 1` 保证行为可预期。**2026-10-29 补强（ADR-044）**：提升语句本身也带上 `(server_id, owner_type, owner_id)` 作用域，跨 owner 的 token id 会被 `NotFound` 拒绝而不是静默变成第二个 default；`is_default` 收敛为单一写者（INSERT / UPDATE 都不写这一列）。
- **`expires_at` 存的是文本** —— 三个方言的「晚于当前时间」比较写法不同（SQLite `datetime()` / PG `::timestamptz` / MySQL `STR_TO_DATE`）。本 Stage 的选择是**在 Rust 侧比较**（读出后在内存里判），代价是每条 token 要多读一个字段，换来的是一处逻辑、三方言无分支。数据量是「每 server 每用户至多几条」，不值得为它写方言 SQL。
- **`transport` 列** 目前只有一个合法值。保留它是为了将来若要接旧版 HTTP+SSE 上游时不必再加迁移 —— 但若 Phase 56 之前仍只有一个值，应在技术债里登记「这是空列」。

---

## 9. 对后续 Stage 的影响

- **141** 依赖本 Stage 的 `McpStore` 与 `alias` 校验；141 追加 `030_mcp_tool_catalog.sql`。
- **143** 依赖 `resolve_outbound_token` 的返回值来决定出站 `Authorization`；143 追加 `031_mcp_call_log.sql`。
- **144** 依赖 `CredentialScope` 的三态语义与 `mcp_server_tokens` 的 `owner_type` 布局；144 追加 `032_mcp_tool_allowlist.sql`。
- 迁移拆成四批（029 / 030 / 031 / 032）而非一次建五表，是为了让每个 Stage 的迁移与其消费者同批落地——表在没有任何代码写它的时候就存在，是最容易长出「没人知道这列干嘛」的地方。
