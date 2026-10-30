# Stage 143: MCP 透传端点 + 协议双栈（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~20h（双栈协商 + 单 server 透传 + SSE 中继 + 调试口 + 调用日志 + real BDD）
**依赖**: Stage 140（凭据解析）、Stage 141（`mcp/probe.rs` 的协议常量与 envelope 构造）
**状态**: 🟡 门禁 7/8 —— 代码与三方言 BDD 全绿，Gate 4 的 HIGH 1 / MED 5 已落修；**真机验收 CC PASS / Codex 卡在客户端侧的 namespace 分发**（ADR-046），该项留作残余风险（2026-10-29）

**提交**: `51ca3a5`（server 透传/中继/调试口）→ `d20841c`（BDD + mock 上游）→ `a5fad16`（Phase 55 收官回写），均 `--signoff`，未 push。

---

## 1. 问题

到这里数据和管理都就位了，但 agent 还连不进来。本 Stage 交付**第一个能被真实 agent 使用的形态**：`/mcp/{alias}` 单 server 透传。

这是整个 Phase 里唯一必须同时满足两个**行为完全不同**的客户端的部分：Claude Code 走现代协议（`server/discover`，无会话），Codex 走旧版（`initialize` + `Mcp-Session-Id`）。两者任何一侧没实现，对应的客户端就完全不可用。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 客户端带来的 `Authorization` 是 **aigw 自己的 key**，绝不能转发给上游 | 转发等于把网关 key 泄漏给每个上游（MCP 规范 MUST NOT） | 本 Stage 的出站请求构造里，`Authorization` **只**来自 `resolve_outbound_token` 解密结果；客户端头在认证后被丢弃 |
| H2 | `GET /mcp/{alias}` 在现代协议下必须应答 **405** | 若回 404 或 500，CC 可能重试；回 405 是规范明确要求，CC 据此确认无独立通知流 | 规范 `2026-07-28` 已删除 GET 流；探针验证 CC 收到 405 后不再请求 GET |
| H3 | Codex 不发 `Mcp-Method` / `Mcp-Name`，路由必须能回落解析 body | 只认头的话 Codex 全部请求都路由不到 | 探针实测：Codex 日志中这两个头从未出现 |
| H4 | SSE 中继必须逐事件 flush 且带 `X-Accel-Buffering: no` | nginx 会缓冲整条流，工具调用看起来像卡死（本仓 websearch / OAuth 反代都踩过类似问题） | **本仓目前没有任何出站 SSE 响应设过这个头** —— `v1_messages.rs:734-740` 与 `chat.rs:1462-1470` 只设 `content-type` / `cache-control` / `connection`，全仓 grep `x-accel-buffering` 零命中。所以这是净新增工作，不是照抄；在 `relay.rs` 里设，并由 UT 断言 |
| H5 | 未知 alias 应返回 **404 + `-32601`**，不是 500 | CC 遇 500 会重试，遇 `-32601` 会走降级。500 会把一次配置错误放大成重试风暴 | 规范对未知方法的错误码定义 |

---

## 2. 边界

**做**：
- `POST /mcp/{alias}` —— 单 server 透传，双栈协商
- `GET /mcp/{alias}` —— 现代协议 405；旧版协议开 SSE 通知流
- `DELETE /mcp/{alias}` —— 旧版协议终止会话；现代 405
- `/mcp-rest/tools/list`、`/mcp-rest/tools/call`、`/mcp-rest/health` —— 无状态调试口
- `031_mcp_call_log.sql` × 三方言 + 每次调用写日志（无论成败）
- `mcp_passthrough.feature`（mock）+ `real/mcp_passthrough_real.feature`（`@real_api`，sqlite / pg / mysql 三后端）—— 两个文件，理由见 §4.3

**不做**（明确排除）：
- **不做 `/mcp` 聚合端点** —— 144
- **不做工具级 ACL** —— 144（此时只有「server 级是否可见」）
- **不做 `{alias}__{tool}` 前缀** —— 单 server 透传不加前缀（前缀是聚合端点的消歧手段，单 server 语境下无意义且会污染上游工具名）
- **不做上游会话复用** —— 每请求带凭据重连（见 §3.4）
- **不做 OAuth 发现** —— Phase 57

---

## 3. 方案

### 3.1 请求管线

```
POST /mcp/{alias}
  ├─ SpendAuth 提取 KeyIdentity（客户端 key）
  ├─ 查 mcp_servers WHERE alias=? AND enabled=1  → 无则 404 + -32601
  ├─ resolve_outbound_token(scope, user_token, server_token) → Option<TokenSource>
  │    └─ user_required 且 None → 403 insufficient_scope（本 Stage 只在 /mcp-rest/* 与
  │       单 server 透传路径生效；聚合端点的那一套在 144）
  ├─ 客户端 Authorization 丢弃，请求体原样
  ├─ 上游：按 §3.2 协商，出站注入解密后的 Authorization
  ├─ 响应中继（§3.5）
  └─ 写 mcp_call_log（无论成败）
```

### 3.2 双栈协商

**对客户端（入站）**：

| 客户端请求 | 网关行为 |
|---|---|
| `server/discover` | 回 `{resultType:"complete", supportedVersions:["2026-07-28"], capabilities:{tools:{}}, _meta:{serverInfo}}`。**不回 `initialize`** 相关任何东西 |
| `initialize` | 回 `{protocolVersion:"2025-06-18", capabilities:{tools:{}}, serverInfo}` + 响应头 `Mcp-Session-Id: <uuid>` |
| 其它方法带 `Mcp-Session-Id` | 旧版会话：网关**不自己维护会话状态**，直接把该头透传给上游（若上游是现代版则丢弃该头） |

**对上游（出站）**：复用 141 探测时的判定结果（`mcp_servers.protocol_version`）：

- 现代（`2026-07-28`）→ 直接发 `tools/call`，不带会话头
- 旧版（`2025-06-18`）→ 每次请求都先 `initialize`，拿会话头，再发目标方法，用完即弃

**为什么每次都重新 `initialize`**（不做上游会话复用）：旧版 `Mcp-Session-Id` 归属在上游，而 aigw 副本会轮换；缓存会话会在 LB 换副本时失效。代价是每请求多一次往返（本 Phase 的上游以内部服务为主，往返是局域网量级）。不引入 Redis —— 项目当前无该依赖，为一个可能用不上的场景加基础设施违反奥卡姆剃刀。

### 3.3 头体一致性校验

客户端若同时给了 `Mcp-Name` 与 body 里的 `params.name` / `params.tool`，两者不一致 → `400` + `-32020`。

**不猜哪个是真的** —— 这是一致性攻击面，也是上游 RFC 9421 签名验证失败的源头。只有 Claude Code 发这些头（H3），因此该校验对 Codex 天然不触发。

### 3.4 调试口 `/mcp-rest/*`

无状态，普通 JSON 进 JSON 出，**不走协议协商**：

| 端点 | 入参 | 出参 |
|---|---|---|
| `POST /mcp-rest/tools/list` | `{alias?}` | `{tools:[...], servers:[...]}` |
| `POST /mcp-rest/tools/call` | `{alias, tool, arguments}` | 上游原始 JSON |
| `GET /mcp-rest/health` | — | 每个 enabled server 的 `{alias, reachable, protocol_version, latency_ms}` |

存在的意义：让 BDD 不必模拟双栈握手，让运维能 `curl` 定位问题。生产可按配置关闭：`general_settings.mcp_disable_rest`（`bool`，默认 `false`），handler 层检查并直接 404 —— **不按配置条件注册路由**，因为 `config.rs:110-140` 的 `GeneralSettings` 没有 `deny_unknown_fields`，加一个 `#[serde(default)]` 字段是向后兼容的，而条件注册会让路由表随配置变形（测试里也难构造）。

### 3.5 响应中继

| 上游形态 | 网关行为 |
|---|---|
| `Content-Type: application/json` | 原样透传 body 与状态码 |
| `Content-Type: text/event-stream` | 逐事件转发：`data: ...\n\n` 立即 flush；每 15s 发注释行 `:\r\n` 心跳；响应头带 `X-Accel-Buffering: no` 与 `Cache-Control: no-cache` |
| 其它 | 原样透传 |

**心跳是唯一护栏**：`2026-07-28` 删除了 `Last-Event-ID` 续传，流断了就是丢了，只能靠心跳压住中间设备的 idle 超时。

### 3.6 `mcp_call_log`

| 列 | 说明 |
|---|---|
| `call_id` | UUID v7 —— 取 `RequestId` extension 的值（与 `chat.rs:938-944` 同源；`aigw_core::request_id` 只导出 `UuidV7RequestId: MakeRequestId`，**没有自由函数**），缺失时回退 `Uuid::now_v7()` |
| `server_alias` / `tool_name` / `method` | |
| `key_id` / `user_id` | `key_id` 存 `KeyIdentity.token_hash`（该 struct 没有 `key_id` 字段，只有 `token_hash` / `key_alias`）；`user_id` 存 `KeyIdentity.user_id` |
| `token_owner` | `server` / `user` / `anonymous` —— 回答「这次是谁的身份」 |
| `status` / `error_code` / `latency_ms` / `request_bytes` / `response_bytes` | |
| `created_at` | |

**无论成败都写** —— 失败的调用才是排障的主要输入，只记成功等于没有审计。

**列类型三方言**：`call_id` `TEXT`/`VARCHAR(64)` 主键（UUID v7 字符串，与其他表的文本 id 一致）；`latency_ms` / `request_bytes` / `response_bytes` `BIGINT`（Rust `i64`，**不要写 INT4** —— PG 的 `INTEGER` 只在该列为 NULL 时能解成 `Option<i64>`）；`key_id` / `user_id` 可空，但 **master key 时 `token_hash` 是字面量 `"*master*"`（`spend.rs:147`）而不是 `None`**，所以 `key_id` 实际恒有值，只有 `user_id` 会为 `None`；`token_owner` `VARCHAR(16)`；`status` `VARCHAR(16)`。索引 `(created_at)` + `(server_alias, created_at)` —— 144 的 ``GET /admin/mcp/calls`` 按这两维过滤与排序。

**`method` 列存的是 MCP 方法名**（`tools/list` / `tools/call` / `initialize` / `server/discover`），不是 HTTP 方法。

**不写 `spend_logs.mcp_namespaced_tool_name`** —— 该列已是 `daily_spend_queue` 聚合复合键的一部分，填值会把日聚合分组打散。MCP 的账走 `mcp_call_log`，与 LLM 用量解耦。

---

## 4. 测试

### 4.1 handler UT（≥10）

| # | 用例 | 期望 |
|---|---|---|
| 1 | 无 `Authorization` | 401（`WWW-Authenticate` 留给 Phase 57 —— 现有 `AuthError::into_response` 是纯 JSON 构造，没有加头的接缝，见 §4.3 注） |
| 2 | 未知 alias | 404 + `-32601` |
| 3 | `alias` 存在但 `enabled=0` | 404 + `-32601` |
| 4 | `server/discover` | 回 `supportedVersions`，**不含** `initialize` 的响应字段 |
| 5 | `initialize` | 回 `protocolVersion` + `Mcp-Session-Id` 响应头 |
| 6 | `GET /mcp/{alias}`（现代上游） | 405 |
| 7 | `Mcp-Name` 与 body 不一致 | 400 + `-32020` |
| 8 | 客户端 `Authorization` 与 token 表值不同 | 出站用的是 token 表的值（**断言不转发客户端头**） |
| 9 | `user_required` + 无用户 token | 403 `insufficient_scope` |
| 10 | 上游 500 | 客户端收到 500 且 `mcp_call_log` 有一行 `status=failed` |

### 4.2 透传 UT（≥6）

用每用例自绑端口的 axum router（同 Stage 141 §4.2 / ADR-041），不用 `bdd_support/mock_upstream.rs`：

| # | 用例 | 期望 |
|---|---|---|
| 1 | 上游现代版，客户端走 `server/discover` | 全程无 `initialize` 发往上游 |
| 2 | 上游旧版，客户端走 `server/discover` | 网关对客户端回现代响应，对上游走 `initialize`（**两侧独立协商**） |
| 3 | 上游 SSE | 首个事件到达 < 500ms，3 个事件顺序正确 |
| 4 | 上游 JSON | 原样透传，字节级相等 |
| 5 | 上游超时 | 504 + 日志 `error_code=timeout` |
| 6 | `/mcp-rest/tools/list` | 不触发任何协商请求 |

### 4.3 BDD

**必须是两个 feature 文件，不是一个** —— `tests/bdd.rs:136-167` 的过滤器按 `AIGW_REAL_API` 二分：`=1` 时只跑带 `@real_api` 的场景，否则跳过所有 `@real_api` 场景。同一个文件不能既做 mock 又做 real；而且 real steps 的先例（`real_websearch_steps.rs:144`）在 `real_api_enabled()` 为假时**直接 return**，混着写会让场景「通过」却什么都没断言。

| 文件 | 标签 | 跑法 | 内容 |
|---|---|---|---|
| `tests/features/mcp_passthrough.feature` | 无（mock） | `task bdd` | 自绑端口的 axum stub 当上游：`/mcp-rest/*` 完整链路（list → call → 断言上游收到注入的 `Authorization`）→ `/mcp/{alias}` 现代握手 → 旧版握手 → 断言 `mcp_call_log` 行数 |
| `tests/features/real/mcp_passthrough_real.feature` | `@real_api` | `task bdd-real-sqlite` / `-pg` / `-mysql` | `mcp_call_log` 的插入与查询在三方言下一致（真库 + 真 server） |

两个文件都要在 `tests/bdd_steps/mod.rs` 注册；real steps 用一个独立模块，开头照 `real_websearch_steps.rs:144` 的 `real_api_enabled()` 守卫。

**UT #1 的 `WWW-Authenticate` 不在本 Stage 做** —— `crates/aigw-core/src/middleware/mod.rs:64-77` 的 `AuthError::into_response` 直接构造 JSON 响应，没有加头的接缝；144 §9 已把 `auth_challenge()` 定为 Phase 57 的改造点。本 Stage 只断言 401。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/031_mcp_call_log.sql` | 新增 |
| `crates/aigw-core/src/mcp/session.rs` | 双栈协商状态机（入站 / 出站两侧） |
| `crates/aigw-core/src/mcp/relay.rs` | 响应中继（JSON / SSE + 心跳）。**`aigw-core` 没有 `tokio-stream` 依赖**（只有 `aigw-server/Cargo.toml:32` 有），而本仓唯一的 SSE 中继范式 `Body::from_stream` + `UnboundedReceiverStream` 就在 `aigw-server` 侧 —— 要么在 `aigw-core/Cargo.toml` 加 `tokio-stream = "0.1"`，要么把 `relay.rs` 放到 `crates/aigw-server/src/routes/`。**选后者**：中继的产物是 `axum::body::Body`，本来就属于 server 层，放进 core 反而要为一个只用一次的依赖加包 |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | 新增，对外端点 |
| `crates/aigw-server/src/routes/mcp_rest.rs` | 新增，调试口 |
| `crates/aigw-server/src/routes/mod.rs` | **必须加 `pub mod mcp_gateway;` 与 `pub mod mcp_rest;`** —— 否则 `main.rs` 的 `use routes::{...}` 编不过 |
| `crates/aigw-core/src/config.rs` | 加 `general_settings.mcp_disable_rest`（`bool`，`#[serde(default)]`，默认 false） |
| `crates/aigw-server/src/main.rs` | 路由注册 |
| `crates/aigw-server/tests/features/mcp_passthrough.feature` + steps | 新增 |
| `scripts/mcp-mock-upstream.py` + `Taskfile.yml` 的 `mcp-mock` | 新增，真机验收的受控上游（ADR-043，已落地）。**任务已存在的部分不改**：这条只是记录验收入口的归属，不是本 Stage 的代码交付 |

---

## 6. 回归验证

- `task test` 全量
- `task bdd` + `task bdd-real-sqlite` / `task test-db-up` + `task bdd-real-pg` / `task bdd-real-mysql`
- **真机验证（关键）**：用真实 Claude Code 与 Codex CLI 各配一次 `claude mcp add` / TOML，对 `/mcp/{alias}` 跑通一次工具调用。这是本 Stage 唯一有意义的验收方式——探针验证的是「协议形状」，真机验证的是「客户端真的会用」。上游一侧用 **`task mcp-mock`**（`scripts/mcp-mock-upstream.py`，监听 `127.0.0.1:8899/mcp`，ADR-043），`task mcp-mock -- --legacy` 可切成 405 的旧版上游
- `task fmt` / `task lint` / `task doctor`

---

## 7. 门禁

- [x] 出站 `Authorization` 恒来自 token 表（UT #8 + BDD `普通 key 调用 tools/call 不会把自己的 key 发给上游`）
- [ ] CC 与 Codex 真机各跑通一次 —— **CC 达成，Codex 只达成协议/传输层**（详见 §8.3）。两侧都用 `task mcp-mock` 起的受控上游（8899 现代 / 8900 旧版 `--legacy`），CLI 配置走隔离参数不落盘。
  - **CC 2.1.312（现代）= PASS**：`/tmp/mcp-mock.log:6` 首帧即 `server/discover`、`authorization: Bearer sk-upstream-acc-secret`，整份日志里 `claude-code` 没有发出任何 `initialize`；`:9`/`:12` 是 `tools/list`、`:10`/`:13` 是 `tools/call`（`name: "search"`、`arguments: {"query":"rust"}`），出站凭据全程是上游 secret 而非客户端虚拟 key。`/tmp/acc-cc-stream.jsonl` 的 tool_result 文本 = `mock upstream ran search with {"query": "rust"}` —— 模型确实拿到了上游结果。
  - **Codex 0.154.0（旧版）= 协议与传输 PASS，工具调用未发生**：握手与列表全程由 aigw 中继成功（`/admin/mcp/calls?server_alias=acc-legacy` 逐轮 `initialize` 217B → `notifications/initialized` 54B → `tools/list` 85B → `GET` 0B，全 `status=success`、`token_owner=server`），但**没有任何一次 Codex 回合产生 `tools/call`**。`/tmp/mcp-mock-legacy.log:108` 那条 `tools/call`（`authorization: Bearer sk-upstream-legacy-secret`、`session_id: mock-session-1`、`params.name=search`）经字节数核对**是手工 curl，不是 Codex**：其 `request_bytes=102` 精确等于 `{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search","arguments":{"query":"rust"}}}` 的长度，而同 shell 前一条 `tools/list` 的 46B 也精确等于手写 body；Codex 自己发的 `tools/list` 是 85B（带 `_meta.progressToken`）、`initialize` 是 217B（带 `clientInfo`），都不匹配。
  - **H1 两侧都成立**：`grep -c sk-Xc9_c2gG3mM1V2VnryDPOA /tmp/mcp-mock.log /tmp/mcp-mock-legacy.log` = `0` / `0`；两个 mock 只见到 `sk-upstream-acc-secret`（9 次）与 `sk-upstream-legacy-secret`（110 次）。客户端始终是同一把虚拟 key，而两个 alias 用的是两条不同的上游 secret。
  - **两处协议假设被印证**：CC 先发 `server/discover` 且整会话无 `initialize`（ADR-040 的现代路径）；Codex 只发 `initialize`、从不发 `server/discover`（legacy 日志里 `server/discover` 仅 2 条：`:3` 无鉴权冒烟、`:8` 带 sealed token 的 curl 探测，均非 Codex）。`/tmp/mcp-mock.log:14` 那条 `initialize` 的 `clientInfo.name=codex-mcp-client 0.154.0` 是绕过 aigw 直连 mock 的对照实验。
  - **`x-call-id` 与 `call_id` 同值**：手工 `tools/list` 响应头 `x-call-id: 01a1847c-8ca9-7ae1-9156-07b058d70107`，`mcp_call_log.call_id` 同值（§8.1 那条修复的端到端印证）。
- [x] `GET /mcp/{alias}` 现代 405 / 旧版 SSE 双分支 —— `legacy_get_opens_a_notification_stream` 断言 200 / `text/event-stream` / `x-accel-buffering: no` / 首帧心跳 / 恰好一行 `method=GET`；现代 405 由 `现代协议下 GET 返回 405` 覆盖
- [x] SSE 首事件 < 500ms，心跳可见 —— 同上
- [x] `mcp_call_log` 失败也写 —— `未知 alias 是 404 且仍然留痕` 断言 `status=failed`；ADR-045 之后 ACL 拒绝也落一行
- [x] 三方言 real BDD 全绿 —— SQLite `/tmp/stage143-bdd-real-sqlite.log`、PG `/tmp/final-pg.log`、MySQL `/tmp/final-mysql.log`，三者同为 19 features / 80 scenarios (80 passed) / 452 steps，`✘`=0，退出码 0
- [x] `task test` + `task bdd` + `fmt` + `lint` green —— **Gate 4 整改后的全量复跑（主 Agent 亲读）**：`/tmp/p55-final-test3.log` **1508 passed / 0 failed / 13 ignored / 22 个 test result**，BDD 半程 **47 features / 362 scenarios (349 passed, 13 skipped) / 2109 steps**、`✘`=0；单跑 `task bdd` `/tmp/p55-bdd-fix.log` 同数字；`/tmp/p55-final-lint.log` `lint passed`；`/tmp/p55-final-fmt.log` `fmt passed`。**此前引用的 `/tmp/stage143-bdd.log` 有一处假绿**：`mcp_aggregate.feature` 的 5 个 `tools/list` 场景因 step expr 里裸 `/` 被当作 alternation 而整段 skipped（该文件属 144，但 143 的 Gate 3 引用的是同一轮日志），现已修并在新日志里真实执行通过。剩余 13 个 skip 全在 `body_archive_*`，见 TD-028

**ADR-045 追加门禁（同日新开）**：ACL 在 `/mcp/{alias}` 与 `/mcp-rest/tools/call` 上同样强制 —— 4 条 BDD（两条拒绝、两条正对照）在 `/tmp/stage143-bdd.log:1199-1245` 全绿。

---

## 8. 风险与遗留

- **真机验证只能在本 Stage 做** —— BDD 用 mock upstream 验证的是协议形状，不能证明 CC / Codex 会接受。若真机不通，除了改协议实现外没有别的路。这决定了本 Stage 的预估可能偏乐观。
  - **两台 CLI 已在本机确认存在**：`claude 2.1.312`（双栈）、`codex-cli 0.154.0`（纯旧版）。验收脚本要能在**不依赖外部真实上游**的情况下跑通 —— 上游一侧用 Stage 141 §4.2 那套自绑端口的 axum router 起一个 mock MCP server，注册进 `mcp_servers` 后让 CC / Codex 连 `/mcp/{alias}`。否则「真机验证」会被一个连不通的公网上游阻断，变成不可复现的验收。**跨进程版已落地**：`task mcp-mock`（ADR-043）起一个 Python 的 JSON-RPC 上游在 `127.0.0.1:8899/mcp`，把每个请求的 method / `Authorization` / `Mcp-Session-Id` 打成一行 JSON 到 stderr —— 那份日志正是下面「验收证据」两条断言的抓取手段。
  - **`claude mcp add` 走的是 `--transport http`**（Streamable HTTP），不要用 `sse`（那是旧版 HTTP+SSE 传输，与 `Mcp-Session-Id` 语义不同）。Codex 的 TOML 段落在 `~/.codex/config.toml` 的 `[mcp_servers.<name>]`，`url` 键。
  - **验收证据**：CC 侧抓 `/mcp/{alias}` 的请求体，确认它先发 `server/discover` 且整会话没有 `initialize`；Codex 侧确认它只发 `initialize`。这两条断言正是 ADR-040 的调研结论，验收时重放一次，协议假设才算被本 Stage 的实现印证。
- **SSE 心跳 15s 是拍脑袋的数** —— 常见中间设备 idle 超时是 30s–60s，取 15s 留一半余量。若实测发现某些反代在 10s 就断，需要下调。
- **响应压缩层不会压 SSE（已核实，无需处理）** —— `main.rs:1134` 的 `build_compression_layer` 用的是 `CompressionLayer::new()` 默认谓词，而 `DefaultPredicate` 由 `SizeAbove` + `NotForContentType::{GRPC, IMAGES, SSE}` 组成（`tower-http-0.6.11/src/compression/predicate.rs:119-128`）。`text/event-stream` 已在排除名单里，逐事件 flush 不会被压缩层攒住。**唯一仍要自己做的是 `X-Accel-Buffering: no`** —— 那是给 nginx 看的，tower-http 管不到。
- **旧版上游的「每请求 initialize」有性能代价** —— 每请求多一次往返。若实测发现某上游对 `initialize` 限流，需改为「按 (server_alias, key_id) 缓存会话 + 失效重试」。当前不做是因为没有证据表明这是瓶颈。
- **`/mcp-rest/*` 生产可开可关** —— 它是无认证旁路吗？不是：它仍走 `SpendAuth`。但它跳过了协议协商，若被误用于生产流量，行为会与 agent 路径不同。默认建议生产关闭，通过配置项控制。

---

### 8.1 实施期修正（2026-10-29）

- **`x-call-id` 响应头原本根本不产生** —— `main.rs` 只挂了 `tower_http::request_id::PropagateRequestIdLayer::new(x-call-id)`，而该 layer 的语义是「把**同名请求头**复制到响应」。`SetRequestIdLayer` 设的是 `x-request-id`，所以除非客户端自己带 `x-call-id`，响应永远不会有这个头。`responses.rs:1705`/`:1423` 之所以过 TD-006 是它们**显式写**了该头。修法：抽 `mcp_relay::stamp_call_id`，五条路径共用 —— `relay()`（buffered + SSE，在 `row` 移入 SSE task 前 clone id）、`finish`、legacy `open_stream` 分支、`mcp_rest.rs` 的 `tools_list` 与 `tools_call`（后两条不经 `finish`，是最初漏掉的两处）。
- **cucumber-expression 里的 `/` 是交替符**（`cucumber-expressions-0.3.0/src/parse.rs:414`，报错文案 `:691` 提示写 `\/`）。所以 `#[step(expr = "… tools/call …")]` 会被解析成 `tools` 或 `call` 两个分支；含路径的 step 必须写 `tools\/call`。**feature 文件里的文本不要转义** —— 那里是原始文本，`\/` 只会让 step 匹配不上（本 Stage 在这里踩过一次，两个 ACL 场景一度被静默跳过）。
- **ADR-045：ACL 只在聚合端点上生效是个洞** —— 原实现 `acl::is_allowed` 全仓只有一个调用点 `call_aggregate_tool`，而 `/mcp/{alias}` 与 `/mcp-rest/tools/call` 同样把请求送到同一批上游、且 `mcp_disable_rest` 默认 `false`、`/v1/mcp/servers` 又把 alias 全量暴露。现抽 `check_tool_allowed`（`mcp_gateway.rs:234`）由三处共用，顺序统一为 registry → ACL → credential。
  - `post_mcp` 里的次序是 **registry → envelope 一致性（400）→ ACL → credential**：`Mcp-Name` 与 body 工具名不一致是请求本身坏了，先查 ACL 会把 400 掩盖成 403，而既有场景 `Mcp-Name 与 body 工具名不一致是 400` 依赖这个次序。
  - `tools/list` **不查** ACL —— 列表照给、调用才封，这是 Stage 144 §3.2 与 BDD `ACL 拒绝只封调用，列表仍然照给` 钉死的行为。
- **`catalog::page` 的下标游标在目录变动时会漂移** —— 已登记 TD-024，不改 key-based 游标的理由在同处；`merge` 的 `has_conflict` 不上线（`to_wire()` 不发该字段），一并在 TD-024 记录。

---

### 8.2 Gate 4 独立复查与整改（2026-10-29，Phase 55 收官批）

独立 reviewer 覆盖 Stage 143/144/145 三份，总判定 **`BLOCK`**：HIGH 1 / MED 6 / LOW 5。六条「必检结论」五条 PASS（凭据方向、ACL 强制、`mcp_call_log` 每路径恰一行、三方言 SQL、并发无跨 await 持锁、前端 token 处理），缺陷集中在「名义通过、实际未覆盖」与「日志与实际行为不符」两类。主 Agent 逐条读码复核后分三批处置。

**落修（本 Stage 范围内）**

| # | 级别 | 缺陷 | 修法 | 回归 |
|---|---|---|---|---|
| 1 | HIGH | `/mcp-rest/tools/list` 返回的 `UpstreamTool` 字段是 `tool_name`，前端非 admin 分支读 `tk.name` → 普通用户的 Playground 工具下拉永远为空，`send()` 因 `!tool` 直接 return | 前端改读 `tool_name`（`McpPanel.tsx:79`、`RestToolEntry` 接口同步），后端不动（`tool_name` 是 `stage-141.md` §3.3 钉死的列名契约） | fe-bdd 新增 `A non-admin sees the tools of the server it selected`（`playground.feature:137`） |
| 2 | MED | `resolve_bearer` 在 token 过期/空值时返回 `(None, token_owner(source))` —— 出站无 `Authorization`，账上却写 `server`/`user` | 两条分支改 `token_owner(None)`（= `"anonymous"`），只有真正解出明文才写 source（`mcp_gateway.rs:195-210`）。`anonymous` 是本 Stage 已有的词表（`token_owner()` 三值之一），不新增枚举 | `an_expired_shared_token_sends_no_authorization_and_logs_anonymous`（`:1722`） |
| 3 | MED | 上游 HTTP 200 + JSON-RPC `error` 被记为 `success` —— `finish` 用 `Outcome::is_failure` 判失败，中继路径只看 HTTP 状态 | buffered 路径解析 body 复用同一套判定（`jsonrpc_error_code`，`mcp_relay.rs:114`）；SSE 路径无整包可解析，保留原规则并在该函数文档写明 | `a_jsonrpc_error_behind_http_200_is_relayed_and_logged_as_failed`（`:1831`） |
| 4 | MED | `tool_name_of` 先取 `params.name` 再兜底 `params.tool`，ACL 只校验取到的那个，而整个 body 会转发 → 用另一个字段可绕过 ACL | 新增 `check_tool_spellings_consistency`（`session.rs:182`）：两个拼写都在且不一致 → 400，与 `Mcp-Name` 不一致同规格（都是「请求本身坏了」，不是权限问题）；聚合改写处顺手 `params.remove("tool")`，不留第二个未校验的拼写（`mcp_gateway.rs:968`） | 单测 5 条（`session.rs:417-428`）+ 两条 handler 路径各接一次（`:424`、`:667`） |
| 5 | MED | `/v1/mcp/.../my-token` 的 `token_name` 只有 trim+非空，无长度上限 → MySQL `VARCHAR(255)` 直接 500 | 复用 `mcp::validate_len(token_name, "token_name", MAX_NAME_LEN, "invalid_token_name")`（`mcp_user.rs:197`），与 admin 的 `create_mcp_token` 同规格 | `mcp_user.rs` 既有 5 条 UT + 新增超长用例 |
| 6 | MED | `shared` + 无 token 时「上游无 `Authorization` 头」只在设计文档里，无断言 —— `Recorder.authorizations` 的唯一消费者是 `the_outbound_bearer_comes_from_the_token_table_not_the_client`，全仓无 `vec![None]` 断言 | 新增 `a_shared_server_without_a_token_sends_no_authorization_at_all`（`mcp_gateway.rs:1656`） | 同上，`assert_eq!(authorizations, vec![None])` |

**登记不修（TD-026，六项）** —— MED#6 `mcp_disable_rest` 默认 `false`、MED#7 SSE `unbounded_channel` 无背压、LOW#8 缓存写序与 `ttl_ms` 无上限、LOW#9 `call_status_filter` 未知状态透传、LOW#10 `MyTokenDialog` 吞错、LOW#12 聚合列表静默剔除。逐条的文件行号与「为何不现在修」见 `docs/12-technical-debt.md` TD-026。

**一处原判定被本批推翻**：HIGH#1 之所以能通过全部既有测试，是因为前后端 mock 各自自证 —— 后端 UT 断言 `tool_name`、前端 fe-bdd 的 mock 用 `{name}`，两边从未对撞。这条不是「漏写用例」，而是「mock 与被测代码同步写错」。补的 BDD 场景走真实后端契约（mock 改 `tool_name`），性质不同。

---

### 8.3 真机验收的残余风险（2026-10-29，ADR-046）

**CC 端到端达成，Codex 只达成协议/传输层。** 该项门禁**未勾选**，Stage 143 记为 🟡 7/8。

- **Codex 的断点在客户端，不在网关。** Codex 0.154.0 给模型的工具清单只有 namespace `mcp__acc_legacy` 加裸子名（`search` / `read_document`），没有平铺的 `mcp__acc_legacy__search`；模型被提示成平铺名后 `codex_core::tools::router` 本地直接回 `unsupported call`（`duration_ms=0`，不发 HTTP）。**对照实验把网关排除**：上游直接指向 `127.0.0.1:8899/mcp`（完全绕过 aigw）得到同样的 `unsupported call`。
- **已试过的无效绕法**（全部记录在 ADR-046）：`--disable tool_search_always_defer_mcp_tools`、`--enable tool_search`、`--enable non_prefixed_mcp_tool_names`（变成 `acc_legacy__search`，仍 unsupported）、`--enable code_mode`（`exec` 回 `tool exec invoked with incompatible payload`）、换模型。
- **一条曾被误判的证据**：`/admin/mcp/calls` 里 `22:01:55 acc-legacy tools/call` 那条**是手工 curl，不是 Codex**。判别靠字节数 —— 该行 `request_bytes=102` 精确等于手写 body，同 shell 前一条 `tools/list` 是 46B；Codex 自己发的 `tools/list` 是 85B（带 `_meta.progressToken`）、`initialize` 是 217B（带 `clientInfo`）。UUIDv7 前缀 `01a18488` 覆盖 22:01:13–22:01:55 整段，手工与 Codex 同前缀，前缀**不能**作为归属依据。
- **`mcp_call_log` 没有客户端指纹** —— `request_bytes` 只是入站 body 长度。若后续验收要机器可判的归因，应在日志里落 `clientInfo.name/version`（TD 候选，本次未登记）。
- **与 MCP 无关但会挡住 code-mode 客户端的一条**：aigw 把 Responses 的 `custom` 工具降级成 `{"input": string}` 的 function（`adapter.rs:2055`、`:2543`），出站只回 `function_call`（`:2861`、`:3059`、`:3324`、`:3374`），不还原 `custom_tool_call`。

---

## 9. 对后续 Stage 的影响

- **144** 在 `/mcp/{alias}` 之外加 `/mcp`，复用本 Stage 的出站 client、中继、日志写入；聚合端点额外做前缀命名与 ACL。
- **145** 的 Playground MCP 标签直接调 `/mcp-rest/tools/call`。
- 本 Stage 的「`user_required` 无 token → 403」只在**显式点名调用**时触发。`tools/list` 侧的静默剔除是 144 的聚合端点行为（单 server 透传下，`tools/list` 若该 server 对此用户不可见，应返回空列表而不是 403）。
