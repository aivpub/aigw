# aigw -- 下一步行动

**上次更新**: 2026-07-08
**当前阶段**: Phase 11 — Stage 25（前端 BDD 测试基础设施）

---

## 当前状态：Phase 0-9 全部完成 + Phase 11 规划中

### Phase 0-9 全部完成（24/24 Stages）

| Phase | Stages | 状态 |
|-------|--------|------|
| Phase 0-4 | Stage 0-6 | ✅ |
| Phase 5 | Stage 7-12 | ✅ |
| Phase 7 | Stage 13-17 | ✅ |
| Phase 8 | Stage 18-20 | ✅ |
| Phase 9 | Stage 21-24 | ✅ |

### Phase 11：前端质量加固 + 安全达标（规划中）

| Stage | 状态 | 目标 | 优先级 |
|-------|------|------|--------|
| Stage 25 | ⏳ 待开始 | 前端 BDD 测试基础设施 — Playwright + Gherkin + 截图/GIF + Mock API | P0 |
| Stage 26 | ⏳ 待开始 | 登录安全对齐 Litellm — `/v2/login` JWT + Cookie + scrypt | P0 |
| Stage 27 | ⏳ 待开始 | 移动端适配 — 全页面响应式改造 | P1 |
| Stage 28 | ⏳ 待开始 | Key 创建 UX 修复 — Token 展示 + 复制确认 | P0 |
| Stage 29 | ⏳ 待开始 | 用户/组织/团队管理前端页面 | P1 |
| Stage 30 | ⏳ 待开始 | Dashboard 数据接入 + 移动端图表 | P2 |

**依赖关系**:
```
Stage 25 (BDD 基础设施)
  ├── Stage 26 (登录安全)
  ├── Stage 27 (移动端)
  │     └── Stage 29 (用户管理页面)
  ├── Stage 28 (Key UX)
  └── Stage 30 (Dashboard 数据)
```

详见各 Stage 文档：`docs/stages/stage-{25..30}.md` 和 `docs/stages/stage-roadmap.md`

---
## 立即行动：Stage 25 — 前端 BDD 测试基础设施

**问题**：前端没有任何自动化测试。Login 跳转失败、Key 创建 UX bug 等问题因无测试而未被发现。

**方案**：Playwright + `playwright-bdd`（Gherkin .feature）覆盖 Login/Dashboard/Keys/Models 4 页面。

**覆盖范围**：
- 3 种 viewport：375px（手机）、768px（平板）、1280px（桌面）
- 失败截图 + trace + video（人工复查用）
- Mock API（Playwright route interception）

**预估**: 4-6h

---
## 技术债状态

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001 | ✅ 已解决 | Dead code cleanup |
| TD-002 | ✅ 已解决 | @real_api step bindings 实现 |
| TD-003 | ⏳ 待处理 | BDD 覆盖率报告自动化（P3） |
| TD-004 | 🔴 新增 | 登录裸 sk 存 localStorage（P0，Stage 26 修复） |
| TD-005 | 🔴 新增 | 前端无自动化测试（P0，Stage 25 修复） |
| TD-006 | 🟡 新增 | Key 创建后 token 不可见（P0，Stage 28 修复） |

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
- **前端管理控制台** React + shadcn/ui（Dashboard、Keys、Models）
- **ADR-007** 前端技术栈决策记录

## 长期路线（Phase 10）

参见 `docs/stages/stage-roadmap.md` Phase 10 表格
