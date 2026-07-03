# Claude Code 入口文件

> aigw — AI Gateway 项目 Claude Code 快速参考

## 项目信息

- **名称**: aigw (AI Gateway)
- **目标**: litellm proxy Rust 最小兼容替代
- **技术栈**: Rust + axum + sqlx + tokio
- **章程**: docs/01-charter.md

## 快速命令

```bash
task doctor      # 检查项目健康状态
task test        # 运行测试
task status      # 显示状态
cargo check      # 编译检查
cargo build      # 构建
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
