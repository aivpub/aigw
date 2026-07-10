# aigw -- 下一步行动

**上次更新**: 2026-07-11
**当前阶段**: Phase 15 反馈改进（P1）— Phase 14 已完成

---

## 当前状态：Phase 15 反馈改进

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
Phase 15:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  🔄 反馈改进（Stages 44-46）
Phase 16:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  ⏳  Playground 增强（Stages 47-49）
Phase 17:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4)  ⏳  Provider 适配架构（长期）
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端单元 | libtest | 316 tests |
| 后端 BDD | cucumber-rust | 92 scenarios (353 steps) |
| 前端 BDD | Playwright + playwright-bdd | 108 tests (36 scenarios × 3 viewports) |

---

## 优先级排序

| 优先级 | Phase | 目标 | 原因 |
|--------|-------|------|------|
| P0 | ~~Phase 14~~ ✅ | `/v1/messages` 接口修复 | 已完成 — 复用 resolve_upstream_params + SSE 格式转换 + 流式 token 计数 |
| **P1** | Phase 15 (Stages 44-46) | Models Cost + Spend Logs 抽屉/导出 + migrate features | 用户反馈需求 |
| P1 | Phase 16 (Stages 47-49) | Playground Virtual Key/Endpoint/GetCode/Markdown | Playground 交互增强 |
| P3 | Phase 17 (Stages 50-53) | Provider 适配架构 | 依赖明确的多厂商接入需求触发 |

---

## Phase 14: `/v1/messages` 修复 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 40 | ✅ | 复用 `resolve_upstream_params` + Key 校验对齐（budget check, token_hash + user_id 保存） | 2026-07-11 |
| Stage 41 | ✅ | 流式 SSE 格式转换（OpenAI → Anthropic）: buffer + \n\n 分割 + adapter 转换 + block boundary 注入 | 2026-07-11 |
| Stage 42 | ✅ | SpendLog api_key/user_id 修复 + 错误码修正（随 Stage 40 一并修复） | 2026-07-11 |
| Stage 43 | ✅ | stream_options include_usage + 流式 token 计数 + SSE 转换单元测试（4 tests） | 2026-07-11 |

**成果**: 77/77 测试通过。`/v1/messages` 现在：
- 复用 `chat::resolve_upstream_params()` 支持 proxy_models 表配置
- Key 预算校验（spend >= max_budget → 429）
- SSE streaming 完成 OpenAI → Anthropic 格式转换
- SpendLog 记录正确的 api_key hash + user_id
- 流式 token 计数从 upstream usage chunk 提取

---

## Phase 15: 反馈改进（P1，预估 10h）

| Stage | 目标 | 预估 |
|-------|------|------|
| Stage 44 | Models Cost 列 | 2h |
| Stage 45 | Spend Logs 抽屉 + 导出 CSV | 5h |
| Stage 46 | aigw-migrate --skip-columns | 3h |

---

## Phase 16: Playground 增强（P1，预估 10h）

| Stage | 目标 | 预估 |
|-------|------|------|
| Stage 47 | Virtual Key 配置 + Endpoint Type 选择 | 3h |
| Stage 48 | Clear Session + Get Code 弹窗 | 2h |
| Stage 49 | Markdown + 气泡边框 + 底部统计 | 5h |

---

## 后续路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| Phase 17 | Provider 适配架构 | P3 | 非 OpenAI 厂商接入需求 |
| LT-2 | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-3 | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-5 | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-6 | PostgreSQL 生产级支持 | P2 | 多实例 + 高可用 |
| LT-7 | Kubernetes Operator | P3 | 云原生需求 |

---

## 技术债

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001~006 | ✅ | 已解决 |
| TD-007 | 🆕 P0 | `/v1/messages` 7 bugs → Phase 14 修复中 |

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | `/v1/messages` 接口审计 — 7 bugs（2 CRITICAL） | 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |

## 当前审核中的文件（未提交）

等待审核后提交：

```
新增文件:
  docs/plans/2026-07-10-phase-14-feedback-round-2.md       (原 Phase 14 → 现 Phase 15 参考)
  docs/plans/2026-07-11-v1-messages-fix-plan.md             (Phase 14 修复方案)
  docs/plans/2026-07-11-phase-16-playground-enhancement.md  (Phase 16 需求概述)
  docs/stages/stage-40.md ~ stage-49.md                      (10 个 Stage 文档)

修改文件:
  docs/11-next-steps.md
  docs/stages/stage-roadmap.md
```
