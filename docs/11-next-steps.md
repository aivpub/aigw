# aigw -- 下一步行动

**上次更新**: 2026-07-07
**当前阶段**: Phase 7 Stage 13-16 完成，Stage 17 收尾中

---

## 当前状态：Phase 0-5 完成，Phase 7 Stage 13-16 完成

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

| Stage | 状态 | 目标 |
|-------|------|------|
| Stage 7 | ✅ | BDD 框架搭建 + 既有功能 .feature |
| Stage 8 | ✅ | 模型管理 CRUD（BDD 驱动） |
| Stage 9 | ✅ | Provider 适配转换层（BDD 驱动） |
| Stage 10 | ✅ | Claude /v1/messages + SSE Streaming（BDD 驱动） |
| Stage 11 | ✅ | Usage 用量查询增强（BDD 驱动） |
| Stage 12 | ✅ | BDD 全量覆盖 + 集成测试体系 |

### Phase 7：生产 litellm 迁移到 aigw

| Stage | 状态 | 目标 |
|-------|------|------|
| Stage 13 | ✅ | credentials 表 + CredentialsStore + 全量 Store PG/MySQL 补齐 |
| Stage 14 | ✅ | NaCl 加密/解密 + aigw-migrate PG 源 + master_key 提取 |
| Stage 15 | ✅ | aigw-migrate 全量迁移（解密 litellm → 重加密 aigw） |
| Stage 16 | ✅ | aigw 运行时解密 + 凭证引用解析 |
| Stage 17 | 🔄 | 生产迁移 SOP + export 增强 + 回滚方案（SOP/export/BDD 已完成，缺自动化预检/监控） |

详见各 Stage 独立文档：`docs/stages/stage-{7..12}.md`

## 技术债状态

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001 | ✅ 已解决 | Dead code cleanup |
| TD-002 | ✅ 已解决 | @real_api step bindings 实现完成 |
| TD-003 | ⏳ 待处理 | BDD 覆盖率报告自动化（P3） |

## 立即行动

1. **Stage 17 收尾**: 自动化预检工具 + 一键回滚脚本
2. **TD-003**: BDD 覆盖率报告自动化（P3）

## 长期路线（Phase 6）

参见 `docs/stages/stage-roadmap.md` Phase 6 表格：
- LT-1 多租户管理 API (P1)
- LT-4 前端管理控制台 (P1)
- LT-2 Redis 缓存 (P2)
- LT-3 Observability (P2)
- LT-6 PostgreSQL 生产级 (P2)

## 关键技术成果（Phase 0-5）

- **BDD 72 场景全部通过**（63 @mock + 9 @real_api）
- **257 步骤**全覆盖
- **SQLite / MySQL / PostgreSQL** 三数据库支持
- **OpenAPI 3.1** 完整规范
- **Docker Compose** 一键部署
- **双向迁移工具** litellm ↔ aigw
