# Claude Code 入口文件

> aigw — AI Gateway 项目 Claude Code 快速参考

## 项目信息

- **名称**: aigw (AI Gateway)
- **目标**: litellm proxy Rust 最小兼容替代
- **技术栈**: Rust + axum + sqlx + tokio
- **章程**: docs/01-charter.md

## 纪律红线（Agent 工作纪律）

**绝对禁止裸命令执行。任何工作任务（编译、测试、构建、运行、部署）必须通过 Taskfile 的 `task` 命令执行，严禁直接使用 `cargo test`、`cargo check`、`cargo build`、`cargo run` 等裸命令。**

违反此纪律是本次对话中反复出现的问题——Agent 跳过 task 直接跑裸命令，导致缺少 Taskfile 内置的关键环境变量（如 `AIGW_TEST_START_SERVER=1`、`AIGW_TEST_DB_DRIVER` 等），测试结果失真，浪费大量时间排查。

```yaml
# 正确 ✅
task test              # 运行全量单元测试（门禁/交付结论只认这个）
task test-filter -- <pattern>   # 开发迭代期按名字过滤跑 aigw-core lib 测试
                                # 例：task test-filter -- websearch
task bdd               # Mock BDD 测试
task bdd-real-sqlite   # SQLite 真实 BDD
task bdd-real-pg       # PostgreSQL 真实 BDD
task bdd-real-mysql    # MySQL 真实 BDD
task doctor            # 编译检查（cargo check --workspace + clippy）+ 项目健康状态
task build             # 构建 release 二进制
task fmt               # 格式检查（只读 --check）
task fmt-fix           # 实际格式化代码（写入文件）
task lint              # clippy -D warnings

# 错误 ❌
cargo test --test bdd        # 缺少 AIGW_TEST_START_SERVER 等环境变量
AIGW_REAL_API=1 cargo test   # 缺少完整配置链
cargo test -p aigw-core --lib websearch   # 应使用 task test-filter -- websearch
cargo fmt --all               # 应使用 task fmt-fix
cargo check                   # 应使用 task doctor（Taskfile 中无 check task）
task check                    # ⚠️ 此 task 不存在，会报 "Task not found" 错误
```

**过滤测试的边界（`task test-filter`）**：它是**迭代加速工具，不是证据**。任何「门禁通过」「Stage 完成」「交付验证」的结论，**必须以 `task test` 全量结果为准**。用过滤结果充当交付依据等同于谎报。

Agent 在处理任何需要执行命令的任务时，必须先查阅 `Taskfile.yml` 找到对应 task，使用 `task <name>` 执行。如果 Taskfile 中没有对应 task，**必须先停下来和用户讨论是否添加**，而不是「因为没有 task」就自作主张跑裸命令——后者是本纪律最常见的破防方式（2026-10-06 Stage 134 实际发生过：发现 `task check` 不存在后直接跑了 `cargo check`、`cargo fmt --all`、`cargo test` 过滤版共十余次）。

## 快速命令

```bash
task doctor      # 编译检查（cargo check --workspace + clippy）+ 项目健康状态
task test        # 运行全量单元测试
task bdd         # Mock BDD 测试
task fmt-fix     # 格式化代码
task status      # 显示状态
```

## RDD 技能

| 技能 | 用途 |
|-------|---------|
| /rdd-stage-auto | 执行 Stage 并通过门禁 |
| /rdd-knowledge | 记录 ADR / 技术债 |

## 关键文档

| 文档 | 用途 |
|------|------|
| `docs/01-charter.md` | 项目章程（愿景、目标、边界、长期路线） |
| `docs/stages/stage-roadmap.md` | Stage 路线图（7 阶段 + 长期路线） |
| `docs/stages/stage-0.md` | 当前 Stage 详情 |
| `docs/11-next-steps.md` | 下一步行动 |
| `docs/08-autonomous-decisions.md` | 自主决策记录 (ADR) |
| `docs/12-technical-debt.md` | 技术债账本 |

## 读取顺序（新会话）

1. `docs/11-next-steps.md` — 当前进度
2. `docs/01-charter.md` — 项目边界
3. 当前 Stage 文档 — 工作详情
4. `docs/08-autonomous-decisions.md` — 关键决策
5. `docs/12-technical-debt.md` — 已知问题
