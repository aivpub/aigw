# Stage 148: Server 管理页的探测与「全部重新探测」进度指示（Phase 56）

**所属**: Phase 56（MCP Gateway 生产反馈迭代）
**预估**: ~10h（后端 run registry + 轮询端点 + 并发探测 + 前端进度对话框 + BDD）
**依赖**: Stage 141（probe / reload 端点与 `probe_and_store`）、Stage 142/145（`/dash/mcp` Server 管理 tab）、Stage 146（会改 `probe_server` 的签名，本 Stage 在其之后）
**状态**: ✅ 完成（2026-10-31）
**提交**: `9a7e125`（与 Stage 147 合并提交，见该 commit message 的归属说明），`--signoff`，未 push。

---

## 1. 问题

「全部重新探测」现在是一个**黑盒按钮**。`reload_mcp`（`crates/aigw-server/src/routes/mcp.rs:626`）串行跑完所有 `enabled` server 才一次性返回，单次探测上限 `PROBE_TIMEOUT = 10s`（`crates/aigw-core/src/mcp/probe.rs:29`），N 个 server 最坏 `N × 10s`。期间前端只有一个按钮 spinner：**看不到是哪个 server 慢、哪个挂了、还剩几个**。生产上 5~10 个 server、其中一个是慢上游时，这个按钮会静默卡住半分钟以上。

单个探测（`mcp.rs:582`）也有同类问题但轻得多：一行按钮连点会向上游打出多次完整探测，且行内没有任何「正在探测」的反馈。

本 Stage 只解决**可观测性与重复请求**，不改探测语义（不删快照、跳过 `disabled`、冲突集合一次性算出——这些由 Stage 141 的 Gate 4 整改钉死）。

---

## 1.5 核心假设

| # | 假设 | 若不成立的后果 | 验证方式 |
|---|------|--------------|---------|
| H1 | 逐个 server 的结果比一个百分比更有用 | 只给 `3/7` 的话，用户还得再猜「哪个挂了」；而失败原因（上游 502 / 超时 / 协议不认）只能逐行看 | 进度对话框的设计就是「进度条 + 逐行明细」，门禁要求失败行单独标红 |
| H2 | 500ms 轮询足够，不需要流式 | 若需要流式，要另写长连端点 + 断线重连 + BDD 里读流，成本远高于收益 | 状态粒度是「几十条记录、每个几秒变一次」，人眼无法分辨 500ms 内的差异 |
| H3 | 进程内状态够用（单实例部署） | 多实例下每个实例只知道自己那次 run；进度查询打到别的实例会 404 | 与 ADR-041 的 `catalog_cache` 同一放置前提；Phase 55 明确不引 Redis |
| H4 | 同时只允许一个 reload 是正确约束 | 两次并发 reload 会对同一批上游重复探测、互相覆盖 catalog 快照（`probe_and_store` 是 read-modify-write），并让两份进度互相矛盾 | `409 reload_in_progress` + 返回在跑那次的 `run_id` |
| H5 | 单 server 探测不需要 run 机制 | 引入状态机后「点一下看结果」要多一次轮询 + 一个对话框生命周期，且它的耗时上限（≤10s）本来就是可接受的 | 端点保持同步，前端只加行内 spinner + 行级 disable |
| H6 | 重启丢进度是可接受的 | 服务端重启时探测任务本身也终止了，不存在「以为在跑实际已死」的误导 | 前端在 `latest()` 缺失时显示「没有进行中的探测」，而不是空进度条 |

---

## 2. 边界

**做**：
- 进程内 `ProbeRun` 注册表（只保留最近一次 run，含每个 server 的 pending/running/ok/failed、工具数、协议版本、错误）
- `POST /admin/mcp/reload` 改为启动即返回 `202 {run_id, total}`；并发请求 `409`
- `GET /admin/mcp/reload/{run_id}` 与 `GET /admin/mcp/reload/latest`
- 探测并发化（上限 4），单 server 失败不影响其它
- 前端 `components/ui/progress.tsx` + `ProbeProgressDialog.tsx`；`handleReloadAll` 改为「启动 → 轮询 → 汇总」
- 单 server 探测：行级 `isProbing` + 按钮 disable + 行内 spinner
- 前端 BDD ≥5 场景 × 3 viewport；后端 UT ≥9

**不做**（明确排除）：
- **不做持久化 run 历史** —— 需要新表 + 保留策略，而它的唯一用途是事后翻旧账，收益不明
- **不做多实例下的共享进度** —— 那要先有 Redis（Phase 55 明确不引）
- **不做取消正在跑的探测** —— 探测到一半取消会让「已替换快照 / 未替换快照」混在一起，收益低、状态复杂度高
- **不做探测失败的自动重试**
- **不做定时自动 re-probe（cron）**
- **不改 `POST /admin/mcp/servers/{id}/probe` 的响应形状**（`ProbeDialog` 依赖它）

---

## 3. 方案

### 3.1 取舍：轮询，不复用 `sse_response`

`mcp_relay.rs` 已有现成的 SSE 基建（`relay_event_stream:135` 的 `tokio::spawn` + `sse_response:195`，含 `X-Accel-Buffering: no`），复用看起来很省事。**本 Stage 不复用它**，理由如下：

- 逐个 server 的结果是**粗粒度、量级几十条**的状态，不是 token 流；SSE 的增量语义在这里没有价值。
- SSE 要多一条长连端点、一套断线重连、以及一个「在 BDD 里读流」的测试替身；轮询只要 `GET` + `useQuery({ refetchInterval })`，`api-mocks.ts` 里加一个按 tick 推进的路由即可。
- 刷新页面后 SSE 连接丢失，必须另配一个 `latest()` 快照端点才能恢复；轮询本来就每 tick 全量读快照，恢复是免费的。

**代价**：每次状态变化最多晚一个轮询周期可见。取 **500ms**：一个 run 的典型时长是数秒到数十秒，500ms 的延迟肉眼不可辨；同时 500ms × 客户端数 的请求量对 admin 端点是噪声级。

### 3.2 `probe_runs.rs`：纯状态机

新增 `crates/aigw-core/src/mcp/probe_runs.rs`，无 IO、无锁、无 `async`，全部可单测：

```rust
pub struct ProbeRun { pub run_id: String, pub total: usize, pub done: usize,
                      pub items: Vec<ProbeRunItem>, pub status: RunStatus }
pub struct ProbeRunItem { pub server_id: i64, pub alias: String,
                          pub state: ItemState, pub tool_count: Option<usize>,
                          pub protocol_version: Option<String>, pub error: Option<String> }

pub enum ItemState { Pending, Running, Ok, Failed }
pub enum RunStatus { Running, Ok, Partial }   // Partial = 至少一条 Failed
pub mod registry;                              // start / item_running / item_done / latest / reset
```

- **`done` 只在终态（`Ok` / `Failed`）递增**，`Running → Pending` 之类回退不存在。`start()` 置全 `Pending` 且 `done = 0`。
- `status` 不单独存可变字段，由 items 在 `done == total` 时算出（`Failed == 0` → `Ok`，否则 `Partial`）。避免「汇总与明细不一致」这个经典 bug。
- **`errors` 只有一个字符串字段**：`error = "{upstream_status} {message} · {body 截断到 500 字符}"`（与 `PROBE_BODY_LIMIT = 500` 对齐），不为它加第三个字段——UI 要的是「点开能看到上游说了什么」，不是结构化解析错误。
- 放置：`OnceLock<Mutex<Option<ProbeRun>>>`（照 ADR-041 的理由，**不挂 `AppState`** —— 39 处 `AppState` 字面量不值得为一个可推导的进度状态改一遍）。
- **`registry::start(run_id, items) -> Result<(), Existing>` 在同一次加锁里做「检查是否已有 Running 的 run + 写入新 run」**，不存在 is-running 与 insert 之间的 TOCTOU；`latest()` 返回 `Option<ProbeRun>` 的克隆。只保留最近一次，`start` 直接覆盖旧结果（旧 run 的 `run_id` 从此查不到 → 404，这是有意的：结果过期了就该消失）。
- 锁只用 `std::sync::Mutex`，**绝不跨 `.await` 持有**；每次探测完成时单独加锁更新一项并立即释放（`catalog_cache.rs:7` 的同一纪律）。
- `reset()` 供 UT 使用（每个用例前清空，与 `catalog_cache::clear()` 同一条 ADR-041 说明）。

### 3.3 端点

```
POST /admin/mcp/reload
  → 202 {"run_id":"<uuid v7>","total":3}
  → 409 {"error":{"type":"reload_in_progress","run_id":"<已有>"}}

GET /admin/mcp/reload/latest      → 200 ProbeRun | 404 {"error":{"type":"no_reload_run"}}
GET /admin/mcp/reload/{run_id}    → 200 ProbeRun | 404（run_id 不匹配最近一次）
```

`run_id` 用 `uuid::Uuid::now_v7().to_string()`（`uuid` 已在 core 与 server 的依赖里；`session.rs:111` / `mcp_gateway.rs:69` 是既有先例）。

`reload_mcp` 的返回类型要从 `Json<Value>` 改成 `(StatusCode, Json<Value>)`（`202` 与 `409` 都要非 200）。改造后它的流程：

1. `require_admin` → **先读一次 `registry::latest()`**，已有 `Running` 的 run 立刻 `409`（省掉后面 N+1 条查询；锁内 `start` 仍是权威判定）→ `list_mcp_servers` → 取 `enabled` 子集（**启动时快照**：中途停用的 server 仍在 items 里跑完，不中途改变 total）
2. 先算每个 server 的 `others`（现有两趟写法原样保留），构造全 `Pending` 的 items，`registry::start` —— 失败即 `409`，**不 spawn**
3. `tokio::spawn` 探测任务，`state` 克隆进任务（`SharedState` 是 `Arc`）
4. 立即返回 `202`

`GET /admin/mcp/reload/latest` 必须**先于** `/{run_id}` 注册（axum/matchit 静态段优先，但注册顺序写错时断言不到，用 BDD 场景钉死）。

### 3.4 并发探测

`for` 循环改成 `futures::stream::iter(enabled).map(|server| async move { ... }).buffer_unordered(4).for_each(...)`。选 `buffer_unordered(4)` 而不是 `join_all` + `tokio::sync::Semaphore`：前者就是「并发上限」的原生表达，少一个同步原语、少一处泄漏守卫。`futures` 已在 `aigw-server` 的依赖里（`Cargo.toml:35`），**不需要新增依赖**。

**为什么是 4**：上游是外部服务，几十路同时打过去在对方看来就是压测；4 路能把 N=10 的最坏 100s 压到约 30s（`ceil(10/4) × 10s`），已经落到「人可以等」的区间。失败隔离天然成立：每个 future 自己处理 `Err`，写一条 `Failed` item。

### 3.5 前端

- **`components/ui/progress.tsx`**：`<div role="progressbar" aria-valuenow aria-valuemin={0} aria-valuemax={total}>` 内一个按百分比设宽的 div。不引第三方库（`components/ui/` 目前只有 17 个手写组件）。
- **`ProbeProgressDialog.tsx`**：标题「重新探测全部」；顶部进度条 `done/total` + 计数；下方逐行 `alias | 状态 | 工具数 | 协议版本 | 错误摘要`；结束态显示汇总（`probed` / `failed`），失败行可展开看完整 `error`。**关闭对话框不取消任务**（任务在服务端跑），再次打开时用 `/reload/latest` 恢复。
- **`handleReloadAll`**：`POST` → 拿 `run_id` → 开对话框 → `useQuery({ queryFn: () => apiGet(\`/admin/mcp/reload/${run_id}\`), refetchInterval: 500, enabled: running })`。`409` 时**直接接上返回的 `run_id`** 并提示「已有一次探测在进行中」，不报错。`status !== "running"` 时停轮询、`queryClient.invalidateQueries({queryKey: ["mcp-servers"]})`（工具数会变）、toast 汇总。
- **单个探测**：新增 `probingId: number | null`，`handleProbe` 期间该行按钮 `disabled` + 行内替换成 `<Spinner>`；`finally` 里清空。`ProbeDialog` 与 toast 行为不变。
- 新 i18n key（`mcp.progress.*`，中英各一份）+ `resources.d.ts` 重生成；对话表格在 320px 下用 `overflow-x-auto` 而不是撑宽页面。

### 3.6 单 server 探测为什么不进 run 机制

`POST /admin/mcp/servers/{id}/probe` 保持同步。它的上限是单个 `PROBE_TIMEOUT`（10s），前端的诉求只是「别连点、告诉我正在跑」—— 两个 state 值就能表达。为它引入 `run_id` + 轮询会让「点一下看结果」多出一个生命周期，收益为负。**这条是刻意的边界，不是遗漏。**

---

## 4. 测试

### 4.1 UT（≥9）

`probe_runs` 纯状态机（无 DB、无网络）：

| # | 用例 | 期望 |
|---|---|---|
| 1 | `start` 后立刻读 | 全 `Pending`、`done == 0`、`status == Running` |
| 2 | `Pending → Running → Ok` | `done` 仅在 `Ok` 那次 +1 |
| 3 | `Pending → Running → Failed` | 同上；`error` 非空 |
| 4 | 全部 `Ok` | `status == Ok` |
| 5 | 一条 `Failed` 其余 `Ok` | `status == Partial` |
| 6 | `start` 覆盖旧 run | `latest()` 只返回新的 `run_id`，旧 `run_id` 查不到 |
| 7 | 已有 `Running` 时再 `start` | 返回 `Err(Existing{run_id})`，且**注册表未被改写** |
| 8 | `reset()` 后 `latest()` | `None` |
| 9 | 0 个 enabled server | `start` 成功、`total == 0`、`status == Ok`（不是 `Running`） |

> 每个用例前调 `registry::reset()` —— 进程级静态在测试间共享（ADR-041：`catalog_cache` 的同一处理）。

handler 层（`crates/aigw-server/src/routes/mcp.rs` 的 `#[cfg(test)]`，用自绑端口的 axum stub，见 4.2）：

| # | 用例 | 期望 |
|---|---|---|
| 1 | `POST /admin/mcp/reload` | `202`，body 含 `run_id`（可解析为 UUID）与 `total` |
| 2 | 一个 run 在跑时再 `POST` | `409`，`error.type == "reload_in_progress"` 且 `error.run_id` 等于第一个 |
| 3 | `GET /reload/{run_id}` | items 长度 == `total`，元素含 `alias` / `state` / `tool_count` |
| 4 | `GET /reload/latest` | 与上一条同一份快照 |
| 5 | 其中一个 stub 上游 500 | 该 item `Failed`，其它 `Ok`，整体 `Partial` |

### 4.2 BDD（前端，≥5 场景 × 3 viewport）

`mcp.feature` 追加：

| # | 场景 | 断言 |
|---|---|---|
| 1 | 点「全部重新探测」 | 进度对话框出现、`role="progressbar"` 可见，最终显示汇总 |
| 2 | 一个 server 失败 | 该行标红且含错误摘要，汇总为 `partial`，其它行仍为绿色 |
| 3 | `POST` 返回 `409` | 直接接上已有任务并显示「已有一次探测在进行中」，**没有**错误 toast |
| 4 | 探测进行中 | 该行单个探测按钮 `disabled` |
| 5 | 关闭再打开对话框 | 进度从 `/reload/latest` 恢复，不是从 0 开始 |

**中间态必须可观测**：`api-mocks.ts` 里的 mock reload run 按「每次轮询推进一个 item」走，并对每次响应加 `await new Promise(r => setTimeout(r, 350))`；否则 playwright 一帧内就能看到终态，进度条等于没测。服务端 UT/Bdd 的探测上游同理 —— `TcpListener::bind("127.0.0.1:0")` 的 axum stub 在 `server/discover` 之前 sleep 350ms（照 `crates/aigw-core/src/websearch/probe.rs:172` 的 `spawn_stub`）。

---

## 5. 变更清单

| 文件 | 动作 |
|---|---|
| `crates/aigw-core/src/mcp/probe_runs.rs` | 新增：`ProbeRun` / `ProbeRunItem` / `RunStatus` / `ItemState` + `registry`（`start` / `item_running` / `item_done` / `latest` / `reset`） |
| `crates/aigw-core/src/mcp/mod.rs` | 加 `pub mod probe_runs;` + 必要 re-export |
| `crates/aigw-server/src/routes/mcp.rs` | `reload_mcp` 改 202/409 + spawn；新增 `reload_status` / `reload_latest`；`probe_and_store` 的失败信息格式化为 item 的 `error` 串 |
| `crates/aigw-server/src/main.rs` | 注册 `GET /admin/mcp/reload/latest`（先）与 `GET /admin/mcp/reload/{run_id}` |
| `crates/aigw-server/Cargo.toml` | **不需要改**：`futures` 已依赖（`:35`），`uuid` 已依赖（`:25`） |
| `crates/aigw-frontend/src/components/ui/progress.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/ProbeProgressDialog.tsx` | 新增 |
| `crates/aigw-frontend/src/pages/mcp/types.ts` | 加 `ProbeRun` / `ProbeRunItem` 类型，删掉 `index.tsx` 里的 `ReloadResult`（`index.tsx:70`） |
| `crates/aigw-frontend/src/pages/mcp/index.tsx` | `handleReloadAll` 重写、`probingId` 状态、两处探测按钮（`:417` 表格 / `:533` 卡片）加 `disabled` + spinner |
| `crates/aigw-frontend/src/components/ui/tabs.tsx` | `TabsList` 透传 `...rest`（否则 `data-testid` 被吞，320px 断言取不到元素）；基类 `h-9` 不动 |
| `crates/aigw-frontend/src/i18n/index.ts` | `react: { bindI18nStore: "added" }` —— 懒加载的 zh-CN chunk 落地后已挂载组件要重渲染（见 §8 末条） |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 加 `mcp.progress.*` 与 `mcp.toast.reloadInProgress` |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 重生成 |
| `crates/aigw-frontend/tests/features/mcp.feature` + `tests/steps/mcp.steps.ts` | 追加 5 场景与步骤 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | `:1225` 的 reload 桩改为「启动 → 轮询推进」的假 registry（含 350ms 延迟、409 分支） |

---

## 6. 回归验证

- `task fmt` / `task lint` / `task doctor`
- `task test`（全量，门禁只认它）
- `task bdd`（后端场景不回归）
- `task fe-lint` / `task fe-build` / `task fe-bdd`（全量，含 142/145 的既有 MCP 场景）
- 手工：真实网关上 5 个 `enabled` server（其中 1 个指向不可达地址）跑一次「全部重新探测」，观察进度条逐行推进、失败行标红、期间单行按钮不可点、关闭再打开能恢复

---

## 7. 门禁

- [x] reload 不再阻塞到全部完成 —— `POST` 首字节 < 1s（UT 断言 `202` 立即返回，且 registry 此时已处于 `Running`）
- [x] 进度条在 mock 延迟上游（≥300ms/响应）下可观测到中间态 —— BDD 断言至少一帧内 `done < total` 且 `state == running`
- [x] 失败 server 单独标红且其它行照常完成 —— BDD 场景 2 + UT handler #5
- [x] 并发 reload 被 `409` 拒绝且返回在跑那次的 `run_id` —— UT handler #2
- [x] `GET /reload/latest` 在页面刷新后能恢复进度 —— BDD 场景 5
- [x] `task test` / `task bdd` / `task fe-bdd` 全绿（全量，非过滤）—— 最终树复跑：`task test` **1620 passed / 0 failed**（22 个 test result 块全部 `ok`，`FAILED` 行数 0，`/tmp/p56-evidence/s148-test-final.log`），其中内嵌 BDD 半程 **378 scenarios（365 passed / 13 skipped）· 2238 steps（2225 passed / 13 skipped）**；独立 `task bdd` 复跑同数字（`/tmp/p56-evidence/s148-bdd-r2.log`）；`task fe-bdd` **639 passed / 3 skipped / 0 failed**（25.8m，EXIT=0，`/tmp/p56-evidence/s148-febdd-final.log`，`✘` 计数 0）；`task fe-build` EXIT=0（`/tmp/p56-evidence/s148-fe-build.log`）。门禁之后仅改了前端文案（Stage 147 的用户列空值措辞），`task fe-bdd` 复跑同数字 **639 passed / 3 skipped / 0 failed**（`/tmp/s148-anon-accept/febdd-full.log`）。13 个后端 skip 全部来自 `body_archive_read.feature`（7 步）与 `body_archive_write.feature`（6 步）的既有缺陷（TD-028，非本 Stage 引入）；3 个前端 skip 是 142/145 起就在的既有 skip
- [x] 320px 无横向滚动（进度对话框的明细表 `overflow-x-auto`）
- [x] `en.json` / `zh-CN.json` key 集合一致（`task fe-lint` 内的 `check-i18n-keys.sh`）

---

## 8. 风险与遗留

- **进程内状态在重启后丢失**：用户会看到进度对话框变成「没有进行中的探测」。但**任务也随进程一起终止了**，不存在「以为在跑实际已死」的误导 —— 这是本设计选择「只保留最近一次 + 重启即空」而不用持久化的关键理由。
- **轮询代价**：500ms × 打开对话框的客户端数。admin 页面并发是个位数，单次响应是几十条 JSON，可忽略；但若将来把这里改成「所有登录用户可见」，频率必须重新评估。
- **`409` 的判定是进程级**：多实例部署时每个实例各自允许一次 reload，退化成「N 个实例可同时各跑一次」。登记为已知边界（与 H3 同源），不是本 Stage 的缺陷。
- **并发探测抬高上游瞬时压力**：4 路上限是刻意的折中（§3.4）；若某个上游对并发敏感，只能靠它自己的限流，网关不做 per-server 节流。
- **错误 body 可能很长**：item 的 `error` 截断到 500 字符（与 `PROBE_BODY_LIMIT` 对齐）。更长的上游响应在 UI 上不可见 —— 这是刻意的，避免一条错误把对话框撑爆。
- **`latest()` 只留最近一次**：管理员在两个标签页各点一次时，先点的那个会在第二次 `start` 后收到 `404`。UI 的兜底是 `404` 即显示「该次探测已完成，结果已失效」，不重试。
- **`describe_probe_failure` 读错了一层（本 Stage 自查发现并修复）**：`probe_and_store` 的错误 body 形状是 `{"error": {...}}`（与 `probe_failed`、`db_error`、`not_found` 一致），而最初的实现从顶层取 `message` / `body`，于是 `error` 恒为 `"502 probe failed"` —— 上游说的话被整段丢掉，正好抵消了 ADR-049「错误只留一个可读字符串」的理由。已改为先取 `body["error"]` 再回退到顶层，并在 `reload_reports_a_failed_probe_without_aborting_the_rest` 里加了一条断言（`error` 必须含 probe 原话 `transport error`，不能是兜底串）。**没有这条断言，这个 bug 是静默的**：`starts_with("502 ")` 对两种实现都成立。
- **失败行的两个数字问题（Gate 4 评审发现并修复）**：① **行首状态码取的是网关的 `502`，不是上游的**。`probe_failed` 对所有 `ProbeError` 变体都答 `502`（「本网关没能完成这次探测」），真实上游码另存在 `error.upstream_status`，而 `describe_probe_failure` 从未读它 —— 上游 `404` 会渲染成 `502 …`，把运维引向错误的方向。改为优先取 `upstream_status`，没有才回退到 handler 的状态码。② **上游 body 被打印两遍**。`ProbeError::Http` 的 `Display` 是 `"upstream returned HTTP {status}: {body}"`，而 `describe_probe_failure` 又把 `error.body` 追加到行尾，于是每一条 HTTP 失败都是 `502 upstream returned HTTP 500: <body> · <body>` —— 一个 300 字符的 HTML 页面在一个 `max-w-14rem break-all` 的单元格里出现两次。改为仅当 `message` 尚未包含 body 时才追加。两条都用新 UT `a_failure_row_names_the_upstream_status_once`（`404` 开头 + `no such endpoint` 只出现一次）钉死；前端对应步骤的断言也从 `"502"` 改成上游原话，**不再替后端保护一个有误导性的数字**。
- **`i18n` 首访中文偶发失败（Stage 147/148 全量 `fe-bdd` 暴露，已修复）**：`fe-bdd` 在 `i18n-switcher.feature › First visit with Chinese browser shows Chinese sidebar menu` 上失败两次（含 retry）。根因不在 Stage 147/148 的改动，而在 TD-008a 的懒加载：`i18n.addResourceBundle` 只在**资源 store** 上 emit `added`，不在 i18next 实例上 emit `languageChanged`，而 `react-i18next` 默认 `bindI18n: 'languageChanged'` / `bindI18nStore: ''`，因此 zh-CN chunk 落地后已挂载的 sidebar **不会重渲染**，页面正文已中文、侧栏仍是英文。竞态取决于 chunk 何时到（同一 run 里 desktop 失败、mobile/tablet 通过）。修复：`i18n.init({ react: { bindI18nStore: "added" } })`。这也解释了为什么它只在负载高的全量 run 里出现 —— 单跑 `i18n-switcher` 时 chunk 总是先于首次绘制到达。

---

## 9. 对后续 Stage 的影响

- Stage 146 改过 `probe_server` 的签名，本 Stage 只消费 `probe_and_store` 的返回值形状，排在其后可以避免返工。
- 这个 run registry 是**「管理页长操作进度」的第一个范式**：将来 body archive 的批量任务若也要进度，形状（`run_id` + items + `start/latest`）可以直接照抄。但**不要**为此把它抽象成通用框架 —— 目前只有一个消费者，抽象出来的「通用任务引擎」会立刻被第二个消费者的差异需求顶住。
