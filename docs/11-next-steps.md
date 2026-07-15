# aigw -- 下一步行动

**上次更新**: 2026-07-15
**当前阶段**: Phase 19 UI Enhancement（Models CRUD + Spend Logs 可视化）

---

## 当前状态：54/58 Stages 已完成，Phase 19 规划完成待执行

---

## 当前状态：全部 54 Stages 已完成

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅ 项目基础设施 + 功能对等 + 部署就绪
Phase 5:    ████████████████████ 100% (6/6)  ✅ 最小化后端 + BDD 测试
Phase 7:    ████████████████████ 100% (5/5)  ✅ 生产 litellm 迁移
Phase 8:    ████████████████████ 100% (3/3)  ✅ 生产化基础（日志/多租户/健康检查）
Phase 9:    ████████████████████ 100% (4/4)  ✅ 前端管理控制台
Phase 11:   ████████████████████ 100% (6/6)  ✅ 前端质量加固 + 安全达标
Phase 12:   ████████████████████ 100% (3/3)  ✅ 前端导航重构 + Playground
Phase 13:   ████████████████████ 100% (6/6)  ✅ 前端反馈改进（Stages 34-39）
Phase 14:   ████████████████████ 100% (4/4)  ✅ /v1/messages 接口修复（Stages 40-43）
Phase 15:   ████████████████████ 100% (3/3)  ✅ 反馈改进（Stages 44-46）
Phase 16:   ████████████████████ 100% (3/3)  ✅ Playground 增强（Stages 47-49）
Phase 17:   ████████████████████ 100% (3/3)  ✅ 代理转发架构重构（Stages 50-52）
Phase 18:   ████████████████████ 100% (2/2)  ✅ Spend Logs & Usage 质量修复（Stages 53-54）
Phase 19:   ░░░░░░░░░░░░░░░░░░░░   0% (2/2)  🔄 UI Enhancement（Stages 55-56）
Phase 20:   ░░░░░░░░░░░░░░░░░░░░   0% (2/2)  ⏳ 可观测性增强（Stages 57-58）
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端单元 | libtest | 269 tests |
| 后端 BDD | cucumber-rust | 93 scenarios (91 passed, 2 skipped) |
| 前端 BDD | Playwright + playwright-bdd | 108 tests (36 scenarios × 3 viewports) |

---

## 优先级排序

| 优先级 | Phase | 目标 | 原因 |
|--------|-------|------|------|
| P2 | LT-Router | Router 负载均衡（多 deployment 选择 + cooldown） | 多实例 upstream 需求 |
| P2 | LT-Usage | Usage 多视角聚合（Global/Team/Org/Key） | 前端用户反馈 |
| P3 | LT-Native | Anthropic 原生上游适配 | 需直接调 Anthropic Messages API |

---

## Phase 18: Spend Logs & Usage 质量修复（P0，已完成 ✅）

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 53 | 时间过滤 + Usage 当天数据修复 — 前端 `presetRange()` 改用 `toISOString()` 发送 UTC 时间戳；后端 `query_activity_*` 两处 `WHERE` 比较改为 `date(start_time) >= date(?)`；`normalize_date_for_query()` 防御层 | ✅ 完成 (2026-07-15) |
| Stage 54 | end_user 提取 + 复制按钮反馈 — 从 metadata.user_id 提取 end_user/session_id；X-Forwarded-For → requester_ip_address；流式 request_id 去掉 req_ 前缀；useCopyToClipboard hook | ✅ 完成 (2026-07-15) |

**设计文档**: `docs/14-spend-logs-usage-bugs.md`

---

## Phase 19: UI Enhancement — Models CRUD + Spend Logs 可视化（规划中）

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 55 | Models 管理页面完整 CRUD 前端 — 结构化表单新增/编辑/删除 + BDD | ⏳ 待开始 |
| Stage 56 | Spend Logs Prompt/Response 结构化可视化 — MessageViewer + ResponseViewer + Tab 切换 | ⏳ 待开始 |

**设计文档**: `docs/plans/2026-07-15-phase-19-20-roadmap.md`
**Stage 文档**: `docs/stages/stage-55.md`, `docs/stages/stage-56.md`

---

## Phase 20: Spend Logs 可观测性 — 过滤器增强 + Overhead 评估 + 修复（规划中）

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 57 | 下拉过滤器 + model_group 修复 + UA/device_id — model/session 下拉 + metadata.user_agent + metadata.device_id + distinct API | ⏳ 待开始 |
| Stage 58 | Gateway Overhead 评估 — proxy_server_request 入口快照 + queue_time + upstream_timing + overhead 可视化 | ⏳ 待开始 |

**设计文档**: `docs/plans/2026-07-15-phase-19-20-roadmap.md`
**Stage 文档**: `docs/stages/stage-57.md`, `docs/stages/stage-58.md`

---

## 后续路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-Router | Router 负载均衡（多 deployment 选择 + cooldown + fallback） | P2 | 多实例 upstream 需求 |
| LT-Usage | Usage 多视角聚合（Global/Team/Org/Key 下拉框 + 饼图联动） | P2 | 前端用户反馈 |
| LT-Native | Anthropic 原生上游适配（OpenAIToAnthropic + AnthropicPassthrough） | P3 | 需直接调 Anthropic Messages API |
| LT-Redis | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-Observ | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-SSO | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-PG | PostgreSQL 生产级支持 | P2 | 多实例 + 高可用 |
| LT-K8s | Kubernetes Operator | P3 | 云原生需求 |

---

## 技术债

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001~007 | ✅ | 已解决 |

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | `/v1/messages` 接口审计 — 7 bugs（2 CRITICAL） | 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 — Phase 17 替换为 ModelResolver + MessageAdapter | 2026-07-14 |
