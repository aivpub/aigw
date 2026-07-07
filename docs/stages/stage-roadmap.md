# aigw — AI Gateway Stage Roadmap

**项目**: aigw (litellm Rust 最小兼容替代)
**最后更新**: 2026-07-07 (Phase 7 Stage 13-16 已完成，Stage 17 80%)

---

## 当前状态

- **当前 Stage**: Phase 7 Stage 17 收尾中
- **状态**: Phase 0-5 全部完成（12/12），Phase 7 Stage 13-16 已完成（4/5）
- **下一里程碑**: Stage 17 收尾 — 自动化预检/监控工具

### 整体进度

```
Phase 0-4: ████████████████████ 100% (6/6 Stages)
Phase 5:   ████████████████████ 100% (6/6 Stages)
Phase 7:   ███████████████████   96% (4.8/5 Stages)
```

---

## Stage 路线图

### Phase 0：项目基础设施

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 0 | ✅ 完成 | RDD 初始化、章程编写、代码基线建立、表名决策、双向迁移策略 | 2026-07-03 |

### Phase 1：数据兼容（核心基础）

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 1 | ✅ 完成 | Schema 100% 对齐（11 张表，SQLite/MySQL/PostgreSQL）+ aigw-migrate 双向迁移工具 | 2026-07-03 |
| Stage 2 | ✅ 完成 | Key API CRUD + SpendLog 读写 + /spend/* 端点 | 2026-07-03 |

### Phase 2：功能对等

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 3 | ✅ 完成 | Chat Completions /v1/chat/completions + /v1/models + Router + Budget/Rate Limit | 2026-07-03 |

### Phase 3：接口规范化

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 4 | ✅ 完成 | OpenAPI 3.1 规范 + Swagger UI + 前端控制台技术选型与规划 | 2026-07-03 |

### Phase 4：部署就绪

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 5 | ✅ 完成 | Docker 化 + Docker Compose + 自托管部署文档 | 2026-07-03 |
| Stage 6 | ✅ 完成 | 云服务 SaaS 架构支持（鉴权网关 + 多实例 + 数据隔离） | 2026-07-03 |

### Phase 5：最小化后端完整版 + BDD 测试（RGR 驱动）

| Stage | 状态 | 目标 | 预估 |
|-------|------|------|------|
| Stage 7 | ✅ 完成 | BDD 框架搭建 + 既有功能 .feature — cucumber-rust 集成、keys/spend/health .feature、RGR 循环验证 | 2026-07-04 |
| Stage 8 | ✅ 完成 | 模型管理 CRUD（BDD 驱动）— `proxy_models` 表（litellm v1.90.3 兼容）、`/model/*` 端点、`/v1/models` 动态列表 | 2026-07-04 |
| Stage 9 | ✅ 完成 | Provider 适配转换层（BDD 驱动）— `ProviderAdapter` trait、OpenAI↔Claude 双向转换（4 种组合） | 2026-07-04 |
| Stage 10 | ✅ 完成 | Claude /v1/messages 端点 + SSE Streaming（BDD 驱动）— `/v1/messages` 端点、4 种流式组合、SSE chunk 转换 | 2026-07-04 |
| Stage 11 | ✅ 完成 | Usage 用量查询增强（BDD 驱动）— `/spend/models` `/global/spend/models` `/spend/providers` 聚合、`/spend/logs` 过滤增强 | 2026-07-04 |
| Stage 12 | ✅ 完成 | BDD 全量覆盖 + 集成测试体系 — 端到端 .feature、Mock 上游、CI 集成、BDD 指南 | 2026-07-05 |

**Stage 7-12 依赖关系**:

```
Stage 7 (BDD 框架搭建 + 既有功能 .feature)
  ├── Stage 8 (模型管理 CRUD BDD)
  │     └── Stage 9 (Provider 适配层 BDD)
  │           └── Stage 10 (Claude /v1/messages + SSE BDD)
  ├── Stage 11 (Usage 用量查询增强 BDD)
  └── Stage 12 (BDD 全量覆盖 + 集成测试收尾)
```

Stage 7 优先完成（BDD 基础设施），后续 Stage 8-12 全部使用 RGR 循环驱动开发。Stage 11 可与 8-10 并行。Stage 12 为收尾。

### Phase 7：生产 litellm 迁移到 aigw

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 13 | ✅ 完成 | credentials 表 + CredentialsStore + 全量 Store PG/MySQL 补齐 | 2026-07-06 |
| Stage 14 | ✅ 完成 | NaCl 加密/解密 Rust 库 + aigw-migrate PostgreSQL 源 + master_key 提取 | 2026-07-06 |
| Stage 15 | ✅ 完成 | aigw-migrate 全量迁移（解密 litellm → 重加密 aigw）+ 端到端验证 | 2026-07-06 |
| Stage 16 | ✅ 完成 | aigw 运行时解密 + 凭证引用解析（litellm_credential_name） | 2026-07-07 |
| Stage 17 | 🔄 进行中 | 生产迁移 SOP + export 增强 + 回滚方案 | — |

**Stage 17 进度**: SOP 文档 (372行) ✅、remote-export 回滚工具 ✅、BDD 迁移测试 ✅；待完成：自动化预检工具、一键回滚脚本。

**Stage 13-17 依赖关系**:

```
Stage 13 (credentials 表 + 全量 Store PG/MySQL)
  └── Stage 14 (NaCl 加解密 + aigw-migrate PG 源)
        └── Stage 15 (全量迁移：解密 litellm → 重加密 aigw)
              ├── Stage 16 (aigw 运行时解密 + 凭证引用)
              └── Stage 17 (生产 SOP + 回滚)
```

纯 DB 迁移方案：NaCl SecretBox 解密 litellm 加密字段 → 用 aigw master_key 重加密 → 写入 aigw DB。详见 `docs/stages/stage-13.md` ~ `docs/stages/stage-17.md`。

### Phase 6：长期路线（Stage 17+ 后续）

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-1 | 多租户管理 API (/org/*, /team/*, /user/* CRUD) | P1 | 有自托管客户需要 Web UI 管理团队 |
| LT-2 | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-3 | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-4 | 前端管理控制台完整实现 | P1 | Stage 4 完成后 |
| LT-5 | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-6 | PostgreSQL 生产级支持 + 迁移工具 | P2 | 多实例 + 高可用 |
| LT-7 | Kubernetes Operator + Helm Chart | P3 | 云原生客户需求 |

### 状态图标说明

- ✅ 完成 - Stage 已完成所有验收标准
- 🔄 进行中 - Stage 正在开发中
- ⏳ 待开始 - Stage 尚未开始
- ❌ 已取消 - Stage 被取消

---

## 修订记录

| 版本 | 日期 | 修订内容 | 修订人 |
|------|------|----------|--------|
| v1.0 | 2026-07-03 | 初始版本，7 Stage + 7 长期路线 | 全栈架构师 |
| v1.1 | 2026-07-03 | Stage 0 标记完成，表名/迁移工具描述对齐 | 全栈架构师 |
| v2.0 | 2026-07-03 | Stage 1-6 全部完成，标记所有 Stage 为 Complete | 全栈架构师 |
| v3.0 | 2026-07-04 | 新增 Phase 5（Stage 7-11）：模型管理 CRUD、BDD 测试体系、SSE Streaming + OpenAI Provider、Claude 适配转换；调整长期路线为 Phase 6 | 全栈架构师 |
| v3.1 | 2026-07-04 | Phase 5 拆分为 6 个 Stage（7-12），BDD 提前到 Stage 7，RGR 驱动后续所有 Stage；每个 Stage 独立文档 | 全栈架构师 |
| v3.2 | 2026-07-04 | Stage 11 缩减：删除 `/v1/key/*` 别名端点（litellm 源码核实无此路由），仅保留 Usage 用量增强；工时 4-6h → 2-3h | 全栈架构师 |
| v4.0 | 2026-07-05 | Phase 5 全部完成：Stage 7-12 标记 Complete，BDD 63 场景通过，9 @real_api 场景创建 | 全栈架构师 |
| v5.0 | 2026-07-06 | 新增 Phase 7（Stage 13-17）：生产 litellm 迁移到 aigw，纯 DB 迁移 + NaCl 解密方案 | 全栈架构师 |
| v5.1 | 2026-07-07 | 更新 Phase 7 实际状态：Stage 13-16 已完成，Stage 17 80%（SOP/export/BDD 已完成，缺自动化预检/监控） | Claude Code |
