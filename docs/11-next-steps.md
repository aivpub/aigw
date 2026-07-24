# aigw -- 下一步行动

**上次更新**: 2026-07-24
**当前阶段**: Phase 30 ⏳ Body Archive 冷存储

---

## 当前状态：77/81 Stages 已完成（Phase 30 待实施）

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
Phase 26:   ████████████████████ 100% (3/3)  ✅
Phase 27:   ████████████████████ 100% (3/3)  ✅ 全栈质量修复 + Usage 图表增强
Phase 28:   ████████████████████ 100% (1/1)  ✅ 安全与质量加固
Phase 29:   ████████████████████ 100% (4/4)  ✅ Cross-DB BDD Hardening
Phase 30:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4)  ⏳ Body Archive 冷存储
```

### 测试目标

| 层 | 框架 | 当前 |
|---|------|------|
| 后端单元 | libtest | ~322 tests |
| 后端 BDD | cucumber-rust | 101 scenarios |
| 前端 BDD | Playwright + playwright-bdd | 108 tests |

---

## 优先级排序

| 优先级 | Phase | 目标 | 状态 |
|--------|-------|------|------|
| P0 | Phase 30 | DB Schema + Core Archiver 写链路 (Stage 78) | ⏳ |
| P0 | Phase 30 | Query Router + Footer Cache 读链路 (Stage 79) | ⏳ |
| P0 | Phase 30 | Admin API + Col Chunk Cache + 存量归档 (Stage 80) | ⏳ |
| P0 | Phase 30 | 前端管理页面 (Stage 81) | ⏳ |

---

## Phase 30: Body Archive 冷存储 ⏳

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 78 | DB Schema + Core Archiver（Migration 020/021 + BodyArchiver + Parquet 写入 + S3 上传 + 清理器） | 后端 | 12h | ⏳ |
| Stage 79 | Query Router + Footer Cache（get_message_body 热/冷路由 + query_s3_with_cache + moka 内存 LRU） | 后端 | 10h | ⏳ |
| Stage 80 | Admin API + Col Chunk Cache + 存量归档（5 端点 + FS LFU 缓存 + bulk archive） | 后端 | 14h | ⏳ |
| Stage 81 | 前端管理页面（Status/Trigger/Job History/Detail 面板，4 BDD × 3 viewports） | 前端 | 10h | ⏳ |

**合计**: 46h，4 Stages

**依赖**: Stage 78 → 79 → 80 → 81（严格串行）

**设计文档**: `docs/stages/stage-78.md` ~ `docs/stages/stage-81.md` + `docs/plans/2026-07-22-body-archive-s3-parquet.md`

## 需求对齐总结

| 问题 | 决策 |
|------|------|
| 是否需要独立 CLI 批量归档存量数据？ | **不需要**，`POST /admin/archive/trigger` API 已支持任意日期范围批量归档（Stage 80） |
| 日 compaction 要纳入首批吗？ | **推迟到后续优化**，小时文件 2-40MB 可接受 |
| 监控指标要纳入首批吗？ | **推迟**，执行进度和错误记录在 `archive_job_logs` 表，可通过 API/前端查看 |
| 交付顺序？ | **写链路 → 读链路 → API → 前端**（严格串行，每 Stage 独立可测） |

## 后续路线

| ID | 主题 | 优先级 | 状态 |
|----|------|--------|------|
| LT-BodyCompact | Body Archive 日 compaction | P2 | 小时文件碎片过多时 |
| LT-BodyLifecycle | S3 生命周期自动删除 | P2 | 冷数据积累 > 100GB |
| LT-BodyMetrics | Body Archive 监控指标 | P2 | 生产运维需要 |
| LT-Redis | Redis 缓存 | P2 | QPS > 1000 |
| LT-PG | PostgreSQL 生产级 | P2 | 多实例 + 高可用 |
| LT-SSO | SSO/OAuth | P3 | 企业客户需求 |
| LT-K8s | Kubernetes Operator | P3 | 云原生客户需求 |

> **已消化**: LT-Native → Phase 22, LT-Router → Phase 23, LT-Settings → Phase 24, LT-Usage → Phase 27, LT-CrossDB → Phase 29, LT-BodyArchive → Phase 30

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | /v1/messages 接口审计 — 7 bugs（2 CRITICAL）| 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 | 2026-07-14 |
| ADR-016 | System Message Normalization (chat_template_compat) | 2026-07-16 |
| ADR-017 | model_group 语义对齐 litellm: model_name 而非 litellm_params.model | 2026-07-21 |
| ADR-018 | HTTP 层重试选用 reqwest-middleware + reqwest-retry, 单条 spend_logs 记录重试次数 | 2026-07-21 |
