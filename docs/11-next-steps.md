# aigw -- 下一步行动

**上次更新**: 2026-07-08
**当前阶段**: 全部完成（30/30 Stages）

---

## 当前状态：Phase 0-11 全部完成

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅ 项目基础设施 + 功能对等 + 部署就绪
Phase 5:    ████████████████████ 100% (6/6)  ✅ 最小化后端 + BDD 测试
Phase 7:    ████████████████████ 100% (5/5)  ✅ 生产 litellm 迁移
Phase 8:    ████████████████████ 100% (3/3)  ✅ 生产化基础（日志/多租户/健康检查）
Phase 9:    ████████████████████ 100% (4/4)  ✅ 前端管理控制台
Phase 11:   ████████████████████ 100% (6/6)  ✅ 前端质量加固 + 安全达标
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端 BDD | cucumber-rust + libtest | 72 scenarios (63 mock + 9 real_api) |
| 前端 BDD | Playwright + playwright-bdd | 69 tests (23 scenarios × 3 viewports) |

## 交付成果

- **30/30 Stages** 全部完成
- **BDD 72 后端场景 + 69 前端测试** 全部通过
- **SQLite / MySQL / PostgreSQL** 三数据库支持
- **Docker Compose** 一键部署
- **Rust 单二进制部署**（rust-embed 嵌入前端）
- **前端管理控制台** 6 页面（Dashboard、Keys、Models、Users、Orgs、Teams）
- **移动端适配** 全页面响应式（375px/768px/1280px）
- **登录安全** JWT + Cookie + scrypt 密码哈希（对齐 litellm v2/login）
- **OpenAPI 3.1** 完整规范 + Swagger UI
- **结构化日志** JSON 格式 + UUID v7 request_id
- **多租户管理 API** org/team/user CRUD（15 端点）
- **生产迁移工具** aigw-migrate + pre-check + rollback.sh

## 后续路线（Phase 10）

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-2 | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-3 | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-5 | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-6 | PostgreSQL 生产级支持 + 迁移工具 | P2 | 多实例 + 高可用 |
| LT-7 | Kubernetes Operator + Helm Chart | P3 | 云原生客户需求 |

## 技术债

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001 | ✅ | Dead code cleanup |
| TD-002 | ✅ | @real_api step bindings |
| TD-003 | ⏳ | BDD 覆盖率报告自动化（P3） |
| TD-004 | ✅ | 登录裸 sk → Stage 26 JWT+Cookie |
| TD-005 | ✅ | 前端无测试 → Stage 25 BDD |
| TD-006 | ✅ | Key token 不可见 → Stage 28 修复 |

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-001 | SQLite 默认 + 多数据库支持 | 2026-07-03 |
| ADR-002 | 纯 DB 迁移方案（非 API 中转） | 2026-07-05 |
| ADR-003 | NaCl SecretBox 解密 + master_key 重加密 | 2026-07-06 |
| ADR-004 | aigw-migrate 工具设计 | 2026-07-06 |
| ADR-005 | 结构化日志方案（tracing + JSON） | 2026-07-08 |
| ADR-006 | 多租户 API 设计 | 2026-07-08 |
| ADR-007 | 前端技术栈决策 | 2026-07-08 |
