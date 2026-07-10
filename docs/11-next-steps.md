# aigw -- 下一步行动

**上次更新**: 2026-07-10
**当前阶段**: Phase 13 — 进行中（Stage 34 ✅，Stages 35-39 待开始）

---

## 当前状态：Phase 13 待开始 ⏳

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅ 项目基础设施 + 功能对等 + 部署就绪
Phase 5:    ████████████████████ 100% (6/6)  ✅ 最小化后端 + BDD 测试
Phase 7:    ████████████████████ 100% (5/5)  ✅ 生产 litellm 迁移
Phase 8:    ████████████████████ 100% (3/3)  ✅ 生产化基础（日志/多租户/健康检查）
Phase 9:    ████████████████████ 100% (4/4)  ✅ 前端管理控制台
Phase 11:   ████████████████████ 100% (6/6)  ✅ 前端质量加固 + 安全达标
Phase 12:   ████████████████████ 100% (3/3)  ✅ 前端导航重构 + Playground
Phase 13:   ███░░░░░░░░░░░░░░░░░  17% (1/6)  🔄 前端反馈改进（Stage 34 ✅）
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端 BDD | cucumber-rust + libtest | 72 scenarios (63 mock + 9 real_api) |
| 前端 BDD | Playwright + playwright-bdd | 102 tests (34 scenarios × 3 viewports) |

## 交付成果

- **33/33 Stages** 全部完成
- **BDD 72 后端场景 + 102 前端测试** 全部通过
- **SQLite / MySQL / PostgreSQL** 三数据库支持
- **Docker Compose** 一键部署
- **Rust 单二进制部署**（rust-embed 嵌入前端）
- **前端管理控制台** 8 页面（Usage、Keys、Models、Users、Orgs、Teams、Spend Logs、Playground）

## Phase 13 规划（Stages 34-39）

基于用户使用反馈 + TTFT 实现差距调研 + daily_spend 聚合表迁移，规划 6 个 Stage（每个 3.5-5.5h）：

| Stage | 目标 | 类型 | 预估 | 优先级 |
|-------|------|------|------|--------|
| Stage 34 | SSE Streaming + completion_start_time + Spend Logs 增强 | 后端 | 5h | P0 |
| Stage 35 | daily_spend 聚合表迁移 + 定时写入 | 后端 | 3.5h | P0 |
| Stage 36 | 前端 Spend Logs 重构（Live Tail+预设+抽屉） | 前端 | 5h | P0 |
| Stage 37 | Users/Orgs 端到端修复 + Provider 解密 | 前后端 | 4.5h | P0 |
| Stage 38 | Usage 聚合端点 + 前端 Global 视图重构 | 前后端 | 5.5h | P1 |
| Stage 39 | Playground 聊天式对话升级 | 前端 | 5h | P2 |

详见 `docs/plans/2026-07-10-phase-13-feedback-improvements.md`

### 关键发现：TTFT 差距

调研发现 `completion_start_time` 列在 schema 中存在但从未被写入（全部硬编码 None），同时 streaming 路径未真正实现 SSE 代理（返回 stub JSON）。Stage 34 将修复这两个问题。

详见 memory `[[ttft-implementation-gap]]`

## Phase 12 完成（Stages 31-33）

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 31 | 侧边栏分组重构 + Usage 重命名（对齐 litellm 5组结构） | ✅ |
| Stage 32 | Spend Logs 独立页面 | ✅ |
| Stage 33 | Playground Chat 调试页 | ✅ |

详见 `docs/plans/2026-07-08-sidebar-playground-redesign.md`
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
| ADR-001 | RDD Framework Adoption | 2026-07-03 |
| ADR-002 | SQLite 默认 + 多数据库支持 | 2026-07-03 |
| ADR-003 | litellm Schema 兼容性 | 2026-07-03 |
| ADR-004 | Dual-Mode SaaS 架构 | 2026-07-03 |
| ADR-005 | Taskfile.yml 统一工作流入口 | 2026-07-03 |
| ADR-006 | BDD with cucumber-rust + Mock Upstream | 2026-07-04 |
| ADR-007 | React + TypeScript + shadcn/ui 前端技术栈 | 2026-07-08 |
| ADR-008 | rust-embed 单二进制前端部署 | 2026-07-08 |
| ADR-009 | 核心 Stages 0-30 完成，延迟 Phase 10 | 2026-07-08 |
| ADR-010 | Phase 12 完成 — Sidebar + Playground + Spend Logs | 2026-07-09 |
