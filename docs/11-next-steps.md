# aigw -- 下一步行动

**上次更新**: 2026-07-16
**当前阶段**: Phase 24 已完成！65/65 Stages

---

## 当前状态：65/65 Stages 已完成 ✅

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
Phase 19:   ████████████████████ 100% (2/2)  ✅ UI Enhancement（Stages 55-56）
Phase 20:   ████████████████████ 100% (2/2)  ✅ 可观测性增强（Stages 57-58）
Phase 21:   ████████████████████ 100% (2/2)  ✅ 协议兼容性修复（Stages 59-60）
Phase 22:   ████████████████████ 100% (2/2)  ✅ Anthropic 原生上游适配（Stages 61-62）
Phase 23:   ████████████████████ 100% (2/2)  ✅ Router 负载均衡（Stages 63-64）
Phase 24:   ████████████████████ 100% (1/1)  ✅ 管理控制台完善（Stage 65）
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端单元 | libtest | 293 tests |
| 后端 BDD | cucumber-rust | 93 scenarios (91 passed, 2 skipped) |
| 前端 BDD | Playwright + playwright-bdd | 108 tests (36 scenarios × 3 viewports) |

### 完成后目标

| 目标 | Phase 21 | +Phase 22 | +Phase 23 | 最终 |
|------|---------|-----------|-----------|------|
| 后端 UT | ~282 | ~292 | ~304 | ~304 |
| 后端 BDD | 93 | 97 | 102 | 102 |
| 前端 BDD | 111 (37×3) | 111 | 114 (38×3) | 114 |

---

## 优先级排序

| 优先级 | Phase | 目标 | 工时 |
|--------|-------|------|------|
| P0 | ~~Phase 21~~ ✅ | 协议兼容性修复（tool_result + system message） | 2026-07-16 |
| P1 | Phase 22 | Anthropic 原生上游适配 | 14h |
| P1 | Phase 23 | Router 负载均衡 + 三级配置 + 前端 | 16h |
| P1 | LT-Observ | Prometheus metrics | ~8h |

---

## Phase 21: 协议兼容性修复

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 59 | Multi tool_result Discard 修复 | ✅ 完成 |
| Stage 60 | System Message Normalization（全栈）| ✅ 完成 |

**设计文档**: `docs/plans/2026-07-16-phase-21-23-roadmap.md`
**Stage 文档**: `docs/stages/stage-59.md`, `docs/stages/stage-60.md`

---

## Phase 22: Anthropic 原生上游适配

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 61 | AnthropicPassthrough + OpenAIToAnthropic | ✅ **完成 (2026-07-16)** |
| Stage 62 | select_adapter 扩展 + Handler 对接 + 全量回归 | ✅ **完成 (2026-07-16)** |

**Stage 文档**: `docs/stages/stage-61.md`, `docs/stages/stage-62.md`

---

## Phase 23: Router 负载均衡

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 63 | Schema 修复 + Router Core | ✅ **完成 (2026-07-16)** |
| Stage 64 | 三级 router_settings + API + 前端 | ✅ **完成 (2026-07-16)** |

**Stage 文档**: `docs/stages/stage-63.md`, `docs/stages/stage-64.md`

---

## Phase 24: 管理控制台完善

| Stage | 目标 | 状态 |
|-------|------|------|
| Stage 65 | SETTINGS 分组 + Router 三 Tab + Models 多 Tab + Credential 前端 + Health Tab | ✅ **完成 (2026-07-16)** |

**Stage 文档**: `docs/stages/stage-65.md`

---

## 依赖关系

```
Phase 21: Stage 59 ∥ 60 (并行)
Phase 22: Stage 61 → 62 (串行)
Phase 23: Stage 63 → 64 (串行)
Phase 24: Stage 65 (独立，前端为主)
```

---

## 后续路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-Observ | Observability (Prometheus metrics) | P1 | Phase 24 完成后启动 |
| LT-Usage | Usage 多视角聚合 | P2 | 前端用户反馈 |
| LT-Redis | Redis 缓存 | P2 | QPS > 1000 |
| LT-PG | PostgreSQL 生产级 | P2 | 多实例 + 高可用 |
| LT-SSO | SSO/OAuth | P3 | 企业客户需求 |
| LT-K8s | Kubernetes Operator | P3 | 云原生客户需求 |

> **已消化**: LT-Native (Phase 22), LT-Router (Phase 23), LT-Settings (Phase 24)

---

## 技术债

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001~007 | ✅ | 已解决 |

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | `/v1/messages` 接口审计 — 7 bugs（2 CRITICAL）| 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 — Phase 17 ModelResolver + MessageAdapter | 2026-07-14 |
| ADR-016 | Anthropic→OpenAI 多 system 消息归一化（chat_template_compat）| 2026-07-16 |
