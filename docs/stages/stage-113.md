# Stage 113: 后端可靠性加固（TD-005 Async Engine 容错 + TD-010a health embedding 探测 + TD-003 BDD 覆盖率）

**所属**: Phase 45（技术债清理）
**预估**: 8h（后端 + 测试）
**依赖**: 无（TD-005 / TD-010a / TD-003 独立可并行）

---

## 1. 目标

后端三项技术债收敛为一个 Stage，均改动收敛、风险低、立即释放价值：
1. **TD-005** — Async Engine 三 loop panic 容错 + CancellationToken 优雅关闭
2. **TD-010a** — health.rs 对 embedding-only 模型走 `/embeddings` 探测（当前误报 400 unhealthy）
3. **TD-003** — BDD 端点覆盖率报告脚本 + `task bdd-coverage`

## 2. 核心设计

### 2.1 TD-005: Engine panic 容错 + 优雅关闭

**现状**（`engine.rs`）：
- `tick_loop`（L437）/ `exec_loop`（L468）/ `cleanup_loop`（L501）均 `loop { ... }`，体内任何 panic 让该 task 永久死掉，吞吐静默下降
- `Engine::run`（L62）`for h in handles { h.await }` 永不返回，无 shutdown channel

**改动**：
- 每个 loop 体用 `std::panic::AssertUnwindSafe + catch_unwind` 包裹单次迭代；panic 时 `error!` log + sleep 30s + continue（不退出 loop）
- `Engine::run` 增 `CancellationToken` 参数（`tokio-util` `sync`）：每个 loop 内 `tokio::select!` 监听 token；取消后优雅退出（等待当前 step 完成），`run` 返回
- main.rs：`shutdown_signal()` 触发后 `token.cancel()`；`tokio::spawn(async move { engine.run(token).await })`
- 新增 `tokio-util` 依赖（`features=["sync"]`）

**TDD**: ~6 UT
| # | Test | 断言 |
|---|------|------|
| 1 | loop 内 panic 不退出 | tick 抛 panic → 下一轮正常执行（log 恢复）|
| 2 | cancellation 让 run 返回 | token.cancel() 后 `run()` 返回（不再无限 await）|
| 3 | 取消时等待 in-flight step | exec loop 当前 step 完成后才退出 |
| 4 | 正常（无取消）不退出 | 不 cancel → run 仍挂起 |
| 5 | panic 恢复后 loop 继续 | panic → sleep 30s → 继续 tick（UT 用假 tick 计数）|
| 6 | multi-loop 独立恢复 | 一个 loop panic 不影响其他 loop |

### 2.2 TD-010a: health embedding-mode 探测

**现状**（`health.rs:266 run_and_save_health_check`）：resolve 后对所有模型 POST `{api_base}/chat/completions` `{model, messages, max_tokens:1}`；embedding-only 模型 400 → 误报 unhealthy。

**改动**：
- resolve 后读 `deployment.raw_params["model_info"]["mode"]`（或 `proxy_models.model_info.mode`）判断 `mode=="embed"`
- embed 分支：probe URL = `{api_base}/embeddings`，body `{model: upstream_model, input:["ping"]}`，Auth Bearer；healthy 判定同现有（2xx/401/429/422 视为可达）
- 非 embed 保持现有 chat 探测路径

**TDD**: ~4 UT + 1 BDD
| # | Test | 断言 |
|---|------|------|
| 1 | embed 模型 probe URL = /embeddings | 构造 mock 上游断言收到 `/embeddings` 请求 |
| 2 | embed 模型 body = input:["ping"] | mock 断言 body 无 messages |
| 3 | 非 embed 保持 /chat/completions | 现有探测路径不变 |
| 4 | embed 探测 200 → healthy | save_result healthy=true |

### 2.3 TD-003: BDD 覆盖率报告

**改动**：`scripts/bdd-coverage`（Rust bin 或 shell + jq）：
- 解析 `tests/features/*.feature` 场景文本 → 提取 HTTP method + path（正则匹配 `发送 (GET|POST|PUT|DELETE) /path`）
- 对照路由表（`main.rs` / openapi.rs 端点清单）→ 输出「已覆盖 / 未覆盖端点」+ 覆盖率%
- 挂 `task bdd-coverage`（Taskfile 新增）

**TDD**: 1 脚本自测（mock feature 断言覆盖率输出）。

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/engine.rs` | 修改 | 三 loop catch_unwind + run 接 CancellationToken |
| `crates/aigw-core/Cargo.toml` | 修改 | + tokio-util (sync) |
| `crates/aigw-server/src/main.rs` | 修改 | shutdown_signal → CancellationToken 传入 engine |
| `crates/aigw-server/src/routes/health.rs` | 修改 | embedding-mode 探测分支 |
| `scripts/bdd-coverage*` | 新增 | 覆盖率脚本 |
| `Taskfile.yml` | 修改 | + task bdd-coverage |
| `crates/aigw-server/tests/features/health.feature` | 修改 | +1 BDD（embed 模型 healthy）|

## 4. 验收标准

- [ ] `task test` 全量 UT 通过（含新增 ~10）
- [ ] `task bdd` mock BDD 全绿（含 embed health 场景）
- [ ] `task bdd-coverage` 输出覆盖率报告（≥90%）
- [ ] fmt + lint 全绿
