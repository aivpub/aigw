# Stage 12 交接手册

**日期**: 2026-07-04
**状态**: Complete
**最后一步**: Stage 12 所有 @mock BDD 场景全部通过 (63/63)

## 当前状态

```
13 features, 72 scenarios (63 passed, 9 skipped)
223 steps (214 passed, 9 skipped)
```

- 63 个 @mock 场景全部通过，9 个 @real_api 场景正确跳过
- CI workflow 已配置（`.github/workflows/bdd.yml`）
- Taskfile 已配置（`task bdd`、`task bdd-real`、`task bdd-all`）

## 关键决策

- ADR-006: BDD 架构 — cucumber-rust + MockUpstream + @mock/@real_api 双模式
- `make_request()` auth 参数不包含 `Bearer ` 前缀（内部自动添加）
- Feature 文件使用 `/`，Rust step 定义使用 `\/`（cucumber expression 要求）
- `max_concurrent_scenarios(1)` 串行执行（共享 MockUpstream 状态）

## 已知问题

- **TD-002**: @real_api 场景缺少 step bindings（P2），需要实现 env-var guard + 真实 HTTP client
- **TD-003**: BDD 覆盖率报告未自动化（P3）

## 文件清单

| 新增/修改 | 文件 |
|-----------|------|
| 新增 | `.github/workflows/bdd.yml` |
| 新增 | `docs/15-bdd-guide.md` |
| 修改 | `Taskfile.yml`（添加 bdd/bdd-real/bdd-all） |
| 修改 | `docs/stages/stage-12.md`（标记 Complete） |
| 修改 | `docs/08-autonomous-decisions.md`（ADR-006） |
| 修改 | `docs/12-technical-debt.md`（TD-002, TD-003） |
| 修改 | `crates/aigw-server/tests/bdd_steps/error_steps.rs`（修复 auth header 重复 Bearer） |
| 修改 | `crates/aigw-server/tests/features/auth.feature`（修正 self-access 端点） |
| 新增 | `crates/aigw-server/tests/features/real/end_to_end_real.feature` |
| 新增 | `crates/aigw-server/tests/features/real/compatibility_real.feature` |

## BDD 场景统计

| Feature | Scenarios | 状态 |
|---------|-----------|------|
| end_to_end.feature | 6 | 全部通过 |
| error_handling.feature | 7 | 全部通过 |
| auth.feature | 5 | 全部通过 |
| keys.feature | 9 | 全部通过 |
| models.feature | 7 | 全部通过 |
| spend.feature | 4 | 全部通过 |
| spend_aggregation.feature | 8 | 全部通过 |
| health.feature | 3 | 全部通过 |
| protocol_conversion.feature | 8 | 全部通过 |
| messages.feature | 6 | 全部通过 |
| end_to_end_real.feature | 6 | 跳过（@real_api） |
| compatibility_real.feature | 3 | 跳过（@real_api） |

## 下一步

Stage 12 是 Phase 5 的收尾 stage。全部 11 个 feature 文件的 @mock 场景均已通过。可以进入下一阶段，或处理 TD-002（real API step bindings）。
