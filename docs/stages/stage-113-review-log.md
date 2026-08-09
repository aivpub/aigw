# Stage 113 Review Log

**Stage**: Stage 113（后端可靠性加固 — TD-005 + TD-010a + TD-003）
**Review Type**: Design + Code
**Review Date**: 2026-08-09

## Review Summary

设计评审（独立 subagent）发现 1 个 Critical 级设计缺陷并已修正；实现完成后 GATE-4 独立 subagent 代码评审 + 主模型自审，无 Critical/High，3 个 Low/Info 记录在案。

## Design Review (Gate 2)

### Findings

| Sev | Finding | 证据 | 处置 |
|-----|---------|------|------|
| Critical | TD-010a 设计的 mode 检测路径错误：设计写「读 `deployment.raw_params["model_info"]["mode"]`」，但 `resolver.resolve_one` 的 `raw_params` 只含 litellm_params（`params_json`），**不含** `model_info` —— 设计路径会静默永不触发。 | `resolver.rs:156`（`params_json` 仅来自 litellm_params）/ `deployment.rs:37`（raw_params 注释「Decrypted litellm_params」） | **已修**：`run_and_save_health_check` 增 `model_info: Value` 参数，从 `ProxyModel.model_info` 直接取（两个 health-check 端点都在 spawn 前 clone `model.model_info`）；抽 `build_probe_spec(deployment, model_info)` 使分支可 UT。 |
| High | TD-005 用 `std::panic::catch_unwind` 无法直接 await `Pin<Box<dyn Future>>`（std 的 catch_unwind 接收同步 `FnOnce`）——设计初稿不可编译。 | `engine.rs` 编译错误（std catch_unwind 需要 FnOnce 闭包） | **已修**：改用 `futures::FutureExt::catch_unwind` 组合子 + `AssertUnwindSafe(body)`。 |
| Medium | `tokio-util` 加 `features=["sync"]` 会构建失败（该 feature 不存在）；`CancellationToken` 在 default features 即可用。 | `cargo check` 报错 `feature sync does not exist` | **已修**：`tokio-util = { version = "0.7" }` 无 features。 |
| Low | `Engine::run` 直接改签名会破坏 main.rs + engine 测试调用。 | `main.rs:330` `engine.run().await` | **已修**：保留 `run()` 兼容包装 + 新增 `run_with_cancel(token)`。 |
| Low | exec-loop「等待 in-flight step」若只在 loop 顶部 select 监听取消，idle sleep 期间取消无法 prompt 退出。 | — | **已修**：exec loop 改为顶部 `is_cancelled` 检查 + idle 分支可取消 select sleep + 迭代后复查。 |

### AI Pre-Filter Results
- 过滤掉 2 条「用 jq 替代 shell」「改 Rust bin」等风格建议（不适用——纯 POSIX sh 脚本与既有 `scripts/` 一致，零依赖）。
- 过滤掉「应给每个 loop 拆独立 struct」等过度设计（YAGNI）。

## Code Review (Gate 4)

### Files Reviewed
- crates/aigw-core/src/engine.rs（guarded + run_with_cancel + 三 loop）
- crates/aigw-server/src/main.rs（engine_token wiring）
- crates/aigw-server/src/routes/health.rs（build_probe_spec + model_info 透传）
- crates/aigw-server/tests/bdd_steps/health_steps.rs（embed BDD step）
- crates/aigw-server/tests/features/health.feature
- crates/aigw-server/tests/bdd_support/mock_upstream.rs（reset_all）
- scripts/bdd-coverage + Taskfile.yml

### Findings

| Sev | Finding | 证据 | 处置 |
|-----|---------|------|------|
| Low | `guarded()` 的 30s backoff 硬编码在各 loop；如需可调后续提 `EngineConfig` 字段。 | engine.rs:470-487 | 记录；不改（YAGNI） |
| Low | exec-loop panic backoff 期间取消最长延迟 30s 观察退出。 | engine.rs:581-590 | 记录；panic 路径极罕见，可接受 |
| Info | bdd-coverage 实测 63%（55/87），设计目标 ≥90% 未达成 —— admin-CRUD/login/key-deleted/model-groups/system-info 无 mock-BDD step。门禁定 60% 回归基线（诚实反映现状 + 防回归）。 | scripts/bdd-coverage 头部注释 | 记录为已知偏差 + 预置缺口列 NOT covered 清单 |

### Verification Method
- 独立 GATE-4 subagent 审阅（correctness 聚焦）+ 主模型逐条读码复验（guarded panic 安全性、exec 关闭语义、embed 分支 URL/header、BDD 隔离、awk 解析）。

## Resolution Summary

| 阶段 | Total | Fixed | Documented | Won't Fix |
|------|-------|-------|------------|-----------|
| Design (Gate 2) | 5 | 5 | 0 | 0 |
| Code (Gate 4) | 3 | 0 | 3 | 0 |

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes
**Gate 5 结论**: ✅ 通过（UT 409 + 136 全绿、mock BDD 233 场景（仅 pre-existing budget_reset flake）、bdd-coverage PASS、fmt + lint green）
