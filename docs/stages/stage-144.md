# Stage 144: MCP 聚合端点 + 用户凭据 API（Phase 55）

**所属**: Phase 55（MCP Gateway）
**预估**: ~20h（聚合 + 前缀 + ACL + 用户凭据端点 + 调用日志端点 + real BDD）
**依赖**: Stage 143（出站 client / 中继 / 日志写入）、Stage 140（`CredentialScope` 与 `resolve_outbound_token`）
**状态**: ✅ 完成（2026-10-29）—— Gate 3 四门禁全绿；ACL 覆盖面缺口转 ADR-045 由 Stage 143 路径补齐

**提交**: —

---

## 1. 问题

Stage 143 做的是「单 server 透传」——一个 alias 对应一个上游，agent 要接三个上游就得配三个 MCP 条目。**聚合才是网关真正的价值**：agent 只配一个 `/mcp`，拿到的是 N 个上游合并后的工具目录。

同一 Stage 补齐用户级凭据：`shared_or_user` / `user_required` 两档在 143 只是「解析函数写得对」，真正的产品行为（列表里可见 / 不可见、调用时 403、用户自己填 token）都在这里落地。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 工具名冲突是常态，不能静默覆盖 | 覆盖会让模型看到的名字指向错误实现，排查成本极高 | 不同上游提供同名工具是 MCP 生态的已知现象（`search` / `get_issue` 之类） |
| H2 | 前缀拆分只能找**第一个** `__` | 上游工具名本身可能含 `_`；贪心拆分会把 `a__get_user_issue` 拆成 `a__get` + `user_issue` | `alias` 注册时禁止含 `__` 且禁止下划线结尾（Stage 140 §3.6），使第一个 `__` 必然是真分隔符 |
| H3 | 排序必须确定性（按 `alias, tool`） | 顺序抖动一次，上游 prompt cache 前缀全部作废，成本翻倍 | 规范是 SHOULD，但这是 prompt cache 的前提 |
| H4 | `user_required` 无 token 时，列表侧应**静默剔除**而非报错 | 报错会让整个 `tools/list` 失败，一个用户没配 token 就看不到**所有** server 的工具 | 用户不配 token 是常态操作，不是一个错误状态 |
| H5 | 聚合的 `tools/list` 翻页必须由网关**自己抹平** | CC 会跟随 `nextCursor`；若网关把上游的分页游标直接透出去，CC 拿到的第二页是某个特定上游的，其它上游的工具永远看不到 | 探针实测 CC 会跟随 `nextCursor` |

---

## 2. 边界

**做**：
- `POST /mcp` 聚合端点（`tools/list` 合并 / `tools/call` 拆前缀路由）
- `{alias}__{tool}` 前缀命名 + 冲突检测
- 工具级 ACL：`032_mcp_tool_allowlist.sql` × 三方言 + `PUT /admin/mcp/servers/{id}/allowlist`
- `GET /admin/mcp/calls`（调用日志查询）
- 用户级凭据接线：`shared_or_user` / `user_required` 的列表侧与调用侧行为
- `GET /v1/mcp/servers`、`GET/PUT/DELETE /v1/mcp/servers/{id}/my-token`
- `mcp_aggregate.feature`（mock）+ `real/mcp_aggregate_real.feature`（`@real_api`，sqlite / pg / mysql 三后端）—— 两个文件，理由见 §4.4

**不做**（明确排除）：
- **不做前端** —— 142 的 `CallLogDialog` / ACL 开关由 145 一并补（见 §9）
- **不做 OAuth 发现** —— Phase 56
- **不做按 key 隔离的上游授权** —— 项目无此机制，且多数上游不支持细粒度授权
- **不做跨 server 的工具重命名/别名表** —— 用户改 alias 即可，加映射表是过度设计

---

## 3. 方案

### 3.1 `tools/list` 聚合

```
POST /mcp  (method=tools/list)
  ├─ 认证 → KeyIdentity
  ├─ 可见 server 集合 = enabled=1
  │    └─ user_required 且该用户无可用 user token → 剔除（静默）
  ├─ 逐 server 取 catalog（内存缓存，按 ttlMs 过期；未命中则连上游拉全翻页）
  ├─ 合并：每个工具名改为 "{alias}__{tool}"
  ├─ 冲突检测：合并后仍重名 → 标 has_conflict（不覆盖、不丢弃）
  ├─ 稳定排序（alias, tool）
  └─ 网关自己分页：nextCursor 为网关生成的序号，与上游游标无关
```

**`tools/list` 不做 ACL 过滤** —— 本节的流水线原先写了「逐工具 ACL 过滤」，与 §3.3 冲突，已删。§3.3 的口径是「列表照给、调用明确拒绝」：ACL 是管理员的明确意图，让它从列表消失等于隐藏了「管理员禁止了它」这一事实，用户会以为上游没提供。ACL 只在 `tools/call` 上生效（§3.2）。

**分页由网关自己生成** —— 若直接透出上游游标，CC 跟到第二页时拿到的只是某一个上游的剩余工具。网关的做法是：把合并后的完整列表算出来，按固定页大小（建议 100）切片，游标就是起始下标。

**冲突不静默覆盖**：两个 server 提供同名工具时，合并后的名字 `a__search` 与 `b__search` 本来就不同，不会冲突。真正的冲突发生在 **alias 相同**（不可能，alias 唯一）或**合并后名字仍相同**（比如 `a__b` + `c` 与 `a` + `b__c` 都得到 `a__b__c`）——后者被 Stage 140 的 alias 规则（禁含 `__`）堵死。因此 `has_conflict` 更多是给「探测时发现两个 server 有同名工具」用的**提示性标记**，而非合并时的阻断条件。

### 3.2 `tools/call` 路由

```
POST /mcp  (method=tools/call, params.name = "github__search")
  ├─ 头体一致性校验（同 143）
  ├─ 拆第一个 "__"：alias="github", tool="search"
  ├─ alias 已注册？ → 否则 404 + -32601
  ├─ 该 server 对此用户可见？（user_required + 无 token → 403 insufficient_scope）
  ├─ ACL 放行？ → 否则 403 + insufficient_scope
  ├─ 解析出站凭据
  ├─ 上游 tools/call，剥前缀，注入 Authorization
  └─ 中继 + 写 mcp_call_log
```

**未知 alias 返回 `-32601` 而不是 500** —— CC 遇 500 会重试，遇 `-32601` 走降级。一次配置错误不该被放大成重试风暴。

### 3.3 工具级 ACL

表 `mcp_tool_allowlist`：

| 列 | 说明 |
|---|---|
| `server_id` / `tool_name` | 定位一个工具 |
| `scope_type` | `key` / `team` / `org` |
| `scope_id` | `scope_type=key` 时存 `KeyIdentity.token_hash`（`KeyIdentity` 无 `key_id` 字段）；`team` / `org` 存对应的 `team_id` / `organization_id` |
| `allowed` | 布尔 |

**行不存在 = 不限制**。这个默认选「放行」而非「拒绝」，理由：ACL 是渐进加上的管控手段，若默认拒绝，新加一个 server 时所有工具立刻不可用，管理员必须先穷举白名单才能用——这与「先能用再收紧」的运维现实相反。

**fail-closed 只封调用，不封列表**：
- **列表侧给该工具（模型看得见），调用侧被 ACL 拒 → 403 + `insufficient_scope`**

给多了（看见了调不动）比给少了（看不见）更糟？——这里反过来：ACL 是**明确的管理员意图**（「这个工具不给这个 team」），让它从列表消失等于隐藏了「管理员禁止了它」这一事实，用户会以为是上游没提供。所以列表照给，调用明确拒绝。

（与 `user_required` 的处理**刻意相反**：那里隐藏的是「你还没配 token」，是一个用户可自行解决的状态；这里隐藏的是「管理员禁止了你」，隐藏会误导。两种措辞对应两种不同的事实。）

**`PUT /admin/mcp/servers/{id}/allowlist` 的契约**（145 的 `ToolsDialog` 直接照此编码）：

```json
// 请求：整表替换该 server 的规则（不是增量）
{"rules": [{"tool_name": "search", "scope_type": "team", "scope_id": "t_1", "allowed": false}]}
// 响应：{"object": "list", "data": [ ...同上结构... ], "count": 1}
```

`GET /admin/mcp/servers/{id}/allowlist` 回同一形状。**整表替换**而非增量，因为 ACL 的编辑单位就是「这个 server 的当前规则集」，PATCH 语义会让「删掉一条规则」与「没设过」无从区分。唯一约束 `(server_id, tool_name, scope_type, scope_id)`；跨 server 的 `tool_name` 不需要存在性校验（规则可以先于工具存在，探测失败时也不该让编辑失败）。

### 3.4 用户凭据 API

| 端点 | 行为 |
|---|---|
| `GET /v1/mcp/servers` | 当前身份可见的 server 列表 + 每项 `credential_scope` / `has_my_token` / `token_required` |
| `GET /v1/mcp/servers/{id}/my-token` | 元数据（`token_name` / `is_default` / `expires_at` / `updated_at`），值恒 `***`；未配置返回 404 而非空对象 |
| `PUT /v1/mcp/servers/{id}/my-token` | 设置；body `{token_name, token_value, expires_at?}`，upsert 到 `(server_id, "user", user_id, token_name)` |
| `DELETE /v1/mcp/servers/{id}/my-token` | 删除我的全部 token（该 server 下 `owner_type=user AND owner_id=me`） |

**身份只从 `KeyIdentity` 取，不接受 `owner_id` 入参** —— 这是防越权代填的编译期/运行期双重保证：handler 签名里没有这个参数，也就没有注入点。

**master key 调这些端点**：`user_id` 为 `None` → 返回 400「该 key 未绑定用户，无法保存个人凭据」。不静默当作 admin 全权处理，因为「以谁的身份调上游」这个问题在 master key 上没有答案。

**这个 400 在 UI 会话下不会触发，只会撞到 master key 直连 API 的调用者** —— `crates/aigw-server/src/routes/login.rs:138-139` 里 admin 登录固定写 `user_id = "default_user_id"`，所以 admin 走 UI 时**有** `user_id`。真正会撞上 400 的是用 master key 直接打 API（curl / 脚本）的调用者。145 的「我的凭据」tab 因此不必为 admin 特判，但要为「接口返回 400」准备一条可读的 `toast`（把后端的 message 直接透出，它是中文语义完整的）。

### 3.5 调用日志查询

`GET /admin/mcp/calls`：分页 + 按 `server_alias` / `tool_name` / `status` / 时间范围过滤。响应信封同 Stage 141 §3.2。

**每行字段与 `mcp_call_log` 的列一一对应**（`call_id` / `server_alias` / `tool_name` / `method` / `key_id` / `user_id` / `token_owner` / `status` / `error_code` / `latency_ms` / `request_bytes` / `response_bytes` / `created_at`），145 的 `CallLogDialog` 直接渲染这些字段，不做二次聚合。默认按 `created_at DESC` 排序；时间范围用 `from` / `to`（RFC 3339，闭区间）。

---

## 4. 测试

### 4.1 聚合 UT（≥8）

| # | 用例 | 期望 |
|---|---|---|
| 1 | 两个 server 各有 2 个工具 | 合并后 4 个，名字带正确前缀 |
| 2 | 工具名含 `_`（如 `get_user_issue`） | 拆分为 `alias` + `get_user_issue`，不误拆 |
| 3 | 上游翻页两页 | 合并结果包含两页全部工具 |
| 4 | 网关分页 | `nextCursor` 是网关序号，第二页内容与第一页不重叠 |
| 5 | 排序 | 两次调用顺序完全一致 |
| 6 | catalog 缓存未过期 | 不连上游（mock 断言零请求）—— 用例前置须 `catalog_cache::clear()`（ADR-041：缓存是进程级静态，跨用例共享） |
| 7 | 缓存过期 | 连上游 |
| 8 | 未知前缀调用 | 404 + `-32601` |

### 4.2 凭据 UT（≥6）

| # | 用例 | 期望 |
|---|---|---|
| 1 | `user_required` + 无 user token | `tools/list` 里**没有**该 server 的工具，**不报错** |
| 2 | `user_required` + 无 user token + 显式调用 | 403 `insufficient_scope`，消息带 server 名 |
| 3 | `shared_or_user` + 两者都有 | 上游收到的是 user token |
| 4 | `shared_or_user` + 只有共享 | 上游收到的是共享 token |
| 5 | `shared` + 都无 | 上游请求**无** `Authorization` 头 |
| 6 | 用户 PUT 自己的 token | 落库，DB 直读为 `v2:gcm:` 密文 |

### 4.3 ACL UT（≥4）

| # | 用例 | 期望 |
|---|---|---|
| 1 | 无 ACL 行 | 允许 |
| 2 | `allowed=0` 精确匹配 key | `tools/list` 仍含该工具；`tools/call` 403 |
| 3 | `allowed=0` 匹配 team（key 无专属行） | 403 |
| 4 | key 行 `allowed=1` 覆盖 team 行的 `allowed=0` | 允许（key 优先） |

**「精确匹配 key」用的是 `KeyIdentity.token_hash`，不是 `key_id`** —— `KeyIdentity`（`crates/aigw-core/src/middleware/mod.rs:36-45`）只有 `token_hash` / `key_alias` / `user_id` / `team_id` / `organization_id` / `is_master_key` / `user_role`，没有 `key_id`。`scope_type=key` 的 `scope_id` 就存 `token_hash`，与 `mcp_call_log.key_id` 同源（143 §3.6）。

### 4.4 BDD

**两个文件，不是一个**（理由同 143 §4.3：`tests/bdd.rs:136-167` 按 `AIGW_REAL_API` 二分，`real_*_steps` 在非 real 模式下直接 return，混写等于零断言）：

| 文件 | 标签 | 跑法 | 内容 |
|---|---|---|---|
| `tests/features/mcp_aggregate.feature` | 无（mock） | `task bdd` | 两个自绑端口的 axum stub 当上游：合并 → 前缀可辨 → 调用正确路由到对应上游 → 冲突提示 → `user_required` 列表剔除 → 补 token 后可见 → ACL 拒绝 403 → 调用日志可查 |
| `tests/features/real/mcp_aggregate_real.feature` | `@real_api` | `task bdd-real-sqlite` / `-pg` / `-mysql` | `mcp_tool_allowlist` 与 `mcp_call_log` 的三方言读写一致 |

两个 feature 的 steps 都要在 `tests/bdd_steps/mod.rs` 注册。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/migrations/{sqlite,postgres,mysql}/032_mcp_tool_allowlist.sql` | 新增 |
| `crates/aigw-core/src/mcp/catalog.rs` | 聚合 / 前缀 / 冲突 / 分页 |
| `crates/aigw-core/src/mcp/acl.rs` | ACL 判定（key > team > org 优先级） |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | 加 `/mcp` 聚合路由 |
| `crates/aigw-server/src/routes/mcp_user.rs` | 新增，`/v1/mcp/*` |
| `crates/aigw-server/src/routes/mcp.rs` | 加 allowlist / calls 端点 |
| `crates/aigw-server/src/routes/mod.rs` | **必须加 `pub mod mcp_user;`** —— 否则 `main.rs` 的 `use routes::mcp_user` 编不过 |
| `crates/aigw-server/src/main.rs` | 路由注册 |
| `crates/aigw-server/tests/features/mcp_aggregate.feature` + steps | 新增 |

---

## 6. 回归验证

- `task test` 全量
- `task bdd` + `task bdd-real-sqlite` / `task test-db-up` + `task bdd-real-pg` / `task bdd-real-mysql`
- **真机验证**：CC 配 `/mcp`（不是 `/mcp/{alias}`），确认它能拿到合并目录并成功调两个不同上游的工具。两个上游都起 `task mcp-mock`（ADR-043，`127.0.0.1:8899`），第二个用 `-- --port 8900` 换端口，注册成两个 alias 即可
- `task fmt` / `task lint` / `task doctor`

---

## 7. 门禁

- [x] 前缀拆分用第一个 `__`，工具名含 `_` 不误拆 —— `catalog.rs:62` 的 `split_prefixed` 用 `split_once`；UT `split_takes_the_first_separator_only`（`:187`，含 `a__b__c → ("a","b__c")` 与 `__leading`/`trailing__` 两侧空半名）、`underscore_in_an_upstream_tool_name_is_not_a_separator`（`:177`）；BDD `工具名含下划线时按第一个双下划线拆分`（`mcp_aggregate.feature:22`）
- [x] 工具排序两次调用完全一致 —— UT `ordering_is_identical_across_calls`（`catalog.rs:209`，两个上游 × 5 工具连建两次比对名字序）
- [x] `user_required` 无 token：列表静默剔除、调用 403，两者都有测试 —— BDD `user_required 且未配置个人 token 时该上游被静默剔除`（`:64`，断言目录里无 `vault__secret`、工具数为 2）+ `user_required 且无个人 token 时显式调用是 403`（`:81`，断言 `error.code = -32003`）；反向对照 `补上个人 token 后该上游的工具出现`（`:72`）。**注意**：这三条里有两条的 `When` 是 `tools/list`，此前 expr 写了裸 `/` 被 cucumber 当成 alternation，整段一直静默 skipped（TD-028），本 Stage 的 Gate 3 曾把「通过」归给它们。收口时把 expr 改为 `tools\/list` 后首次真正执行，在 `/tmp/p55-bdd-fix.log:1078-1100` 全绿 —— 断言本身成立，但此前的引用不成立
- [x] `shared` + 无 token：上游**无** Authorization 头 —— Gate 4 MED#2 整改后补了 `a_shared_server_without_a_token_sends_no_authorization_at_all`（`mcp_gateway.rs:1686`，断言 `Recorder.authorizations == vec![None]`，即整轮只发生一次上游请求且不带该头）与 `an_expired_shared_token_sends_no_authorization_and_logs_anonymous`（`:1722`，过期共享 token 同样不发头且日志 `token_owner` 为匿名）；此前 `Recorder.authorizations` 的唯一消费者是 `the_outbound_bearer_comes_from_the_token_table_not_the_client`，全仓无 `vec![None]` 断言。两条均在 `/tmp/p55-final-test3.log` 通过
- [x] 用户 token 落库为密文 —— `mcp_user.rs:461-475` 断言库里 `token_value` 以 `v2:gcm:` 开头、不含明文 `plain-secret-value`，且 `unseal_token(value, MK)` 能还原；另有 `:481-504` 覆盖轮换
- [x] `/v1/mcp/*` 签名不含 `owner_id` 入参 —— 四个 handler 的提取器只有 `State<SharedState>` / `KeyIdentity` / `Path<i64>` / `Json<SetMyTokenBody>`，身份一律经 `user_id_of(auth)`（`mcp_user.rs:52`）取得；`owner_id` 在文件里只作为写入值（`:243`）与过滤条件（`:75`）出现
- [x] 三方言 real BDD 全绿 —— SQLite / PG `/tmp/final-pg.log` / MySQL `/tmp/final-mysql.log` 同为 19 features / 80 scenarios (80 passed) / 452 steps，`✘`=0，退出码 0（含本 Stage 的 `real/mcp_aggregate_real.feature`）
- [x] `task test` + `task bdd` + `fmt` + `lint` green —— Gate 4 整改落定后的全量复跑（主 Agent 亲读）：`/tmp/p55-final-test3.log` **1508 passed / 0 failed / 13 ignored / 22 个 test result**，其中 BDD 半程 **47 features / 362 scenarios (349 passed, 13 skipped) / 2109 steps**、`✘`=0；`/tmp/p55-bdd-fix.log`（单独 `task bdd`）同数字、`bdd passed`；`/tmp/p55-final-lint.log` `lint passed`；`/tmp/p55-final-fmt.log` `fmt passed`（此前 3 个文件的格式差异已由 `task fmt-fix` 收口）。13 个 skip 全部在 `body_archive_read/write.feature`，与本 Stage 无关，已登记 TD-028 🟡

---

## 8. 风险与遗留

**Gate 3 最终证据（2026-10-29，主 Agent 亲读日志）**：`task test` `/tmp/agg_test2.log` **1497 passed / 0 failed / 22 个 test result**；`task bdd` `/tmp/agg_bdd.log` 358 场景 340 passed / 18 skipped、2045 steps、`✘`=0；`task fmt` `/tmp/agg_fmt.log` `fmt passed`；`task lint` `/tmp/agg_lint.log` `lint passed`；`task bdd-real-sqlite` 19 features / 80 scenarios (80 passed) / 452 steps。PG / MySQL 真库跑在 143 收口后的统一复跑里覆盖。

**那 18 个 skipped 里有 5 个是本 Stage 的假绿（2026-10-29 收口时发现并修复）**：`mcp_aggregate.feature` 的 `tools/list` 场景，step expr 写成 `... 发送 tools/list`，而 cucumber expression 里 `/` 是 **alternation 分隔符**，必须先转义成 `\/`（`cucumber-expressions-0.3.0/src/parse.rs:43` 的 `RESERVED_CHARS` 含 `/`）。于是 `#[when(expr = ...)]` 编译期虽不报错、运行时永远匹配不上，5 个场景（合并目录、同名工具共存、`user_required` 剔除、补 token 后可见、ACL 列表照给）从 Stage 144 起一直 `? Step skipped`。cucumber-rs 的 `StepSkipped` **不计为失败**（`runner/basic.rs:1345`），所以 `task bdd` 一路绿灯。改 `tools\/list` 两处（`mcp_aggregate_steps.rs:372`/`:378`）后首次真实执行，`/tmp/p55-bdd-fix.log:1019-1116` 全绿 —— 断言本身成立，但「本 Stage 的聚合目录断言已被 BDD 覆盖」这句话在此前是**没有证据**的。同类问题在 `body_archive_*` 还有 13 处未修，见 TD-028。

**Gate 4 批次结论（同日，与 143/145 同一份独立复查）**：本 Stage 无 HIGH；MED#4（`params.name` / `params.tool` 双读可绕过 ACL）落在本 Stage 的聚合改写路径上 —— `call_aggregate_tool` 原来只改写 `params.name`，`params.tool` 原样留给上游（`mcp_gateway.rs:970-974` 现同时 `remove("tool")`），并在 `post_mcp_aggregate` 的 method 分发与 ACL 之前加 `check_tool_spellings_consistency` → 400 + `-32020`。登记不修的 MED#6/MED#7/LOW#8/LOW#9/LOW#12 见 TD-026。

- **ACL 只覆盖聚合端点（已由 ADR-045 补齐，不在本 Stage 范围）** —— 本 Stage 把 `is_allowed` 接在 `call_aggregate_tool`（`mcp_gateway.rs:883`），但 `/mcp/{alias}` 与 `/mcp-rest/tools/call` 是 Stage 143 的路径、同样不读 allowlist，等于 ACL 只在客户端走 `/mcp` 时生效。Stage 143 的只读复查发现后，主 Agent 定为 ADR-045：三路径统一强制，`check_tool_allowed` 抽成共用函数，`tools/list` 仍不过滤。见 `docs/08-autonomous-decisions.md` ADR-045。
- **聚合目录的分页游标是下标偏移** —— 两页之间 server 集合变动会让后续下标错位、客户端静默跳/重。已登记 TD-024（P2），不改 key-based 游标的理由在同一处。
- **`merge` 的 `has_conflict` 不上线** —— `ToolEntry::to_wire()` 不发该字段，`conflicted()` 无生产消费者；它是「调用方 bug 的防御网」而非线上契约，`/mcp` 的 `tools/list` 不带冲突信息按现状接受（TD-024 同段记录）。
- **real 侧 steps 的状态预置** —— 非 real 分支的 `when_read_allowlist` / `when_query_calls` 起初是裸 `return;`，会让上一 step 的 mock body 留下来，在 `/tmp/stage141-test-1793220911.log` 里以 `real 调用日志 total_count 为 1`（`left: None`）现形。现已按 `real_api_steps.rs:346` 的约定预置状态；规则内容不同（`key/false` vs `team/true`）的两个场景靠 `mcp-agg-real:last-rule` 记住写的是哪条。

- **聚合的延迟是 N 倍** —— 冷缓存下要串行连 N 个上游。缓解：并发拉取（`futures::future::join_all`），单上游超时 10s；有工具拉取失败时**用其它上游的结果继续**（部分成功优于全失败），并在响应里带一个告警字段。这与 websearch 的「基础设施不可用时降级放行」同一思路。
- **ACL 的口径「行不存在 = 不限制」** 意味着误删一行会放行。这是刻意的取舍（见 §3.3），但需要在 UI 上让人看得见「当前只有 3 条规则」。
- **`/v1/mcp/*` 是新的鉴权路径** —— 与 `/admin/*` 不同，它必须允许普通用户调用。若混用 `require_admin` 会直接锁死。本 Stage 的门禁要专门测「普通用户能调、只有能改自己的」。
- **master key 无 `user_id`** 时用户凭据端点返回 400。这是个产品决策而非技术限制——若将来需要「服务账号级别的个人 token」，要另加一层概念。

---

## 9. 对后续 Stage 的影响

- **145** 消费本 Stage 的 `/v1/mcp/*` 与 `/admin/mcp/calls`，并回头补 142 的 `CallLogDialog` 与 ACL 开关。
- Phase 56（OAuth 发现）会在 143 / 144 的 401 响应上追加 `WWW-Authenticate: Bearer resource_metadata=...`。本 Stage 的 401 / 403 响应构造应留出加头的空间（不要在 handler 里硬编码完整响应，抽一个 `auth_challenge()` 函数）。
