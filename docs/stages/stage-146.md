# Stage 146: 自定义认证头名与额外上游请求头（Phase 56）

**所属**: Phase 56（MCP Gateway 生产反馈迭代）
**预估**: ~16h（迁移 ×3 + store ×3 + 出站注入四处 + 探测 + 管理 API + 前端 ServerDialog + BDD）
**依赖**: Stage 140（`mcp_servers` 表与 `McpServerRow`）、Stage 141（probe / reload）、Stage 143（`session::UpstreamTarget` / `request_builder`）、Stage 142（`ServerDialog`）
**状态**: ✅ 完成（2026-10-31）

**提交**: —

---

## 1. 问题

2026-10-30 在生产网关（9.135.87.221:4001）逐个接本地 Claude Code 的 MCP server 时，**两个上游接不进来，原因都是鉴权头不是 `Authorization: Bearer`**：

- **TAPD**（`https://mcpgw.knot.woa.com/tapd/`）：要 `X-Tapd-Access-Token: <token>`。给 Bearer 得 `401 token invalid`。它的 OAuth access token 另有 24h 上限（`token expired:maxAlive-86400`），本身就当不了长期凭据。
- **工蜂**（`https://git.woa.com/api/mcp/mcp`）：`initialize` 用 Bearer 能过，`tools/call` 返回 `Unauthorized`；实测要 `PRIVATE-TOKEN: <token>`。这类「握手能过、调用不过」的失败最费排查时间——探测与真实调用失败在不同阶段。

网关当前出站只注入一个头：`crates/aigw-core/src/mcp/session.rs:227-229` 的 `req.bearer_auth(bearer)`。这一行把「接入任意 Streamable-HTTP MCP 上游」压成了「接入支持 Bearer 的上游」。同类需求还有**不鉴权但要求固定业务头**的上游（`X-Project-Id` / `X-Tenant` 之类），它们现在只能靠给 URL 加 query 参数绕，而多数上游不认。

**已核实的现状**（本次改动的事实基础）：

- 出站构造点**四处**，全部要改：
  1. `session.rs:94` `pub struct UpstreamTarget<'a> { url, bearer, stack, timeout }`（4 字段，非 `Copy`），`:217` `fn request_builder(target, protocol_version, session)`。
  2. `session.rs:322` `pub async fn reachable(url, bearer, timeout)` —— 自己拼一个 ad-hoc `UpstreamTarget`（`/mcp-rest/health` 用）。
  3. `probe.rs:444` `pub async fn probe_server(url, bearer, other_server_tool_names)` 与 `:366` `struct Session { client, url, bearer, protocol_version, session_id, next_id }`，`:379` `fn build_request` 里 `req.bearer_auth(bearer)`。**探测走的是另一套 builder，与 session.rs 不共用**——这正是下面 §3.3 要求统一的原因。
  4. `crates/aigw-server/src/routes/mcp_gateway.rs:304` `pub(crate) fn target(ctx: &CallContext) -> UpstreamTarget`（唯一的正式调用点）。
- `McpServerRow`（`models.rs:1347`）字段：`id, alias, display_name, url, transport, credential_scope, enabled, protocol_version, tool_count(#[sqlx(default)]), created_at, updated_at`。**结构体字面量 9 处**（`models.rs`、`db.rs:12206`、`routes/mcp.rs:324`、`routes/mcp_user.rs:413`、`routes/mcp_gateway.rs:1053`、`tests/bdd_steps/mcp_aggregate_steps.rs:227`、`mcp_passthrough_steps.rs:201` 等）——加字段会全部编译失败，属已知成本，不绕开。
- SQL 常量按方言各一组（`UPDATE` 是**整行 SET**）：`CREATE/GET/GET_BY_ALIAS/LIST/UPDATE_MCP_SERVER_{SQLITE,MYSQL,PG}` 三簇（`db.rs:6355`/`:6793`/`:7187` 附近），另有 `MCP_SERVER_PROJECTION`（`db.rs:6398`）与 `MCP_SERVER_GROUP_BY`（`:6404`）两段带 `s.` 前缀的拼接串。加列要同步改三方言的 SET/INSERT/SELECT + 这两段 + `Database` 分发（`db.rs:6449`/`:6897`/`:7260`）。
- 迁移已到 `032_mcp_tool_allowlist.sql`；本 Stage 用 `033_mcp_server_headers.sql` × 三方言。MySQL 注意：TEXT 不能带 DEFAULT；`created_at` 类是 `VARCHAR(64)`、Rust 侧 `String`；InnoDB 忽略列级 `REFERENCES`（029/030/032 头注释已记录）。
- 加密：`mcp/credential.rs:84` `seal_token(plaintext, master_key) -> Result<String, String>` / `:88` `unseal_token`，封装 `crypto::encrypt_litellm_value_gcm`，产出 `v2:gcm:` 前缀密文；`mcp_server_tokens.token_value` 就是这么落库的。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 「一个自定义头名 + 一个值」这一自由度覆盖真实上游 | 需要 HMAC 签名 / 多头联动的上游仍接不进来，只能等后续 Stage | 生产上 TAPD、工蜂两个真实阻塞点都恰好是「单头裸 token」形态 |
| H2 | `extra_headers` 整块加密够用，不需要逐条加密 | 逐条加密要处理「哪条是机密」的判断 + 单条解密失败的局部降级，复杂度换不到收益 | 额外头数量在个位数，改一次就整块重写一次，与 ACL 整表替换同一成本量级 |
| H3 | 探测路径能作为配置正确性的判据 | 配错了要等真实用户调用才发现，而 `tools/call` 阶段的 401 会被记成上游错误，归因困难 | 探测与运行共用同一个 `UpstreamAuth`（§3.3），且同样发 `initialize` + `tools/list` |
| H4 | 管理 API 把值置 `***` 不破坏既有 UI | `ServerDialog` 编辑时若把 `***` 当值回填并提交，会把自己的密文覆盖成三个星号 | 前端「值」输入框只读 placeholder、不预填；UT 断言 GET 响应不含明文（§4.2） |
| H5 | 受保护头黑名单不会误伤真实需求 | 某个上游确实要求自定义 `Accept` 时会被网关 400 挡住 | 黑名单只含协议必需头；误伤时放宽黑名单，但 `Mcp-Session-Id` / `MCP-Protocol-Version` 永不放宽 |
| H6 | 头值不落明文（与 token 同一套 `seal_token`） | 任何一次 DB 读权限泄漏都等于所有上游凭据全泄 | real BDD 直读库断言 `v2:gcm:` 前缀且不含明文，同 Stage 144 `mcp_user.rs:461` 手法 |

---

## 2. 边界

**做**：
- `mcp_servers` 三列：`auth_header_name` / `auth_header_value`（密文）/ `extra_headers`（整块密文 JSON）
- `033_mcp_server_headers.sql` × 三方言 + 三份 store 的读写与 `Database` 分发
- 出站注入改造四处：`UpstreamTarget` + `request_builder` + `reachable` + `target()`，以及 `probe.rs` 的 `Session` / `build_request` / `probe_server`
- 头名校验与受保护头黑名单（新模块 `mcp/headers.rs`）
- `POST/PUT /admin/mcp/servers` 接受三个新字段；`GET` 响应脱敏
- `ServerDialog` 三个新控件 + i18n ×2
- BDD：mock（断言上游实际收到的头）+ real（三方言迁移与密文落库）

**不做**（明确排除）：
- **不做自定义请求方法 / 路径** —— 上游端点就是 `url` 一整串，拆成 base+path 是另一种建模，需求没有
- **不做 query 参数注入** —— 与头注入是两条不同的注入面，且 query 值进日志/URL 更易泄漏
- **不做 HMAC / 签名类鉴权** —— 要按 body 与时间戳算签名，得把「请求体」交给签名器，与现在的 `request_builder` 结构不兼容
- **不做按用户区分的头** —— 那是 `mcp_server_tokens` 的语义（`owner_type=user`），不属于 server 级静态配置
- **不做响应头匹配 / 校验** —— 本期只关心「发出去的能不能被上游接受」
- **不做 OAuth 发现** —— Phase 57

---

## 3. 方案

### 3.1 存法：加三列，不建子表

| 列 | 类型 | 语义 |
|---|---|---|
| `auth_header_name` | `TEXT NULL` | 空 / `NULL` = 现行为（`Authorization: Bearer`） |
| `auth_header_value` | `TEXT NULL` | **密文**，`seal_token` 落库，注入前 `unseal_token` |
| `extra_headers` | `TEXT NULL` | JSON 文本 `[{"name":"...","value":"..."}]`，**整块密文** |

- `auth_header_value` 之所以**不复用 `mcp_server_tokens`**：那张表是「按 owner 多维」的凭据桶（server / user × 名字 × 默认 × 过期），而认证头名是 server 级的**单一静态配置**。塞进 token 表要么多一个 `owner_type`，要么借 `token_name` 表达「这是头值」——两种都是给既有语义加包袱。
- `extra_headers` **整块加密**（一次 `seal_token(&serde_json::to_string(&v)?)`）：额外头里大概率有 `X-Api-Key` 一类机密，而「哪些是机密」在注册时判断不了，逐条加密要引入「本条是否加密」的标记位，不如统一加密。代价见 §8。
- **反方案「建 `mcp_server_headers` 子表」不选**：额外头的编辑单位天然是「这个 server 当前的头顶集合」，与 Stage 144 ACL 的整表替换同一语义。子表要 4 个端点（list/create/update/delete 或 replace）+ 排序 + 级联，换来的唯一好处是「能单独查某个头」——没有这个需求。
- **列类型取 `TEXT` 而非 `VARCHAR(n)`**：值长度是密文长度（`v2:gcm:` + base64，约 1.4× 明文），写死宽度会在加密后溢出。长度上限放在 Rust 侧校验（单值 ≤ 4096 字符、`extra_headers` ≤ 32 项且序列化后 ≤ 8192 字符），三方言行为一致。

**值永不出网关**：
- `GET /admin/mcp/servers` 与 `GET /admin/mcp/servers/{id}`：`auth_header_name` 明文回显（它是配置，得让人看见自己在配什么）；`auth_header_value` 已配置回 `"***"`、未配置回 `null`；`extra_headers[].value` 恒 `"***"`（`name` 明文）。
- **LIST 解密（与规划时的判断相反，2026-10-31 修正）**：原方案写的是「列表页不需要值，不解密」，实现时发现列表行正是编辑对话框的入口 —— 不解密就只有一个头名、没有值，`extra_headers` 会显示成空列表，下一次保存就把用户配的头全删了（`extra_headers` 是整块替换）。所以列表也解密，只用于取出**名字**，值在 `server_wire` 里一律换成 `***`。代价是每行一次 `unseal_token`（AES-GCM，微秒级）；分页上限 100 行，可接受。复用的是同一个 `decrypt_litellm_value`，`v2:gcm:` 走 GCM 分支；若历史行是明文/NaCl 形态会落到 PBKDF2（600k 次）那条慢路径 —— 与 token 表同一既有行为，不额外处理。
- 编辑语义：`PUT` 时 `auth_header_value` / `extra_headers` **缺省 = 不改**，空串 / 空数组 = **清除**。前端「值」输入框 `type="password"`、编辑时 placeholder 写「留空则不修改」、旁边显示 `***`——与 `MyTokenDialog`「只能替换不能编辑」同一语义。

### 3.2 注入：一个头名 + 一个值，`UpstreamAuth` 枚举

```rust
pub enum UpstreamAuth<'a> {
    Bearer(&'a str),
    Header { name: &'a str, value: &'a str },
}

pub struct UpstreamTarget<'a> {
    pub url: &'a str,
    pub auth: Option<UpstreamAuth<'a>>,
    pub extra_headers: &'a [(String, String)],
    pub stack: UpstreamStack,
    pub timeout: Duration,
}
```

`request_builder` 里：

```rust
match target.auth {
    Some(UpstreamAuth::Bearer(t)) => req = req.bearer_auth(t),
    Some(UpstreamAuth::Header { name, value }) => req = req.header(name, value),
    None => {}
}
for (name, value) in target.extra_headers {
    req = req.header(name, value);
}
```

**为什么不把 Bearer 也压成 `Header { "Authorization", "Bearer …" }`**：`bearer_auth` 会做 `reqwest` 侧的头值合法化（非 ASCII、含空白时的处理），裸 `header("Authorization", format!("Bearer {t}"))` 丢掉这层；更重要的是两者在 code review 与日志里语义不同——「这是协议规定的 Bearer」与「这是管理员配的自定义头」不该长得一样。两个分支各走各的 API，语义不混。

`bearer` 字段**保留**（`shared` / `shared_or_user` / `user_required` 三态解析、`resolve_outbound_token` 一字不改），只是「最终怎么放进请求」由 `auth_header_name` 决定：

| `auth_header_name` | 解析出的 token | 出站 |
|---|---|---|
| `NULL` / 空 | `Some(t)` | `UpstreamAuth::Bearer(t)` |
| 非空 | `Some(t)` | `UpstreamAuth::Header { name, value: t }`（**不加 `Bearer ` 前缀**，TAPD / 工蜂这类头就是裸 token） |
| 任意 | `None` | `None`（匿名出站，仅发 `extra_headers`） |

`target()`（`mcp_gateway.rs:304`）多一个分支：从 `CallContext` 上取已解密的 `auth_header_name` / `extra_headers`。**解密只在这一处发生一次**（与 `build` 出站同一路径），不要在 `request_builder` 里做（那里没有 master key）。

### 3.3 校验：拒绝能配出「网关自己发不出去」的配置

- **受保护头黑名单**（注册时 400 `invalid_header_name`）：`Content-Type`、`Accept`、`MCP-Protocol-Version`、`Mcp-Session-Id`、`Host`、`Content-Length`，大小写不敏感。理由：这六个由网关按协议与运行状态写入，管理员覆盖任何一个都能让网关发出非法 MCP 请求（例如手工 `Mcp-Session-Id` 会串会话），而错误现场在上游侧，排查成本极高。
- **头名语法**：RFC 7230 token，`^[A-Za-z0-9!#$%&'*+\-.^_`|~]+$`，长度 ≤ 256。
- **头值**：拒绝 `\r` / `\n`（防头注入），长度 ≤ 4096。
- **`extra_headers` 与 `auth_header_name` 不允许重名**（大小写不敏感）→ 400 `duplicate_header_name`。两个源头写同一个头，最终值取决于注入顺序，这种「取决于顺序」的行为不该让管理员在 UI 上撞。
- **配了头名却没配值** → 创建时 400 `auth_value_required`；更新时若头名已变更且值留空，同样 400（否则会得到「头名叫 X，值是旧头名的值」这种错配）。`auth_header_name` 为空串 = 回到 Bearer，同时清掉 `auth_header_value`。

### 3.4 探测也必须带这套头

`probe.rs:379` `build_request` 与 `session.rs:322` `reachable` 都要。

**探测是这套配置唯一的验证手段**：TAPD 的情况是 `initialize` 就 401，工蜂是 `tools/call` 才 401——两种都只有探测能立刻说清「错在头」，而真实调用失败会被记成上游错误（`mcp_call_log.error_code` 是上游语义，不区分「头错」与「上游故障」）。

为此 `probe_server` 签名改为：

```rust
pub struct ProbeTarget<'a> {
    pub url: &'a str,
    pub auth: Option<session::UpstreamAuth<'a>>,
    pub extra_headers: &'a [(String, String)],
}
pub async fn probe_server(target: &ProbeTarget<'_>, other_server_tool_names: &[String]) -> Result<ProbeOutcome, ProbeError>
```

**直接复用 `session::UpstreamAuth`，不在 probe 另立枚举**——类型相同才能保证「探测怎么发，运行就怎么发」。`probe.rs` 的 `Session` 把 `bearer: Option<String>` 换成 `auth`（拥有所有权的镜像）+ `extra_headers`，`build_request` 与 `session::request_builder` 走同一段 match。`reachable` 同理，签名从 `(url, bearer, timeout)` 改为接 `&UpstreamTarget`，由调用方（`mcp_rest::health`）从 `McpServerRow` 组装。

**唯一的分叉点是凭据来源**：`probe_bearer`（`routes/mcp.rs:474`）改名 `probe_credential`，因为返回值已不是 bearer 而是「头名 + 值」；`user_required` 仍返回 `None`（保持 Stage 141 现状，管理员探测没有用户身份）。

### 3.5 `transport` 列不动

`transport` 仍只有一个合法值（Stage 140 §8 已登记为空列技术债）。本 Stage 不顺手删的理由：删它要改三方言 DDL + 9 处字面量 + 前端表单 + `005`/`029` 的迁移历史（已发布的迁移不能改，只能再加一次删列），变更面与「加三列」完全重叠，收益是删掉一个常量。不划算，且两件事搅在一个 PR 里会让 review 分不清哪处改动是被哪件事驱动的。

### 3.6 管理 API

`POST/PUT /admin/mcp/servers` body 追加：

```json
{
  "auth_header_name": "X-Tapd-Access-Token",
  "auth_header_value": "<明文，仅写入方向，永不回显>",
  "extra_headers": [{"name": "X-Project-Id", "value": "12345"}]
}
```

| 字段 | POST 缺省 | PUT 缺省 | PUT 空串 / 空数组 |
|---|---|---|---|
| `auth_header_name` | `NULL`（Bearer） | 不改 | 清除（回 Bearer） |
| `auth_header_value` | `NULL` | 不改 | 清除 |
| `extra_headers` | `[]` | 不改 | 清除 |

「缺省 vs 空」这个区分必须显式实现（`Option<Option<String>>` 或用 `serde(default)` 的 sentinel），否则用户无法删掉已配的值——这是本 Stage 最容易写错的一处。

### 3.7 前端 `ServerDialog`

现有字段 alias / displayName / url / credential_scope / enabled，payload `{display_name, url, credential_scope, enabled}`。新增：

- **「认证方式」二选一**：`Bearer`（默认）/ 自定义头名（文本框，placeholder `X-Tapd-Access-Token`，前端做与后端同一条 token 正则 + 黑名单的即时校验）。
- **「凭据值」**：`type="password"`；编辑已有 server 时 placeholder「留空则不修改」，旁侧显示 `***`，且**绝不预填**（H4）。
- **「额外请求头」**：可增删的行编辑（名称 + 值），保存时整体替换；已存值的行值框留空即不变。
- i18n ×2（`zh` / `en`）+ `resources.d.ts` 重生成。

---

## 4. 测试

### 4.1 core UT（≥8）

| # | 用例 | 期望 |
|---|---|---|
| 1 | `UpstreamAuth::Bearer` 出站 | 请求含 `Authorization: Bearer <t>` |
| 2 | `UpstreamAuth::Header` 出站 | 请求含 `<name>: <value>` 且**无** `Authorization` |
| 3 | 受保护头逐个尝试 | 全部 400，且错误码可区分于「非法头名」 |
| 4 | 非法头名（含空格 / `:` / 非 ASCII / 超长） | 拒绝 |
| 5 | 头值含 `\r` / `\n` | 拒绝 |
| 6 | `extra_headers` 序列化 → `seal_token` → `unseal_token` → 反序列化 | 往返相等，且密文不含明文 |
| 7 | `auth_header_name` 与 `extra_headers` 重名（含大小写差异） | 拒绝 |
| 8 | `auth_header_name` 为空 | 回落 `Bearer`，且 `auth_header_value` 被清 |

### 4.2 handler UT（≥4）

| # | 用例 | 期望 |
|---|---|---|
| 1 | 创建带自定义头 + 额外头 | 201/200，库里两列为 `v2:gcm:` 密文 |
| 2 | 更新时 `auth_header_value` 缺省 | 原值不变（解密可还原） |
| 3 | 更新时 `auth_header_value` 为空串 | 列变 `NULL` |
| 4 | GET 列表 / 单项 | `auth_header_value == "***"`、`extra_headers[].value == "***"`、`name` 明文，响应体不含明文 |
| 5 | 非法配置 | 400 + 稳定错误码（`invalid_header_name` / `duplicate_header_name` / `auth_value_required`） |

### 4.3 BDD（mock）

`tests/features/mcp_server_headers.feature`（无标签，`task bdd`）——上游用**自绑端口的 axum stub**，在 handler 里记录收到的头并回显断言用的 JSON：

| 场景 | 断言 |
|---|---|
| 自定义头名 | stub 收到 `X-Tapd-Access-Token: <token>`，且**没有** `Authorization` |
| 额外头 | stub 收到 `X-Project-Id`，`auth_header_name` 为空时仍收到 `Authorization: Bearer` |
| 默认 Bearer 不回归 | 未配任何新字段的 server，行为与 Stage 143 逐字节一致 |
| 探测用同一套头 | stub 在一次探测里收到的头序列与一次 `tools/call` 相同 |
| 重名 / 受保护头 | 注册 400，stub 零请求 |
| GET 脱敏 | 响应里值为 `***`，且不出现明文 token |

### 4.4 real BDD（三方言）

`tests/features/real/mcp_server_headers_real.feature`（`@real_api`）：

- `033` 迁移在 SQLite / PG / MySQL 真实应用成功（`task bdd-real-sqlite` / `task test-db-up` + `-pg` / `-mysql`）。
- **直读库断言**：`auth_header_value` 以 `v2:gcm:` 开头、不含明文、`unseal_token` 可还原；`extra_headers` 同样。手法与 Stage 144 `mcp_user.rs:461-475` 一致（那边断言的是 `mcp_server_tokens.token_value`）。
- 「值缺省不改」与「空串清除」在三方言上都成立（整行 SET 的 SQL 最容易在某方言漏列而静默清空）。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/033_mcp_server_headers.sql` | 新增（三方言 `ALTER TABLE mcp_servers ADD COLUMN` ×3） |
| `crates/aigw-core/src/models.rs` | `McpServerRow` +3 字段；9 处字面量补齐 |
| `crates/aigw-core/src/db.rs` | 三方言 SQL 常量 ×5 类（CREATE/GET/GET_BY_ALIAS/LIST/UPDATE）SET/SELECT/INSERT；`MCP_SERVER_PROJECTION` / `MCP_SERVER_GROUP_BY` +3 列；`Database` 分发；`db.rs:12206` 测试构造 |
| `crates/aigw-core/src/mcp/headers.rs` | **新增**：头名校验、受保护头黑名单、`extra_headers` JSON 编解码与加解密、长度上限 |
| `crates/aigw-core/src/mcp/mod.rs` | 挂 `pub mod headers;` |
| `crates/aigw-core/src/mcp/session.rs` | `UpstreamAuth` 枚举；`UpstreamTarget` 换 `bearer` 为 `auth` + `extra_headers`；`request_builder`；`reachable` 签名 |
| `crates/aigw-core/src/mcp/probe.rs` | `ProbeTarget`；`Session.bearer` → `auth`；`build_request`；`probe_server` 签名 |
| `crates/aigw-server/src/routes/mcp.rs` | create/update body +3 字段与校验；`probe_bearer` → `probe_credential`；`probe_and_store` 组装 `ProbeTarget`；GET 脱敏 |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | `:304` `target()` 组 `UpstreamAuth` + `extra_headers`；`:1053` 字面量 |
| `crates/aigw-server/src/routes/mcp_rest.rs` | `:365` `health` 传 `UpstreamTarget` |
| `crates/aigw-server/src/routes/mcp_user.rs` | `:413` 字面量 |
| `crates/aigw-frontend/src/pages/mcp/ServerDialog.tsx` | 认证方式 / 凭据值 / 额外头 三个控件；payload +3 |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | 响应类型 +3 |
| `crates/aigw-frontend/src/i18n/locales/{zh,en}*` + `resources.d.ts` | 新增文案 + 重生成 |
| `crates/aigw-server/tests/features/mcp_server_headers.feature` + `tests/bdd_steps/mcp_headers_steps.rs` | 新增（`mod.rs` 与 `bdd_steps/mod.rs` 注册） |
| `crates/aigw-server/tests/features/real/mcp_server_headers_real.feature` | 新增 |
| `crates/aigw-server/tests/bdd_steps/mcp_aggregate_steps.rs` / `mcp_passthrough_steps.rs` | `McpServerRow` 字面量补齐（`:227` / `:201`） |
| `crates/aigw-frontend/src/api-mocks.ts` | MCP server 响应加新字段（默认 `null` / `[]`） |

---

## 6. 回归验证

全部经 `task`，禁止裸 `cargo`：

- `task test`（全量，门禁只认这个）
- `task bdd` / `task bdd-real-sqlite`
- `task test-db-up` + `task bdd-real-pg` / `task bdd-real-mysql`
- `task fmt`（只读检查）/ `task lint` / `task doctor`
- `task fe-bdd` / `task fe-lint` / `task fe-build`

**手工**：把 TAPD 与工蜂重新注册进真实网关（9.135.87.221:4001），配 `X-Tapd-Access-Token` / `PRIVATE-TOKEN`，用探测验证 `tools/list` 成功；再用本地 Claude Code 走一次 `/mcp/{alias}` 的 `tools/call`（工蜂的关键失败点在调用而非握手，只探测不够）。

---

## 7. 门禁

- [x] 库里的 `auth_header_value` 与 `extra_headers` 不含明文，以 `v2:gcm:` 开头且可 `unseal_token` 还原（real BDD 直读，三方言）—— `real_mcp_headers_steps.rs` 直读三方言库断言密文前缀与明文，`bdd-real-sqlite|pg|mysql` 各 84/84
- [x] `GET /admin/mcp/servers` 与 `/{id}` 响应里 `auth_header_value` / `extra_headers[].value` 一律 `***`，响应体不出现明文 —— UT `a_saved_response_redacts_both_secrets`；`server_wire` 是唯一序列化点（list / all / get / create / update / toggle 全部经它）
- [x] 受保护头（六个）注册被拒并给出 `invalid_header_name`；`auth_header_name` 与 `extra_headers` 重名给 `duplicate_header_name`；配名不配值给 `auth_value_required` —— `reserved_names_are_rejected_case_insensitively` 覆盖全部六个；`a_reserved_auth_name_is_rejected` / `an_extra_header_colliding_with_the_auth_name_is_rejected` / `a_name_without_a_value_is_rejected`
- [x] 默认 Bearer 路径零回归：未配新字段的 server，出站头与 Stage 143 逐字节相同 —— `OutboundHeaders::resolve` 在无名字无静态值时就产 `UpstreamAuth::Bearer(token)`，BDD 场景 `默认 Bearer 不回归`；`an_unset_auth_name_still_owns_the_authorization_header` 钉死「未配名字时 `Authorization` 仍属协议凭据」
- [x] 探测与运行路径用同一套头：`probe.rs` 与 `session.rs` 共用 `UpstreamAuth`，BDD 断言两者到达上游的头序列一致 —— `mcp_server_headers.feature` 的「探测与调用到达上游的头一致」；`user_required` 的例外见 ADR-048 决策 2（探测显式抹掉静态值，与真实调用一致）
- [x] 三方言 real BDD 全绿（含 `033` 迁移应用成功、值缺省不改 / 空串清除在三方言一致）—— 2026-10-31 在 §8.1 的七条修正**之后**重跑 `bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql`，各 `84 scenarios (84 passed) / 473 steps`，exit 0
- [x] 前端编辑已有 server 时不把 `***` 当值写回（值框不预填，UT/BDD 断言提交体里不含 `***`）—— 值框只读 placeholder；后端另加 `ValueIsRedacted` 兜底（ADR-048 决策 5），任何客户端回写 `***` 都是 400
- [x] `task test` / `task bdd` / `fmt` / `lint` 全绿 —— `task test` 全量 1576 passed / 0 failed；`task bdd` 见下方证据；`task fmt` / `task lint` / `task doctor` 均通过；`task fe-bdd` 594 passed / 3 skipped

---

## 8. 风险与遗留

- **9 处 `McpServerRow` 字面量是机械改动，会漏** —— 编译器能兜住漏改，但 BDD 里的两处（`mcp_aggregate_steps.rs:227`、`mcp_passthrough_steps.rs:201`）容易在「先让 lib 编过」时被顺手塞 `unwrap()` 或错误默认值，导致测试意外走 Bearer 分支。改的时候一律显式写 `None` / `&[]`。
- **`extra_headers` 整块加密 = 改一个头要重写整块密文** —— 两个人同时编辑同一 server 的额外头，后保存的会覆盖前者的全部改动（与 ACL 整表替换、与 `git` 的整文件写同一取舍）。当前无并发编辑场景（管理员个位数），不做乐观锁；若将来需要，加 `updated_at` 版本号比较即可。
- **受保护头黑名单是「宁可多拒」的方向** —— 可能误伤某个要求自定义 `Accept` 的怪异上游。届时的放宽顺序：先放 `Accept` / `Content-Type`，`MCP-Protocol-Version` 与 `Mcp-Session-Id` 永不放宽（前者决定协议分支，后者串会话）。
- **探测专用凭据的空缺** —— `user_required` 的 server 在探测时无凭据（Stage 141 现状），所以这类 server 即便配了自定义头，也只有真实用户调用才能验证。**不要把探测成功当作「这个 server 一定能用」**；UI 上探测结果对 `user_required` 应带一句说明（可留到 Stage 148 的进度明细里）。
- **值长度的上限是 Rust 侧校验** —— 绕过 API 直接写库可以插入超长/超项的值，读时 `unseal_token` 仍能解，只是出站会变慢。可接受：DB 是受信边界，与 token 表现状一致。
- **`reachable` 签名改动波及 `/mcp-rest/health`**（`mcp_rest.rs:365`），这条路径的响应形状不变，但它现在也会带自定义头——意味着 health 探测对 TAPD 这类上游从「401 不可达」变为「可达」，这是修复而非回归，需要在 §6 手工验证里确认一次。
- **`transport` 空列** 仍保留（§3.5），技术债照旧登记。
- **`probe_bearer` → `probe_credential` 的改名是语义纠偏** —— 旧名会让后来者以为「探测只能带 bearer」，从而在 probe 里再分叉一次，正是本 Stage 要消灭的形态。

### 8.1 Gate 4 复查后的修正（2026-10-31）

独立复查找出的问题全部在**失败路径**上，已按 ADR-048 修完并各补一条测试：

| # | 问题 | 修正 |
|---|------|------|
| 1 | `decrypt_extra_headers` 解密失败返回空列表，而这个空列表会进 `PUT` 的整表替换 → 换 key 后保存一次就把密文覆写成 `NULL` | 新增严格版 `try_decrypt_extra_headers`；管理读写路径用它，解不开就 500 且密文不动 |
| 2 | `OutboundHeaders::resolve` 对 `user_required` 仍回落静态值 → 探测成功但真实调用永远 403 | `probe_and_store` 显式抹掉静态值，与 `prepare_for` 的判定一致 |
| 3 | `token_owner` 只认 token 行 → 只有静态值的 server 每次调用都记 `anonymous` | 新增 `effective_owner`：凭据实际出站就记 `server` |
| 4 | `validate_value` 只拒 CR/LF，非 ASCII / 控制字符存进去后每次调用都是不可归因的传输错误 | 与 `HeaderValue::from_str` 同集合：只接受可打印 ASCII + tab |
| 5 | `***` 可当值提交 → `GET→改→PUT` 的客户端把真凭据换成三个星号且看不出来 | 新增 `ValueIsRedacted`，仍归 `invalid_header_value` |
| 6 | `auth_header_value` 无 `auth_header_name` 时 200 且丢弃 | 400 `auth_value_required` |
| 7 | 仅大小写变化的改名被当成改名，要求重输值 | 改名判定用 `eq_ignore_ascii_case` |

第 1 条的另一半是**换 key 没有迁移路径**这件事本身 —— 已登记 **TD-031**，不在本 Stage 处理。

---

## 9. 对后续 Stage 的影响

- **Stage 147（调用日志页）不受影响** —— 它只读 `mcp_call_log`，本 Stage 不改该表。
- **Stage 148（探测进度）必须排在本 Stage 之后** —— 它要改 `probe_and_store`，而本 Stage 已把 `probe_server` 的签名换成 `&ProbeTarget`；反过来做会让两边的改动互相踩。
- **OAuth（Phase 57）与本 Stage 正交** —— Phase 57 在 `WWW-Authenticate` 上做发现与刷新，本 Stage 管「发出去的凭据放在哪个头」；组合起来的形态是「OAuth 拿到的 token 放进管理员配的头名」，不需要额外建模。一个已知的交汇点：TAPD 的 OAuth token 有 24h 上限，接管它之后 `auth_header_value` 会变成「可被刷新覆盖的槽位」，届时需要区分「静态值」与「OAuth 管理值」，留待 Phase 57 决定。
