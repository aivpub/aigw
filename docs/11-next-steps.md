# aigw -- 下一步行动

**上次更新**: 2026-07-04
**当前阶段**: Phase 5 规划完成，Stage 7 待开始（BDD 框架搭建）

---

## 当前状态：Phase 0-4 完成，Phase 5 规划完成

### Phase 0-4 已完成（6 个 Stage）

| Stage | 状态 |
|-------|------|
| Stage 0 -- RDD 初始化 | ✅ |
| Stage 1 -- Schema 对齐 + 迁移工具 | ✅ |
| Stage 2 -- Key API + SpendLog | ✅ |
| Stage 3 -- Chat Completions + Router | ✅ |
| Stage 4 -- OpenAPI + Swagger UI | ✅ |
| Stage 5 -- Docker + Deployment | ✅ |
| Stage 6 -- SaaS Architecture | ✅ |

### Phase 5：最小化后端完整版 + BDD 测试（RGR 驱动）

| Stage | 状态 | 目标 | 预估 |
|-------|------|------|------|
| Stage 7 | ⏳ 待开始 | BDD 框架搭建 + 既有功能 .feature | 4-6h |
| Stage 8 | ⏳ 待开始 | 模型管理 CRUD（BDD 驱动） | 6-8h |
| Stage 9 | ⏳ 待开始 | Provider 适配转换层（BDD 驱动） | 6-8h |
| Stage 10 | ⏳ 待开始 | Claude /v1/messages + SSE Streaming（BDD 驱动） | 6-8h |
| Stage 11 | ⏳ 待开始 | Usage 用量查询增强（BDD 驱动） | 2-3h |
| Stage 12 | ⏳ 待开始 | BDD 全量覆盖 + 集成测试体系 | 4-6h |

详见各 Stage 独立文档：`docs/stages/stage-{7..12}.md`

## 立即行动

1. **Stage 7**: BDD 框架搭建 — cucumber-rust 集成，为既有 /key/* /spend/* /health/* 写 .feature
2. **Stage 8**: 模型管理 CRUD — `proxy_models` 表（litellm v1.90.3 兼容）
3. **Stage 9-10**: Provider 适配层 + Claude 端点（依赖链）

## 长期路线（Phase 6）

参见 `docs/stages/stage-roadmap.md` Phase 6 表格：
- LT-1 多租户管理 API (P1)
- LT-4 前端管理控制台 (P1)
- LT-2 Redis 缓存 (P2)
- LT-3 Observability (P2)
- LT-6 PostgreSQL 生产级 (P2)

## 关键技术成果（Phase 0-4）

- **141 个测试** 全覆盖
- **SQLite / MySQL / PostgreSQL** 三数据库支持
- **OpenAPI 3.1** 完整规范
- **Docker Compose** 一键部署
- **双向迁移工具** litellm ↔ aigw
- **系统信息端点** `/system/info`
