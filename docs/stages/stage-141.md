# Stage 141: MCP 管理 API + 上游探测（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~16h（servers CRUD + tokens 子资源 + 双栈探测 + catalog 快照 + handlers UT + BDD）
**依赖**: Stage 140（`McpStore` / `alias` 校验 / token 加解密）
**状态**: ✅ 完成（2026-10-29）—— Gate 3 全绿，Gate 4 的 2 HIGH + 5 MED + 3 LOW 已落并经主 Agent 逐条复核

**提交**: `51ca3a5`（server 管理 API）→ `d20841c`（BDD + mock 上游）→ `a5fad16`（Phase 55 收官回写），均 `--signoff`，未 push。

---

## 1. 问题

Stage 140 之后数据层就位，但管理员还没有任何办法把一个上游 server 注册进来、填上 token、看看它到底有哪些工具。

本 Stage 交付 `/admin/mcp/*` 的**全部管理端点**，以及一个真正的探测实现——探测不只是「能不能连通」，而是「协商到哪个协议版本、有哪些工具、有没有和别的 server 撞名」。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 上游探测**必须先试 `server/discover`，失败再回落 `initialize`** —— 不能只试一个 | 只试现代：旧版上游全挂；只试旧版：拿不到 `ttlMs` / `supportedVersions`，缓存策略无从谈起 | 实测：CC 先发 `server/discover`；Codex 只发 `initialize`。上游两侧都存在 |
| H2 | `tools/list` 会翻页，`nextCursor` 必须跟到底 | 工具超过一页时 catalog 残缺，模型「想不到用那个能力」，且没有任何报错 | 探针实测（`paged_probe.py`）：两次 `tools/list`，第二次带 `cursor: cursor-page2` |
| H3 | 探测要**跑完整解析**，而不是只看 HTTP 状态码 | SearXNG 的「HTTP 200 + HTML」教训已在本仓发生过一次；上游返回 200 但 body 是错误页时，只看状态码会把失败记成成功 | 解析 JSON-RPC envelope，断言有 `result` 且有 `tools` 数组 |
| H4 | admin 鉴权用既有的 `SpendAuth` + `require_admin`，不新造中间件 | 若造新中间件，两条鉴权路径会漂移 | 读 `crates/aigw-server/src/routes/spend.rs:96-126` / `:233-242`，`proxies.rs` / `websearch.rs` 全部如此 |
| H5 | token 明文在**任何响应里都不出现** | 一次泄漏即全部上游凭据泄漏 | `McpTokenResponse`（Stage 140 已落地）只含 `token_name` / `token_redacted` / `is_default` / `enabled` / `expires_at` / 时间戳；`token_redacted` 是 `"***"` 或空串（空 = 未设置），`token_value` 字段在 DTO 上不存在 |

---

## 2. 边界

**做**：
- `030_mcp_tool_catalog.sql` × 三方言（探测快照表）
- `/admin/mcp/servers` 全套：`GET`（分页/`status`/`search`）、`GET /all`、`POST`、`GET/PUT/DELETE /{id}`、`POST /batch-delete`、`POST /{id}/toggle`
  - `POST /batch-delete` 目前**没有前端消费者**（Stage 142 的表格没做多选，`api-mocks.ts` 只为它留了桩）。仍照 `proxies` / `websearch` 的形状实现 —— 三个管理页同一套端点形状，少一个才是不一致；但它是本 Stage 唯一没有端到端证据的端点，门禁不覆盖它。
- `/admin/mcp/servers/{id}/tokens` 四端点：`GET`、`POST`、`DELETE /{token_id}`、`POST /{token_id}/default`
- `POST /admin/mcp/servers/{id}/probe`：双栈协商 → 全翻页 `tools/list` → 写 catalog 快照 → 返回协议版本 / `serverInfo` / `ttlMs` / 工具数 / 冲突工具
- `GET /admin/mcp/servers/{id}/tools`：读 catalog 快照
- `POST /admin/mcp/reload`：清空 catalog 快照并全部重探
- `mcp_server_crud.feature` BDD

**不做**（明确排除）：
- **不做 `/mcp` / `/mcp/{alias}` 对外端点** —— 143
- **不做 ACL 端点** —— 144
- **不做调用日志端点** —— 144（表由 143 的 `031_mcp_call_log.sql` 建，端点由 144 建）
- **不做 OAuth 发现** —— Phase 57
- **不做探测结果的前端呈现** —— 142

---

## 3. 方案

### 3.1 路由注册

在 `crates/aigw-server/src/main.rs` 的 admin 路由块（`:670-742`，`proxies` / `websearch` 那段）追加 `mcp` 一组，形状照抄：

```rust
.route("/admin/mcp/servers", get(list_mcp_servers).post(create_mcp_server))
.route("/admin/mcp/servers/all", get(list_all_mcp_servers))
.route("/admin/mcp/servers/batch-delete", post(batch_delete_mcp_servers))
.route("/admin/mcp/servers/{id}", get(get_mcp_server).put(update_mcp_server).delete(delete_mcp_server))
.route("/admin/mcp/servers/{id}/toggle", post(toggle_mcp_server))
.route("/admin/mcp/servers/{id}/probe", post(probe_mcp_server))
.route("/admin/mcp/servers/{id}/tools", get(list_mcp_server_tools))
.route("/admin/mcp/servers/{id}/tokens", get(list_mcp_tokens).post(create_mcp_token))
.route("/admin/mcp/servers/{id}/tokens/{token_id}", delete(delete_mcp_token))
.route("/admin/mcp/servers/{id}/tokens/{token_id}/default", post(set_default_mcp_token))
.route("/admin/mcp/reload", post(reload_mcp))
```

### 3.2 列表响应信封

与 `proxies` / `websearch` 完全一致：
```json
{"object":"list","data":[...],"count":20,"total_count":137,"page":1,"page_size":20,"total_pages":7}
```

分页参数 `page` / `page_size`（clamp 1..=100）/ `status`（`all|enabled|disabled`）/ `search`（alias 或 display_name 模糊）/ `sort_by` / `sort_order`。计数与取页用 `tokio::try_join!` 并发。

**每个 `data` 元素必须带 `tool_count: i64`** —— Stage 142 的表格有一列「工具数」，且该列在**未打开 ProbeDialog 时**就要显示。本 Stage 的取页查询用 `LEFT JOIN mcp_tool_catalog` 聚合出计数（`COUNT(c.id)`，无快照则为 `0`），不能让前端为每一行单独发一次 `/tools` 请求。`GET /admin/mcp/servers/{id}` 单条详情同样带 `tool_count`。

`tool_count` 直接挂在 `McpServerRow` 上（`#[sqlx(default)] pub tool_count: i64`），不另造第二个 DTO —— `#[sqlx(default)]` 只在 `ColumnNotFound` 时生效，所以 `GET_MCP_SERVER_*` 那种不带聚合的 SELECT 也能复用同一个 struct。**注意 `#[sqlx(default)]` 不覆盖 NULL**：聚合列必须写成 `COALESCE(COUNT(c.id), 0)`，否则 LEFT JOIN 无匹配行时解 `i64` 会失败。

`GET /admin/mcp/servers/{id}` 走 `with_tool_count`（单条下多一次 catalog 查询，比维护第二套 SELECT 形状便宜）；`GET /admin/mcp/servers/all` **不带 `tool_count`，恒为 `0`** —— 它的唯一消费者是 142 的 alias 查重下拉，不读工具数。不要把它当列表数据用。

### 3.3 探测实现

```rust
pub struct ProbeOutcome {
    pub protocol_version: String,      // "2026-07-28" 或 "2025-06-18"
    pub server_info: Option<Value>,
    pub ttl_ms: Option<i64>,
    pub tools: Vec<UpstreamTool>,
    pub conflicts: Vec<String>,        // 与本网关其它 server 撞名的工具（去前缀后）
}
```

**两个端点的响应形状钉死（Stage 142 已按此编码，改动即为破坏契约）：**

```
POST /admin/mcp/servers/{id}/probe
  → 200 ProbeOutcome，serde 全部 snake_case（无 rename_all）：
    {"protocol_version":"2026-07-28","server_info":{"name":"...","version":"..."},
     "ttl_ms":60000,"tools":[{"tool_name":"search","description":"...","input_schema":{...},
     "annotations":null,"has_conflict":false}],"conflicts":["search"]}
  → 探测失败：502 {"error":{"message":"...","type":"probe_failed","upstream_status":405,"body":"<前 500 字>"}}

GET /admin/mcp/servers/{id}/tools
  → 200 {"tools":[<同 UpstreamTool 结构>],"conflicts":["..."],"fetched_at":"<RFC3339|null>"}
     （读 `mcp_tool_catalog` 快照；无快照 = `tools: []` + `fetched_at: null`，不是 404）
```

`UpstreamTool` 的字段名沿用 catalog 列名（`tool_name` / `description` / `input_schema` / `annotations` / `has_conflict`），不额外造 `name`/`schema` 别名 —— 前端与后端读同一张表的同一个名字，改名时两边会一起被改到。

> **`CreateMcpServerRequest` 必须接受 `enabled`**（`#[serde(default)] pub enabled: Option<bool>`，缺席时视为 `true`）。Stage 140 的 DTO 没有这个字段，而 Stage 142 的 `ServerDialog` 始终在 create body 里带它（对话框有启用开关）。serde 默认丢弃未知字段，于是「关着开关新建」会得到一个启用的 server —— 无报错、无 toast，界面显示与用户意图相反。PUT 侧的 `UpdateMcpServerRequest` 本来就有 `enabled`，只需补 create 侧。

流程：

1. `POST {url}`，body `{"jsonrpc":"2.0","id":1,"method":"server/discover"}`
   - 头：`MCP-Protocol-Version: 2026-07-28`、`Content-Type: application/json`、`Accept: application/json, text/event-stream`
   - 凭据：`resolve_outbound_token` 的返回值决定是否带 `Authorization: Bearer`
2. 200 且 `result.supportedVersions` 非空 → **现代**，取数组里最新的为 `protocol_version`；`ttlMs` 取 `result.ttlMs`
3. 否则（4xx / 405 / 无 `supportedVersions`）→ **旧版**：
   `initialize`（`protocolVersion: "2025-06-18"`）→ 存 `Mcp-Session-Id` → `notifications/initialized` → 后续请求带回该头

> **token 加解密用 `state.aigw_master_key`**，与 `websearch.rs:686-695` 的 `seal_api_key` 同一把钥匙（不是 `state.master_key`）。BDD 里没有 master key 时创建 token 会 500 `master_key_missing`，所以 token 相关场景必须走已有的 `数据库已初始化且已配置 master key` given（`proxy_steps.rs`），不能只靠默认 `ensure_state()`（它的 `aigw_master_key: None`）。
4. `tools/list`，循环跟 `nextCursor` 直到无 `nextCursor`，**设硬上限 50 页**（防止上游返回自身游标造成死循环）
5. 逐工具算冲突：与 catalog 里**其它 server** 的 `tool_name` 同名 → 记入 `conflicts`
6. 事务内 `DELETE FROM mcp_tool_catalog WHERE server_id=?` 再批量插入（快照语义，不是增量）
7. 更新 `mcp_servers.protocol_version` / `updated_at`

**响应可能不是 JSON**：请求带 `Accept: application/json, text/event-stream`，Streamable HTTP 允许上游用 `text/event-stream` 回一个普通的 POST 响应（JSON-RPC 结果作为一条 `data:` 事件送达）。因此解析必须按 `Content-Type` 分派：

- `application/json` → 直接解析 envelope
- `text/event-stream` → 逐事件读，取第一条 `data:` 中 `id` 匹配且含 `result` / `error` 的 envelope；读到流结束仍未匹配 = 失败
- 其它（含 HTML）→ 失败（H3）

把这段「从响应里取出 JSON-RPC envelope」抽成 `mcp/probe.rs` 里的**一个有类型返回的函数**（`read_jsonrpc_response(resp) -> Result<Value, ProbeError>`），Stage 143 的出站协商复用同一个函数 —— 上游用 SSE 回 `initialize` 是同样合法的行为，两处各写一遍等于两处都要各自踩一次。

**失败处理**：探测失败**不删旧快照**，只返回错误（含上游状态码与截断后的 body 前 500 字）。理由：上游临时抖动不该让工具目录瞬间清空。

#### `030_mcp_tool_catalog.sql` 列定义

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | 自增主键 | 同 029 |
| `server_id` | 外键 → `mcp_servers(id)` `ON DELETE CASCADE` | **MySQL 必须表级 `FOREIGN KEY`** |
| `tool_name` | 文本 | 上游原始工具名，**不带 alias 前缀**（前缀是 144 聚合时才加的） |
| `description` | 文本，默认 `''` | |
| `input_schema` | JSON | SQLite `TEXT` / PG `JSONB` / MySQL `JSON`（照 `028_web_search_providers.sql` 的 `probe_result` 三方言写法）。Rust 侧绑 `serde_json::Value`（同 `proxies.probe_result`，`models.rs:1233`）—— sqlx 的 MySQL JSON 编解码原生处理，**不涉及** `aigw-migrate` 那条 `CAST(CAST(X'hex' AS CHAR) AS JSON)` 裸 SQL 字面量的坑 |
| `annotations` | JSON，可空 | 原样存不解析（§8） |
| `has_conflict` | 布尔，默认假 | 探测时算出的「与其它 server 同名」 |
| `fetched_at` | 文本 | RFC 3339 |

唯一约束 `uq_mcp_tool_catalog_server_tool (server_id, tool_name)` —— 同一 server 同一工具名不可能有两个，这是快照语义的直接表达（事务内先删后插也不会撞，但约束能挡住写路径的 bug）。索引 `idx_mcp_tool_catalog_server (server_id)` 由唯一约束的左前缀覆盖，**不需要单独建**。

### 3.4 token CRUD 的越权边界

`POST /admin/mcp/servers/{id}/tokens` 的 `owner_type`：

- `server` —— 仅 admin（本组路由本身已 `require_admin`）
- `user` —— admin 可**代建**（`owner_id` 必填，必须是真实 `user_id`）

用户自助路径走 `/v1/mcp/*`（144），那条路径**不接受 `owner_id` 入参**，身份只从 `KeyIdentity` 取。

### 3.5 删除守卫

`DELETE /admin/mcp/servers/{id}`：直接删，连带 `ON DELETE CASCADE` 清 token 与 catalog。**不加 in-use 守卫** —— 与 `proxies` 不同，MCP server 被删不会让任何已注册模型失效，唯一后果是 agent 下次 `tools/list` 少一个前缀。

---

## 4. 测试

### 4.1 handler UT（≥10）

| # | 用例 | 期望 |
|---|---|---|
| 1 | 非 admin 调 `/admin/mcp/servers` | 403 `forbidden` |
| 2 | create 缺 `url` | 400 |
| 3 | create `alias` 含 `__` | 400 |
| 4 | create `alias` 重复 | 409 |
| 5 | 列表分页 + `search` 过滤 | 条数与 `total_count` 匹配 |
| 6 | toggle 两次 | 回到原状态 |
| 7 | create token 后 GET tokens | 响应**不含** `token_value`（DTO 上无此字段），`token_redacted == "***"` |
| 8 | 置第二条为默认 | 第一条 `is_default=false` |
| 9 | delete server | token 与 catalog 行全清（级联） |
| 10 | probe 打一个拒绝连接的 URL | 返回错误，**旧 catalog 不被删除** |

### 4.2 探测 UT（≥6）

用每用例自绑端口的 axum router（照 `crates/aigw-core/src/websearch/probe.rs:172` 的 `spawn_stub`，ADR-041），不用 `bdd_support/mock_upstream.rs` 的固定形状 mock：

| # | 用例 | 期望 |
|---|---|---|
| 1 | 上游支持 `server/discover` | `protocol_version == "2026-07-28"`，不发 `initialize` |
| 2 | 上游 405 `server/discover` | 回落 `initialize`，`protocol_version == "2025-06-18"` |
| 3 | `tools/list` 两页 | 工具数 = 两页之和 |
| 4 | 上游返回 200 + HTML | 判为失败（H3） |
| 5 | 上游返回自身游标（无限页） | 50 页硬上限生效，返回已有工具并告警 |
| 6 | 两个 server 有同名工具 | `conflicts` 含该工具名 |
| 7 | 上游用 `text/event-stream` 回 `tools/list` 结果 | 解析出与 JSON 分支相同的工具集（§3.3 的 `read_jsonrpc_response`） |

### 4.3 BDD

`crates/aigw-server/tests/features/mcp_server_crud.feature`：创建 → 列表可见 → 探测 → 工具数 > 0 → 加 token（响应脱敏）→ 置默认 → 停用 → 列表按状态过滤 → 删除 → 列表消失。

**探测场景需要自建 JSON-RPC stub，`MockUpstream` 顶不了** —— `bdd_support/mock_upstream.rs` 的路由表是固定的 LLM 形状（`/v1/chat/completions` 等），没有 `server/discover` / `initialize` / `tools/list` 的处理，探测步骤打过去会失败。因此 step 里要照 Stage 141 §4.2 的做法起一个 `TcpListener::bind("127.0.0.1:0")` 的 axum stub，把它的地址作为该 server 的 `url` 写进 DB，场景结束后释放。token 场景走 `数据库已初始化且已配置 master key` given（§3.3 的 blockquote）。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/030_mcp_tool_catalog.sql` | 新增 |
| `crates/aigw-core/src/models.rs` | `McpToolRow` / `ProbeOutcome` / DTO |
| `crates/aigw-core/src/mcp/probe.rs` | 双栈协商 + 全翻页 + 冲突计算（纯逻辑与 IO 分层） |
| `crates/aigw-core/src/mcp/catalog_cache.rs` | 进程级 catalog 缓存（ADR-041）：`get` / `put` / `invalidate` / `clear` |
| `crates/aigw-core/src/db.rs` | 新增 catalog store（快照替换 / 按 server 读 / 冲突查询）+ **分页列表**（`list_mcp_servers_paged(limit, offset, status, search, sort_by, sort_order)` + `count_mcp_servers(...)`，照 `list_proxies` / `count_proxies` 的形状）。servers / tokens 的 CRUD 在 Stage 140 已就位（`McpStore`），本 Stage 只补齐「分页 + 计数」两个缺口 |
| `crates/aigw-server/src/routes/mcp.rs` | 新增，全部 handler |
| `crates/aigw-server/src/routes/mod.rs` | **必须加 `pub mod mcp;`** —— 不注册模块，`main.rs` 里的 `use routes::mcp` 找不到 |
| `crates/aigw-server/src/main.rs` | 路由注册 |
| `crates/aigw-server/tests/features/mcp_server_crud.feature` + `tests/bdd_steps/mcp_steps.rs` | 新增（新 steps 文件必须在 `crates/aigw-server/tests/bdd_steps/mod.rs` 注册 `pub mod mcp_steps;`，否则 cucumber 找不到步骤） |
| `crates/aigw-server/tests/bdd.rs` | **不改** —— `ensure_state` 里的 `AppState` 字面量不因本 Stage 变化（ADR-041：catalog 缓存是进程级静态，不进 `AppState`） |

---

## 6. 回归验证

- `task test` 全量
- `task bdd`（新增场景）
- `task test-db-up` + `task bdd-real-pg` / `task bdd-real-mysql`
- `task fmt` / `task lint` / `task doctor`

---

## 7. 门禁

- [x] 全部端点非 admin 返回 403 —— 18 个 `/admin/mcp/*` handler **逐一核对**均以 `require_admin(&auth)?` 开场（18/18，`mcp.rs` 内 `pub async fn` 全量扫描）；具名断言 `non_admin_key_is_forbidden_on_the_server_list`（`mcp.rs:1355`）、`a_non_admin_cannot_read_or_write_an_allowlist`（`:2464`，读+写两条）、`a_non_admin_cannot_read_the_call_log`（`:2761`）
- [x] 任何响应体不含 token 明文（grep 断言）—— `a_token_response_never_carries_the_plaintext`（`:1768`，断言 list 响应串不含 `sk-upstream-secret`）+ `a_stored_token_is_sealed_not_plaintext`（`:1801`，断言库里存的是密文）
- [x] 探测对现代 / 旧版上游都成功 —— `crates/aigw-core/tests/mcp_probe.rs` 的 `discover_selects_the_newest_supported_version_and_skips_initialize`（`:82`）与 `discover_405_falls_back_to_initialize_and_carries_the_session_id`（`:131`）
- [x] 探测失败不摧毁旧快照 —— `a_probe_against_a_refused_endpoint_fails_and_keeps_the_old_snapshot`（`mcp.rs:1726`）+ `reload_keeps_a_disabled_servers_snapshot`（`:2136`，H1 整改后的回归），另 `reload_flags_a_shared_tool_name_on_both_servers`（`:2170`）守住冲突检测对称性
- [x] 三方言迁移应用成功 —— 029/030 在 SQLite / PG / MySQL 真库上应用成功：`task bdd-real-sqlite` 与 PG / MySQL 真库跑同一批 `@real_api` 场景均 0 失败（PG `/tmp/final-pg.log`、MySQL `/tmp/final-mysql.log`，19 features / 80 scenarios / 452 steps 全过，即含 141 的 `real/mcp_server_crud_real.feature` 6 场景）
- [x] `task test` + `task bdd` 全绿 —— `/tmp/stage143-test-final.log` 1497 passed / 0 failed / 22 个 test result、`/tmp/stage143-bdd.log` 362 场景 344 passed / 18 skipped、`✘`=0（均为 143 收口后的全量复跑，覆盖 141 的用例）

---

## 8. 风险与遗留

**Gate 4 结论：BLOCK（2 HIGH / 5 MED / 3 LOW，2026-10-29，独立 reviewer）**。安全核心无缺陷 —— 凭据方向编译期封死、`user_required` 不回落、三方言 bind 数逐一核对、迁移方言陷阱、12 个 `/admin/mcp/*` 路由的 authz 全部正确。缺陷集中在两处行为与一处健壮性：

| # | 级别 | 缺陷 | 被推翻的原文假设 |
|---|---|---|---|
| H1 | HIGH | `POST /admin/mcp/reload` 先 `clear_mcp_tool_catalog()` 全表清空、再只探测 `enabled` 的 server。禁用 server 的旧快照**永久丢失**（reload 跳过它，重新启用也不会补探）；enabled 但上游临时不可用时旧快照被换成空。且 `probe_and_store` 在循环内读 `mcp_tool_names_elsewhere`，读的是已清空的表 → 冲突只能看到「先探测过的」，第一个 server 永远零冲突 | §1.5 / §8「探测失败不删旧快照」「reload 能刷新出全局视图」 |
| H2 | HIGH | `display_name`（MySQL `VARCHAR(255)`）与 `transport`（`VARCHAR(32)`）只校验非空，超长在 MySQL 严格模式报 1406 → 500，SQLite/PG 同请求 200；上游 `tool_name` > 255 会让整个探测事务回滚并报通用 `db_error` 而非探测错误 | 设计未提长度校验 |
| M1 | MED | `search` 在 SQLite 大小写敏感、MySQL/PG 不敏感；`%` / `_` 三方言均未转义 | 无 |
| M2 | MED | `token_name` 查重是 check-then-insert 无事务，竞态下唯一约束违反 → 500 而非承诺的 409 | 无 |
| M3 | MED | `first_result_event` 只判「含 `result`/`error`」，不校验 `id`，错配的通知/乱序事件会被当成本次答案 → 静默空目录或错误中止 | 「§3.3 取 `id` 匹配的 envelope」 |
| M4 | MED | `batch-delete` 的 `deleted_ids` 来自请求体、`skipped` 恒为空数组，不存在的 id 也报成功 | 无 |
| M5 | MED | `page`/`page_size` 的 offset 在 `i32` 里算，`page=2000000000` 溢出（debug panic / release 负偏移） | 无 |
| 补 | — | `expires_at` 入库前不校验 RFC 3339，坏值让 token **永久不可用且无报错**（`is_token_usable` 对不可解析值返回 false）—— 与 142 的 H1 同族 | 无 |

### 8.1 Gate 4 整改（已落地，2026-10-29）

| # | 整改内容 | 代码位置 | 覆盖它的 UT |
|---|---|---|---|
| H1 | `reload_mcp` 不再调用 `clear_mcp_tool_catalog()`。改为两趟：先对所有 `enabled` server 各算一次 `mcp_tool_names_elsewhere`（读的是**旧的**整表），再逐个探测并把同一份 `others` 传进 `probe_and_store`。跳过 `disabled` 是刻意的 —— 它们没有新快照可换，探测它们只会把「停用期间的上游故障」变成「快照被清空」 | `crates/aigw-server/src/routes/mcp.rs:626`（两趟循环）、`:525`（`probe_and_store` 改签名收 `others`）、`:582`（单点 probe 自己算 `others`）；`clear_mcp_tool_catalog` 的 doc 已注明「请求路径不再调用」`crates/aigw-core/src/db.rs:6324` | `reload_keeps_a_disabled_servers_snapshot`（`mcp.rs:2136`）、`reload_flags_a_shared_tool_name_on_both_servers`（`mcp.rs:2170`）、`reload_reports_a_failed_probe_without_aborting_the_rest` |
| H2 | 列宽在进 DB 之前拦：`display_name` / `token_name` ≤ 255、`transport` ≤ 32，超长返回 400；上游 `tool_name` > 255 变成 `ProbeError::Malformed`，不再让整个探测事务以通用 `db_error` 回滚 | `mcp.rs:52`/`:53`（上限常量）、`:155`（`validate_len`）、`:289`（create `display_name`）、`:303`（create `transport`）、`:376`（update `display_name`）、`:712`（create token `token_name`）；`crates/aigw-core/src/mcp/probe.rs:39`（`MAX_TOOL_NAME_LEN`）、`:299`（超长 → `Malformed`） | `a_display_name_over_the_column_width_is_a_bad_request`（`mcp.rs:1383`）、`a_transport_over_the_column_width_is_a_bad_request`（`mcp.rs:1416`）、`parse_tools_rejects_a_name_that_would_not_fit_the_column`（`probe.rs:645`） |
| M1 | `search` 在三个方言上统一成大小写不敏感（SQLite/PG 显式 `LOWER(...)`，PG 用 `ILIKE`）+ `%` / `_` / `\` 全部转义并配 `ESCAPE '\'`。六个查询（3 list + 3 count）改法一致，list 与 count 不会给出不同答案 | `crates/aigw-core/src/db.rs:6435`（`mcp_search_pattern` 转义）、`:6364`/`:6802`/`:7196`（三方言 list 常量）、`:6679`/`:6857`/`:7459`（三方言 count） | `a_search_full_of_wildcards_matches_nothing`（`mcp.rs:1592`，断言 `search=%25` 匹配 0 行且 `count == total_count`）、`search_and_status_filters_agree_with_total_count`（`mcp.rs:1546`） |
| M2 | `token_name` 的 check-then-insert 竞态：先查重（409），INSERT 若仍撞唯一约束再按 SQLSTATE 判一次（PG `23505` / MySQL `1062` / SQLite `2067`）→ 同样 409 `DUPLICATE_TOKEN_NAME`，不再是 500 | `mcp.rs:105`（`duplicate_token_name`）、`:120`（`is_unique_violation`）、`:800`（INSERT 的错误映射） | `a_duplicate_token_name_for_the_same_owner_is_a_conflict`（`mcp.rs:1935`） |
| M3 | `first_result_event` 增加 `expected_id`：只接受 `id` 与本次请求一致、且带 `result`/`error` 的 envelope。`Session::call` 把自增 id 一路传进 `read_jsonrpc_response_for` → `parse_jsonrpc_body_for` → `first_result_event`。relay 侧（143）仍走 `None` 的宽松路径 | `crates/aigw-core/src/mcp/probe.rs:223`（`first_result_event`）、`:130`（`read_jsonrpc_response_for`）、`:158`（`parse_jsonrpc_body_for`）、`:417`（`Session::call` 传 `Some(&json!(id))`） | `first_result_event_ignores_an_envelope_answering_another_request`（`probe.rs:664`）、`first_result_event_ignores_a_foreign_error_and_keeps_looking`（`probe.rs:672`） |
| M4 | `batch-delete` 先 `get_mcp_server`：存在才删并进 `deleted_ids`，不存在进 `skipped`（`{"id", "reason": "not_found"}`）。不再把请求体里的 id 原样当成功结果回显 | `mcp.rs:422` | `batch_delete_reports_an_unknown_id_as_skipped`（`mcp.rs:2021`）、`batch_delete_reports_every_removed_id` |
| M5 | offset 在 `i64` 里算：`(page as i64 - 1) * page_size as i64`。`page=2000000000` 不再在 `i32` 溢出 | `mcp.rs:226`（servers）、`:1031`（同形状的另一处分页） | `an_absurd_page_number_does_not_overflow_the_offset`（`mcp.rs:2038`） |
| 补 | `expires_at` 入库前按 RFC 3339 解析，失败 400 `invalid_expires_at`；trim 后空串视为 `None` | `mcp.rs:173`（`normalize_expires_at`）、`:775`（create 调用点） | `an_unparseable_expires_at_is_a_bad_request`（`mcp.rs:1426`，同时断言坏值**不落库**、带空格的合法值被 trim 后接受） |
| L1 | `with_tool_count` 改走 `count_mcp_tool_catalog`（`SELECT COUNT(*)`），不再为一个计数把整份快照拉回来 | `mcp.rs:208`；`crates/aigw-core/src/db.rs:6319`（trait）、`:6725`/`:7119`/`:7505`（三方言实现）、`:8095`（`Database` 分发） | `detail_carries_tool_count_from_the_snapshot` |
| L2 | `CatalogSnapshot::from_rows` 的 `fetched_at` 取 `max()` 而非 `rows.first()` —— 行按 `tool_name` 排序，`first()` 会让答案取决于字母序而不是写入 | `crates/aigw-core/src/models.rs:1517` | `fetched_at_is_the_snapshot_write_not_the_alphabetically_first_row`（`models.rs:1645`）、`an_empty_snapshot_has_no_timestamp_and_no_conflicts`（`models.rs:1657`）、`conflicts_are_the_sorted_deduplicated_flagged_names`（`models.rs:1665`） |
| L3 | `version_key` 末尾追加标记位：以数字结尾（正式版）补 `u32::MAX`，否则补 `0` —— `2026-07-28-alpha` 不再与 `2026-07-28` 打平 | `crates/aigw-core/src/mcp/probe.rs:263` | `newest_version_prefers_a_release_over_its_own_prerelease`（`probe.rs:589`） |

`parse_tools` 与 `parse_upstream_tool` 对「无名条目」的定义统一到 `declared_tool_name`（`probe.rs:280`）：单条目解析视为 `Malformed`，整页解析按 skip 处理。此前两者各有一套判空，H2 加严后互相矛盾、把既有 skip 语义的测试打断。

- **探测是同步请求**，一个连不通的上游会让 `probe` 端点卡到超时。缓解：单次探测超时 10s（`proxies` 的探测是异步的，因为它测的是延迟；MCP 探测要返回值给 UI，异步化收益不大）。若实测卡顿明显，改为「立即返回 `202` + 前端轮询 `GET /{id}/tools`」。
- **`conflicts` 只在探测时算**。两个 server 先后注册时，后注册的那个会看到冲突；先注册的那个不会。~~这是可接受的——UI 的「全部重新探测」能刷新出全局视图。~~ **Gate 4 H1 推翻了这条假设**：reload 的全局清空让「全局视图」不可能成立。整改后改为「reload 前一次性算出 `others`，每个探测拿同一份集合」。
- **catalog 快照的 `annotations` 列**采用 JSON 原样存，不解析。理由：`annotations` 的字段集仍在演进，解析它只会让自己被上游的格式变更拖住。

---

## 9. 对后续 Stage 的影响

- **142** 直接消费本 Stage 的端点与 `ProbeOutcome` 形状。
- **143** 复用 `mcp/probe.rs` 的双栈协商代码做**对外**协商（一侧是 `server/discover` 客户端、另一侧是服务端，二者共享协议常量与 envelope 构造）。
- **144** 在 `mcp_tool_catalog` 之上做聚合与前缀命名。
