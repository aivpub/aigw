# aigw -- 下一步行动

**上次更新**: 2026-07-22
**当前阶段**: Phase 27 全栈质量修复 + Usage 页面图表增强（Stages 69-71），Stage 69 ✅

---

## 当前状态：69/71 Stages 已完成

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅
Phase 5:    ████████████████████ 100% (6/6)  ✅
Phase 7:    ████████████████████ 100% (5/5)  ✅
Phase 8:    ████████████████████ 100% (3/3)  ✅
Phase 9:    ████████████████████ 100% (4/4)  ✅
Phase 11:   ████████████████████ 100% (6/6)  ✅
Phase 12:   ████████████████████ 100% (3/3)  ✅
Phase 13:   ████████████████████ 100% (6/6)  ✅
Phase 14:   ████████████████████ 100% (4/4)  ✅
Phase 15:   ████████████████████ 100% (3/3)  ✅
Phase 16:   ████████████████████ 100% (3/3)  ✅
Phase 17:   ████████████████████ 100% (3/3)  ✅
Phase 18:   ████████████████████ 100% (2/2)  ✅
Phase 19:   ████████████████████ 100% (2/2)  ✅
Phase 20:   ████████████████████ 100% (2/2)  ✅
Phase 21:   ████████████████████ 100% (2/2)  ✅
Phase 22:   ████████████████████ 100% (2/2)  ✅
Phase 23:   ████████████████████ 100% (2/2)  ✅
Phase 24:   ████████████████████ 100% (1/1)  ✅
Phase 25:   ████████████████████ 100% (1/1)  ✅
Phase 26:   ██████████████░░░░░░  50% (1/2)  🔄 OTEL ⏳
Phase 27:   ██████████░░░░░░░░░░  33% (1/3)  🔄 全栈质量修复 + Usage 图表增强
```

### 测试目标

| 层 | 框架 | 当前 |
|---|------|------|
| 后端单元 | libtest | ~322 tests |
| 后端 BDD | cucumber-rust | 101 scenarios |
| 前端 BDD | Playwright + playwright-bdd | 108 tests |

| 目标 | 当前 | +Stage 70/71 | 最终 |
|------|------|-----------|------|
| 后端 UT | ~322 | +1 → ~323 | ~323 |
| 后端 BDD | 101 | +0 → 101 | 101 |
| 前端 BDD | 108 | +36 → 144 | 144 |

---

## 优先级排序

| 优先级 | Phase | 目标 | 状态 |
|--------|-------|------|------|
| P0 | Phase 27 | 后端质量修复+数据增强 (Stage 69) | ✅ 完成 |
| P0 | Phase 27 | 前端页面修复 Models/Keys/Users/SpendLogs (Stage 70) | ⏳ |
| P1 | Phase 27 | Usage 图表增强 堆叠+排行榜 (Stage 71) | ⏳ |
| P2 | Phase 26 | OTEL Traces (Stage 68) | ⏳ |

---

## Phase 27: 全栈质量修复 + Usage 页面图表增强

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 69 | 后端质量修复 + Usage 数据增强（model_group/retry/IP/Daily/Keys） | 后端 | 8h | ✅ 2026-07-22 |
| Stage 70 | 前端页面修复（Models/Keys/Users/SpendLogs 表格补全） | 全栈 | 8h | ⏳ |
| Stage 71 | Usage 图表增强（堆叠 bar + Top Keys/Models 排行榜） | 前端 | 8h | ⏳ |

**合计**: 24h，3 Stages

**设计文档**: `docs/stages/stage-69.md` ~ `docs/stages/stage-71.md`

## 依赖关系

Stage 69（数据层 + 端点）→ Stage 70 / 71 可并行 ✅ Stage 69 已完成

## 后续路线

| ID | 主题 | 优先级 | 状态 |
|----|------|--------|------|
| LT-Observ | Observability (OTEL Traces) | P1 | Stage 68 待开始 |
| LT-Usage | Usage 多视角聚合 | P2 | 已消化 → Phase 27 |
| LT-Redis | Redis 缓存 | P2 | QPS > 1000 |
| LT-PG | PostgreSQL 生产级 | P2 | 多实例 + 高可用 |
| LT-SSO | SSO/OAuth | P3 | 企业客户需求 |
| LT-K8s | Kubernetes Operator | P3 | 云原生客户需求 |

> **已消化**: LT-Native → Phase 22, LT-Router → Phase 23, LT-Settings → Phase 24, LT-Usage → Phase 27

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | /v1/messages 接口审计 — 7 bugs（2 CRITICAL）| 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 | 2026-07-14 |
| ADR-016 | System Message Normalization (chat_template_compat) | 2026-07-16 |
| ADR-017 | model_group 语义对齐 litellm: model_name 而非 litellm_params.model | 2026-07-21 |
| ADR-018 | HTTP 层重试选用 reqwest-middleware + reqwest-retry, 单条 spend_logs 记录重试次数 | 2026-07-21 |
