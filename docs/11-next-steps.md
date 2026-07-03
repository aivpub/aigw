# 下一步计划

**项目**: aigw — AI Gateway
**最后更新**: 2026-07-03

## 当前状态

**阶段**: Stage 0 — 项目初始化（✅ 完成）
**下一 Stage**: Stage 1 — Schema 100% 对齐 + aigw-migrate 双向迁移工具

## Stage 0 产出总结

- [x] RDD 框架初始化 + `.rdd/config.yml` 配置
- [x] 项目章程 `docs/01-charter.md` v2.0（多租户最小化兼容、双向迁移、长期路线、OpenAPI/前端、云/自托管）
- [x] litellm diff 基线 `docs/litellm-diff-baseline.md`（表名映射 §5 + 双向迁移策略 §6）
- [x] Rust workspace：`crates/aigw-core` + `crates/aigw-server`
- [x] 数据模型：全部 11 张表（aigw 自有表名，litellm 列兼容）
- [x] 表名决策：aigw 使用自有表名，`aigw-migrate` 负责双向映射
- [x] Stage 路线图 `docs/stages/stage-roadmap.md`

## Stage 0 关键决策记录

| 决策 | 结论 | 文档位置 |
|------|------|---------|
| 表名 | aigw 自有表名（`virtual_keys` 等），不照搬 litellm 表名 | diff-baseline.md §5 |
| 迁移 | `aigw-migrate import/export/verify` 双向迁移工具，Stage 1 交付 | diff-baseline.md §6 |
| 多租户 | 完整保留 Org/Team/User/Project/Budget（9 张表），只读 API | charter.md §6 |
| 长期 | 7 条长期路线，不在最小化版本做但不阻断演进 | charter.md §8 |
| OpenAPI | Stage 4 生成，覆盖全部核心端点 | charter.md §7 |
| 部署 | 企业自托管（Stage 1-5）+ 云服务 SaaS（Stage 6）| charter.md §6 |

## 下一步：Stage 1

具体任务：
1. 编写完整 SQLite migration SQL（9 张表，aigw 自有表名，litellm 列兼容）
2. 补齐所有 index（startTime, api_key, user_id+team_id, budget_reset_at+expires, session_id）
3. 实现 `aigw-migrate` 工具（import + export + verify）
4. Smoke test 完整往返：litellm DB → import → aigw 启动 → 运行 → export → litellm 验证

详见 `docs/stages/stage-1.md`（待创建）。
