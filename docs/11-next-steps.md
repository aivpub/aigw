# aigw -- 下一步行动

**上次更新**: 2026-07-08
**当前阶段**: Phase 0-9 全部完成（24/24 Stages），Phase 10 长期路线待触发

---

## 当前状态：Phase 0-9 全部完成（24/24 Stages）

### Phase 0-7 全部完成

| Phase | Stages | 状态 |
|-------|--------|------|
| Phase 0-4 | Stage 0-6 | ✅ |
| Phase 5 | Stage 7-12 | ✅ |
| Phase 7 | Stage 13-17 | ✅ |

### Phase 8：生产化基础（全部完成）

| Stage | 状态 | 目标 |
|-------|------|------|
| Stage 18 | ✅ 完成 | 结构化日志 — tracing + tracing-subscriber + JSON 格式 + request_id |
| Stage 19 | ✅ 完成 | 多租户管理 API — /org/* /team/* /user/* CRUD（15 端点，BDD 驱动） |
| Stage 20 | ✅ 完成 | 健康检查增强 — /health/metrics（DB 连接池、uptime、key/model 计数） |

### Phase 9：前端管理控制台（全部完成）

| Stage | 状态 | 目标 |
|-------|------|------|
| Stage 21 | ✅ 完成 | 前端工程搭建 — Vite + React + shadcn/ui + rust-embed 集成 |
| Stage 22 | ✅ 完成 | Key 管理页面 — 列表/搜索/创建/编辑/删除/复制 API key |
| Stage 23 | ✅ 完成 | 用量 Dashboard — 支出卡片 + 图表 + spend logs 表格 + 日期筛选 |
| Stage 24 | ✅ 完成 | Model 管理页面 — proxy_models 列表 + 详情展开 |

详见各 Stage 文档：`docs/stages/stage-{13..24}.md`

## 技术债状态

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001 | ✅ 已解决 | Dead code cleanup |
| TD-002 | ✅ 已解决 | @real_api step bindings 实现完成 |
| TD-003 | ⏳ 待处理 | BDD 覆盖率报告自动化（P3） |

## 待完成：rust-embed 前端集成

前端 dist/ 需要嵌入 aigw-server binary，通过 `/admin` 路径提供 SPA 服务（含 client-side routing fallback）。

## 长期路线（Phase 10）

参见 `docs/stages/stage-roadmap.md` Phase 10 表格：
- LT-2 Redis 缓存 + 性能优化 (P2)
- LT-3 Observability: Prometheus + OTEL (P2)
- LT-5 SSO/OAuth 鉴权 (P3)
- LT-6 PostgreSQL 生产级支持 (P2)
- LT-7 Kubernetes Operator + Helm Chart (P3)

## 关键技术成果

- **BDD 72 场景全部通过**（63 @mock + 9 @real_api）
- **257 步骤**全覆盖
- **SQLite / MySQL / PostgreSQL** 三数据库支持
- **OpenAPI 3.1** 完整规范
- **Docker Compose** 一键部署
- **双向迁移工具** litellm ↔ aigw（含 pre-check + rollback.sh）
- **结构化日志** JSON 格式 + UUID v7 request_id
- **多租户管理 API** org/team/user CRUD（15 端点）
- **健康检查增强** /health/metrics（DB 连接池、uptime、key/model 计数）
- **前端管理控制台** React + shadcn/ui（Dashboard、Keys、Models、Health）
- **ADR-007** 前端技术栈决策记录
