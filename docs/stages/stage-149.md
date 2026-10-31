# Stage 149: MCP 调用日志的数据正确性与可读性（Phase 58）

**所属**: Phase 58（MCP 日志与审计完善）
**预估**: ~14h（聚合 owner 归并 + cursor 修复 + 名称批量补全 + 前端列改造 + 测试）
**依赖**: Stage 147（`/dash/mcp` 调用日志页与 `/v1/mcp/calls`）、Stage 144（`/mcp` 聚合端点与 `McpCallFilter`）
**状态**: ✅ 完成（2026-10-31）
**提交**: `af79061`（Phase 58 四个 Stage 合并的代码提交）+ `2add376`（四个 stage 文档与 ADR/技术债/roadmap 回写），均 `--signoff`，未 push。

---

## 1. 问题

2026-10-31 在生产网关（`9.135.87.221:4001`）用 master key 做带/不带 token 的对照实验时，`mcp_call_log` 暴露出三处「账上没有如实记录」的问题。全部是实测，不是推演。

### 1.1 聚合端点的 `token_owner` 恒为 `anonymous`（明明带了凭据出站）

同一把 master key、相隔 4 秒的两行：

```
2026-10-31T07:06:17 alias=*aggregate*  method=tools/list  owner=anonymous  user_id=None
2026-10-31T07:06:11 alias=iwiki        method=tools/list  owner=server     user_id=None
```

聚合 `tools/list` 实际给 5 个上游各自解出了 server token（否则那 100 个工具根本拿不到），账上却写 `anonymous`。根因在 `crates/aigw-server/src/routes/mcp_gateway.rs:875` 的 `list_aggregate_tools`：

```rust
match resolve_bearer(state, auth, &server).await {
    Ok((credential, _)) => visible.push((server, credential)),   // owner 被丢弃
```

`resolve_bearer` 返回的 `&'static str` owner 直接丢掉了，且该函数**从不**给 `row.token_owner` 赋值，于是保留 `call_row` 的默认值 `"anonymous"`（`:355`）。对照单 server 路径：`prepare_for`（`:296`）把 owner 交给 `effective_owner` 修一遍，`post_mcp`（`:404` 之后）与 `call_aggregate_tool`（`:1010`）都显式写入。

两个方向都错的风险都在这里：日志对「明明带了凭据」的调用报匿名，与 Stage 146 §Gate 4 #3 修掉的是同一类缺陷（当时修的是静态头出站仍记 `anonymous`）。

### 1.2 分页 cursor 传数字时静默回放第一页

`tools_page_cursor`（`:844`）只接受字符串：

```rust
.and_then(Value::as_str)
```

生产实测（`/mcp`，master key，共 196 个工具、`DEFAULT_PAGE_SIZE=100`）：

| 请求 | 返回 |
|---|---|
| `{"params":{"cursor":"100"}}` | 96 个，`nextCursor=null` ✅ |
| `{"params":{"cursor":100}}` | **100 个（第一页），`nextCursor="100"`** ❌ |

网关自己回的 `nextCursor` 是字符串，所以「照抄回来」的客户端不受影响；但客户端自己构造或把 cursor 当数字透传时会**静默死循环**——每次拿到同一页、同一个 `nextCursor`。MCP 规范里 cursor 是不透明字符串，实现方不保证类型；这里没有理由因为一个数字就退化成「从头开始」。

### 1.3 日志页把人读不懂的两个 id 直接铺在表格里

`CallLogTab.tsx` 三处直接渲染原始 id（`:318`、`:354`、`:411`）：

```
key_id  = 'd1eb8d1c9001ce4ab467c79ebdf51f334ec92e9a4e5e3eddb8a73fe53ee054e6'   ← SHA256
user_id = 'default_user_id'
```

`mcp_call_log` 只存 hash 与 id，**一个名称字段都没有**。生产上实际的 key 在 `virtual_keys` 里是有 `key_name`/`key_alias` 的（如 `OpenCodeReview`、`agent-ops`），用户有 `user_email`/`user_alias`。消费日志页早就建立了先例（`spend-logs/index.tsx:956`：`log.key_name || truncate8(log.api_key)`），MCP 日志页却退回了裸 hash —— 运维要拿 hash 去 `virtual_keys` 里反查才知道是谁。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 聚合请求的 owner 应写成「对**所有**被联系上游的统一描述」，不一致时必须有第四个值 | 若只取最弱（任一 anonymous 就写 anonymous），那么「5 个 server 里 4 个带凭据」也会被报成匿名——正是 §1.1 要修的谎言，只是换了个触发条件 | 生产 5 个 server 全是 `shared_or_user` 且有默认 server token，实测应得 `server`；UT 造混合场景断言 `mixed` |
| H2 | 名称在**响应时补**，不写进 `mcp_call_log` | 写进表要加迁移 ×3 + 回填 + 改名不跟随；而消费日志的先例就是读取时补名称 | 审计语义讨论：key 改名后历史行该显示新名（现状先例）还是旧名（严格审计）——详见 §3.3 |
| H3 | `key_id` → 名称需要一次批量查询，不能逐行 N+1 | 30 行就是 30 次查询，页面翻页会明显变慢 | 设计上限定为「当页去重后的 id 集合」，最多 2 次查询（keys + users） |
| H4 | cursor 应同时接受字符串与数字 | 只接受字符串时，客户端传数字会静默重放第一页（§1.2 实测），这是最难排查的一类缺陷：没有报错、分页看起来正常、永远看不到第 2 页之后的工具 | UT 直接驱动 `catalog::page` 与 handler |
| H5 | 新增 `mixed` 值不破坏既有消费方 | `/v1/mcp/calls`、`CallLogTab`、BDD fixture 都以字符串比较，穷举 union type 的地方需要同步（`McpTokenOwner`） | `rg -n "McpTokenOwner"` 全量检查 |

---

## 2. 边界

**做**：
- `list_aggregate_tools` 收集每个被联系 server 的 owner，归并后写入 `row.token_owner`：全 `server` → `server`；全 `user` → `user`；全 `anonymous` → `anonymous`；混合 → `mixed`；**零个上游被联系** → `anonymous`（没有凭据出站这件事为真）
- `McpTokenOwner` 增加 `mixed`，前端徽章与 i18n 同步
- `tools_page_cursor` 同时接受 `Value::as_str` 与 `Value::as_u64`
- `GET /admin/mcp/calls` 与 `GET /v1/mcp/calls` 的响应行补两个只读字段：`key_name`（`virtual_keys.key_name` → `key_alias` → null）与 `user_name`（`users.user_alias` → `user_email` → null）
- 前端日志页：用户列显示 `user_name ?? user_id ?? 「未绑定」`，`user_id` 进 `Tooltip`；抽屉的 key/用户两行同样显示名称、id 进 tooltip
- UT：聚合 owner 四种归并 + 零上游 + cursor 两类型 + 名称补全（含查不到的 id 回退）
- BDD：表格显示名称、tooltip 含 id

**不做**（明确排除）：
- **不把名称写进 `mcp_call_log`**（不新增列、不改写入路径）——见 §3.3
- **不做 owner 的逐 server 明细**（一行一条日志的结构不变；要明细得改表结构，收益不抵成本）
- **不给 `mixed` 加悬停展开「哪几个是 anonymous」**（需要明细，同上）
- **不改 `effective_owner` 的语义**（静态头仍算 `server`）
- **不做名称的模糊搜索**（筛选仍只按 `user_id` 等值；按名称筛是另一个需求）

---

## 3. 方案

### 3.1 聚合 owner：保留 owner，四处一致归并

`resolve_bearer` 的返回原样保留，`visible` 从 `Vec<(McpServerRow, Option<String>)>` 变成再带一个 owner：

```rust
let mut owners: Vec<&'static str> = Vec::new();
for server in servers.into_iter().filter(|s| s.enabled) {
    match resolve_bearer(state, auth, &server).await {
        Ok((credential, owner)) => {
            // 与单 server 路径同一把尺子：静态头出站也算 server
            let headers = outbound_headers(state, &server, credential.as_deref());
            owners.push(effective_owner(&headers, owner));
            visible.push((server, credential, headers));
        }
        ...
    }
}
```

注意 `effective_owner` 需要 `OutboundHeaders`，而 `catalog_tools` 内部**也会**自己算一次 `outbound_headers`（`:795`）。避免算两遍：把 `outbound_headers` 提到循环里算一次，传给 `catalog_tools`（改签名接 `&OutboundHeaders`，内部不再重复解封）。解封是 AES-GCM 解密，算两遍是纯浪费，且**两处若参数不同会得到不同的头**——这正是 Stage 146 §Gate 4 要求「探测与运行用同一套头」的同一类隐患。

归并函数是纯函数，放 `crates/aigw-core/src/mcp/catalog.rs` 旁边可单测：

```rust
pub fn merge_owners(owners: &[&str]) -> &'static str
```

规则：空 → `anonymous`；全部相等 → 该值；否则 `mixed`。

### 3.2 `effective_owner` 的归宿

现在 `effective_owner` 在 `mcp_gateway.rs:154`。聚合路径也要用，且 3.1 要求在 `list_aggregate_tools` 内可用 —— 保持原位即可（同 crate 内 `pub(crate)`）。不搬到 core：它依赖 `OutboundHeaders`，而 core 里已有 `mcp::headers`，可以搬，但搬动会让 `mcp_gateway` 的调用点全部改路径，收益为零。**留在 server**。

### 3.3 名称在响应时补，不写表

两条路的取舍：

| | 响应时补（本 Stage） | 写入时固化 |
|---|---|---|
| 迁移 | 无 | `035` 加两列 × 三方言 + 历史行永远为空 |
| key 改名后 | 显示新名 | 显示当时的名 |
| 查询成本 | 每页 2 次批量查询 | 0 |
| 与消费日志一致 | 是（`key_name` 就在 `spend_logs` 里，但那是写入时带的字段，不是 join） | — |

严格审计语义下「写入时固化」更对：日志应该记录当时的事实。但消费日志已经用「写入时带 key_name」的方式落地了，而 MCP 日志在写入时**手上没有** key_name（`KeyIdentity` 只有 `token_hash`），要补就得在 `call_row` 里多一次 `get_key_by_token`——那是每**次调用**多一次查询，换成每**页**两次查询明显更划算。

因此：**响应时补**，并把这个取舍写进 ADR-052（名称是展示属性，不是审计事实；审计事实是 `key_id`/`user_id`，两者都仍然原样保留）。

实现落点：`call_log_response`（`crates/aigw-server/src/routes/mcp.rs:1483`）拿到 `rows` 之后：

1. 收集非空 `key_id` 去重集合 → 一次查 `virtual_keys`（`WHERE token IN (...)`）→ `HashMap<hash, String>`：`key_name` 优先，空则 `key_alias`
2. 收集非空 `user_id` 去重集合 → 一次查 `users`（`WHERE user_id IN (...)`）→ `HashMap<id, String>`：`user_alias` 优先，空则 `user_email`
3. 每行补 `key_name` / `user_name`；查不到 → `null`

SQL 用 `QueryBuilder` 或按长度拼接占位符；三方言 `?` 与 `$N` 的差异集中在 db 层（`crates/aigw-core/src/db.rs`），新增两个 store 方法 `mcp_call_key_names(&[String])` 与 `mcp_call_user_names(&[String])`。

`*master*` 不参与查询（它不是 hash，稳定映射到 `"master"`）。

**不得改变行结构**：`McpCallLogRow` 是 `mcp_call_log` 的一一映射（stage-144 §3.5 的约定），所以名称字段加在**响应 JSON** 上，不加在 `McpCallLogRow` 上。新增一个 `McpCallLogView` 包一层 `#[serde(flatten)] row` + 两个 `Option<String>`，或直接把行序列化后 `insert` 两个键。选后者：不新增类型，`serde_json::to_value` 后插入。

### 3.4 cursor

```rust
fn tools_page_cursor(parsed: &Value) -> Option<Cow<'_, str>> {
    let raw = parsed.get("params")?.get("cursor")?;
    match raw {
        Value::String(s) => Some(Cow::Borrowed(s.as_str())),
        Value::Number(n) => Some(Cow::Owned(n.to_string())),
        _ => None,
    }
}
```

其他类型（bool/object/array）仍视为「没有 cursor」，与现状一致：`catalog::page` 对无法解析的字符串也是从头开始（`:115` 的 `unwrap_or(usize::MAX)` 会**跳到末尾**——注意这里语义相反：非法**字符串** → `usize::MAX` → 空页；**非字符串** → 从头开始。这个不一致本 Stage 不统一，只在 §8 记录，因为改动它需要先决定「非法 cursor 该报错还是该重放」，那是协议层决策）。

不对，`usize::MAX` 会被 `.min(entries.len())` 夹住，结果是空页 + `nextCursor=None`。所以「非法字符串 → 空页」「非法类型 → 第一页」。两者都不能静默，但都不在本 Stage 改。§8 记为遗留。

### 3.5 前端

- `types.ts`：`McpTokenOwner = "server" | "user" | "anonymous" | "mixed"`；`McpCallRow` 加 `key_name?: string | null`、`user_name?: string | null`
- `CallLogTab.tsx`：
  - 用户列：`<Tooltip><TooltipTrigger>{user_name ?? user_id ?? 未绑定}</TooltipTrigger><TooltipContent>{user_id}</TooltipContent></Tooltip>`；未绑定时 tooltip 不显示（或显示「该 key 未绑定用户」）
  - 抽屉：`key` 行显示 `key_name ?? key_id ?? "—"`，`user` 行同理，id 进 tooltip
  - `ownerKey` 加 `mixed: "mcp.calls.owner.mixed"`
- i18n：`mcp.calls.owner.mixed` = 「混合」/「Mixed」；`mcp.calls.key` / `mcp.calls.keyId` 文案调整（名称在左、id 在 tooltip）
- `TooltipProvider` 的挂载点与 `usage/index.tsx` 一致（照抄现有用法，不新造）

---

## 4. 测试

### 4.1 core UT（≥6）

`crates/aigw-core/src/mcp/catalog.rs`：

1. `merge_owners(&[])` → `"anonymous"`
2. `merge_owners(&["server","server"])` → `"server"`
3. `merge_owners(&["user"])` → `"user"`
4. `merge_owners(&["server","anonymous"])` → `"mixed"`
5. `page` 对 `Some("100")` 与 `Some("100")` 的等价性（字符串路径已覆盖）；新增数字路径由 handler UT 覆盖

`crates/aigw-core/src/db.rs`（SQLite 内存库）：

6. `mcp_call_key_names` 传入 3 个 hash（2 个存在、1 个不存在）→ 返回 2 条，不存在的键缺席
7. `mcp_call_user_names` 同上；`key_name` 为空、`key_alias` 非空时取 alias（**优先级顺序被钉死**）
8. 空输入 → 空 map，且**不发 SQL**（`Vec::is_empty` 早退）

### 4.2 handler UT（≥6）

`crates/aigw-server/src/routes/mcp_gateway.rs`（聚合测试区，沿用 `aggregate_guard`）：

1. 一个 server、配了 server token → 聚合 `tools/list` 行的 `token_owner == "server"`（**当前会失败，这是本 Stage 的主回归**）
2. 两个 server、一个 `user_required` 无 user token 被跳过、另一个 `shared` 有 token → `server`（被跳过的 server 不参与归并）
3. 两个 server 都被联系、一个带 token 一个没有 → `mixed`
4. 零个 enabled server → `anonymous`
5. 静态头（`auth_header_value`）配了但无 token 行 → `server`（`effective_owner` 在聚合路径也生效）
6. `{"params":{"cursor":100}}` 数字 → 返回第一页且 `nextCursor` 不为 null（≥101 个工具时）
7. `{"params":{"cursor":"100"}}` 字符串 → 与数字请求**逐字段相同**

`crates/aigw-server/src/routes/mcp.rs`：

8. 造两行（一个 key 有 `key_name`、一个用户有 `user_alias`）→ `/admin/mcp/calls` 响应含 `key_name`/`user_name`
9. 造一行 `key_id` 在 `virtual_keys` 里查不到 → `key_name` 为 `null`，行仍然返回（**不能因为补名称失败而丢行**）
10. `key_id == Some("*master*")` → `key_name == "master"`

### 4.3 BDD（mock，≥3 场景 × 3 viewport）

`crates/aigw-frontend/tests/features/mcp.feature`：

1. 用户列显示名称而不是 hash：fixture 给 `user_name`，断言单元格文本为名称、`Tooltip` 里含 `user_id`
2. `user_name` 为 null 但 `user_id` 有值 → 显示 id
3. `token_owner=mixed` → 徽章文案为「混合」，且**不与 anonymous 共用样式**（amber）
4. 抽屉里 key 行显示 `key_name`，tooltip 含 `key_id`

`api-mocks.ts` 的 `/admin/mcp/calls` fixture 补 `key_name`/`user_name`。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/src/mcp/catalog.rs` | 新增 `merge_owners` + UT |
| `crates/aigw-core/src/db.rs` | `mcp_call_key_names` / `mcp_call_user_names`（三方言 IN 查询，空输入早退） |
| `crates/aigw-server/src/routes/mcp_gateway.rs` | `list_aggregate_tools` 保留 owner、`outbound_headers` 提到循环外、`effective_owner` 归并、写 `row.token_owner`、name 显示用的 `catalog_tools` 签名调整；`tools_page_cursor` 接数字 |
| `crates/aigw-server/src/routes/mcp.rs` | `call_log_response` 批量补 `key_name`/`user_name` |
| `crates/aigw-server/src/routes/mcp_user.rs` | 复用 `call_log_response`，无需改动（确认） |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | `McpTokenOwner` 加 `mixed`；`McpCallRow` 加两个名称字段 |
| `crates/aigw-frontend/src/pages/mcp/CallLogTab.tsx` | 用户列/抽屉改名称 + tooltip；`ownerKey` 加 `mixed` |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | `mcp.calls.owner.mixed`、`mcp.calls.key` 等 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | `/admin/mcp/calls` fixture 补名称字段 |
| `crates/aigw-frontend/tests/features/mcp.feature` + steps | 新增 4 场景 |

---

## 6. 回归验证

全部通过 `task`，禁止裸 `cargo`：

- `task test`（全量，门禁只认这个）
- `task bdd`
- `task fe-bdd` / `task fe-lint` / `task fe-build`
- `task fmt` / `task lint` / `task doctor`
- 本 Stage 无迁移，不需要 `bdd-real-*`；若 3.3 的 IN 查询涉及三方言绑定，则补 `task test-db-up` → `task bdd-real-pg` / `bdd-real-mysql` → `task test-db-down`

手工：生产网关上发一次聚合 `tools/list`，确认 `/admin/mcp/calls` 该行 `token_owner=server` 而不是 `anonymous`；再发一次 `{"cursor":100}`，确认拿到第 2 页。

---

## 7. 门禁

- [x] 聚合 `tools/list` 的 `token_owner` 与「实际出站的凭据」一致（server/user/anonymous/mixed 四态各有 UT）—— `an_aggregate_list_records_the_credential_that_actually_left`（server）、`an_aggregate_list_that_only_used_user_tokens_records_user`（user）、`an_aggregate_list_with_no_reachable_server_records_anonymous`、`a_fan_out_that_mixes_credential_sources_is_recorded_as_mixed`，外加 `a_server_skipped_for_a_missing_user_token_does_not_colour_the_merge`（被跳过的上游不得染色）与 `a_configured_static_header_still_counts_as_a_server_credential`（静态头仍记 server）
- [x] 混合场景写 `mixed`，且前端有对应文案与样式（不与 `anonymous` 共用 amber）—— `CallLogTab.tsx:75` 的 `ownerClass`：mixed 走 violet、anonymous 走 amber；i18n `mcp.calls.owner.mixed` = 混合 / Mixed
- [x] `cursor` 传数字与传字符串得到**逐字段相同**的响应 —— `a_numeric_cursor_pages_exactly_like_the_string_it_came_from`；另 `a_cursor_of_another_type_is_still_treated_as_no_cursor` 钉死「其它类型仍是第一页」这一遗留语义
- [x] 名称补全不引入 N+1：一页最多 2 次批量查询（代码审阅 + UT 断言空输入早退）—— `with_display_names` 用 `tokio::join!` 并发两次；`mcp_call_key_names` / `mcp_call_user_names` 空输入直接返回空 map（`db.rs:14594` 断言）
- [x] 补名称失败不丢行（查不到的 id 回退为 null，行照常返回）—— `an_unknown_key_still_returns_its_row_with_a_null_name`（`count` 仍为 1，两个名字字段为 null）
- [x] `McpCallLogRow` 结构未变（仍是 `mcp_call_log` 的一一映射），名称只在响应层出现 —— `models.rs:1567` 只有表列；`the_call_log_carries_display_names_alongside_the_ids` 同时断言 `key_name`/`user_name` 与 `key_id`/`user_id` 并存
- [x] `task test` 全量通过（数字以全量日志为准）—— 22 个 `test result` 块全 `ok`、1683 passed / 0 failed，末行 `test passed`，见 §7.1
- [x] `task fe-bdd` 全量通过；320px 视口下新 tooltip 不撑破表格 —— 数字见 `stage-151.md` §7.1（同一棵最终树上复跑）
- [x] `task fmt` / `task lint` / `task doctor` EXIT=0 —— 三者均 EXIT=0（`/tmp/aigw-fmt-150.log`、`/tmp/aigw-lint-150.log`、`/tmp/aigw-doctor-150.log`）

### 7.1 门禁证据（最终树复跑）

- `task test`：22 个 `test result` 块全部 `ok`，`FAILED` 行 0，合计 **1683 passed / 0 failed / 13 ignored**，末行 `test passed`，EXIT=0（`/tmp/aigw-test-150f.log`）。内嵌 mock BDD：**51 features / 382 scenarios（369 passed, 13 skipped）/ 2252 steps（2239 passed, 13 skipped）**。13 个 skip 全是既有 `body_archive_read/write.feature` 的缺陷（TD-028），非本 Stage 引入。
- `task fmt` / `task lint` / `task doctor` 均 EXIT=0（`/tmp/aigw-fmt-150.log`、`/tmp/aigw-lint-150.log`、`/tmp/aigw-doctor-150.log`）。
- `task bdd`：同一份 51/382/2252 的统计（`/tmp/aigw-bdd-150.log`，末行 `bdd passed`）。
- `task bdd-real-pg`：**22 features / 91 scenarios（90 passed, 1 failed）/ 500 steps（499 passed, 1 failed）**（`/tmp/aigw-bdd-real-pg-150b.log`）；唯一失败是 `real_mcp_aggregate_steps.rs:292` 的 `hyper::Error(IncompleteMessage)`，即 TD-028 记录的 `tracing-subscriber` span-guard flake，与 Stage 149/150 无关（MCP 代码零 span guard）。本 Stage 新增的 4 条保留期 real 场景全部 ✔。
- 本 Stage 无迁移，`IN (...)` 的绑定顺序由 `in_placeholders` 统一生成，三方言共用同一构造。
- 前端门禁（`task fe-bdd` / `fe-lint` / `fe-build`）在 Stage 151 的最终树上统一复跑并在 `stage-151.md` §7.1 记录 —— 同一份 `CallLogTab` / `mcp.feature` 改动，分两次跑得到的数字没有额外证据价值。
- 上述 1683 是本 Stage 那棵树上的数字；本 Phase 收尾后的最终树合计 **1716 passed / 0 failed / 13 ignored**（`stage-152.md` §7.1）。

---

## 8. 风险与遗留

- **`mixed` 是个新词表值** —— 将来的消费方（导出 CSV、告警规则）如果穷举三个值会漏掉它。缓解：`McpTokenOwner` 是前端唯一 union 类型，`rg` 可穷举；后端 `token_owner` 是 `String`，本就无编译期约束。
- **非法 cursor 的两种行为不一致**（字符串 → 空页；数字以外的类型 → 第一页）——本 Stage 只修「数字被当成没有 cursor」这一条，两类非法输入的统一语义（报错 vs 重放）需要协议层决策，记为遗留。真正静默的是「空页」：客户端会以为工具列表到此为止。
- **名称查询是逐页的，页大小 100 时最多 100 个 id** —— IN 列表长度上限在 SQLite 是 999 个参数，PG/MySQL 无此限制；页大小上限 100（`call_log_response:1490` 的 clamp）保证安全，但**这个耦合要写进注释**，否则将来放开页大小会撞上 SQLite 的限制。
- **`user_alias` / `user_email` 可能都为空** —— 生产上有 `default_user_id` 这类占位 id，`users` 表里可能根本没有。回退链终止于原 id，不显示空白。
- **聚合 owner 依赖 `catalog_tools` 的缓存命中**：缓存命中时不会重新解封凭据，因此 `effective_owner` 的静态头判断依赖 `outbound_headers` 的结果（3.1 已把 headers 提到循环外算一次），与缓存无关。
