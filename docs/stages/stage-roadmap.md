# aigw — AI Gateway Stage Roadmap

**项目**: aigw (litellm Rust 最小兼容替代)
**最后更新**: 2026-07-11

---

## 当前状态

- **当前 Phase**: Phase 14 — `/v1/messages` 接口修复（最高优先级）
- **状态**: 39/39 Stages 已完成，Phase 14-17 规划完成
- **下一里程碑**: Phase 14 Stage 40-43（Claude SDK 兼容性修复）

### 整体进度

```
Phase 0-4:  ████████████████████ 100% (6/6 Stages)
Phase 5:    ████████████████████ 100% (6/6 Stages)
Phase 7:    ████████████████████ 100% (5/5 Stages)
Phase 8:    ████████████████████ 100% (3/3 Stages)
Phase 9:    ████████████████████ 100% (4/4 Stages)
Phase 11:   ████████████████████ 100% (6/6 Stages)
Phase 12:   ████████████████████ 100% (3/3 Stages)
Phase 13:   ████████████████████ 100% (6/6 Stages)
Phase 14:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4 Stages)  ⚠️ /v1/messages 修复
Phase 15:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3 Stages)  反馈改进
Phase 16:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3 Stages)  Playground 增强
Phase 17:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4 Stages)  Provider 适配架构（长期）
```

---

## 当前 Phase 详情

### Phase 14：`/v1/messages` 接口修复（最高优先级 P0）

> **触发**: Claude Code 实测 `/v1/messages` 报错。审计发现 7 个 bug，其中 2 个 Critical。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 40 | ⏳ 待开始 | `/v1/messages` 复用 `resolve_upstream_params` + Key 校验对齐 | 后端 | 1.5h |
| Stage 41 | ⏳ 待开始 | `/v1/messages` 流式 SSE 格式转换（OpenAI→Anthropic） | 后端 | 2.5h |
| Stage 42 | ⏳ 待开始 | `/v1/messages` SpendLog 修复（api_key/user_id）+ 错误码修正 | 后端 | 0.5h |
| Stage 43 | ⏳ 待开始 | `/v1/messages` 流式 Token 计数 + BDD/手工测试 | 后端+测试 | 1.5h |

**依赖关系**: Stage 40 → 41 → 42 → 43（串行，渐进式交付）。预估 6h。

参见 `docs/plans/2026-07-11-v1-messages-fix-plan.md`

### Phase 15：第二轮反馈改进（P1）

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 44 | ⏳ 待开始 | Models 页面 Cost 列（Input/Output Per Million Tokens） | 前端 | 2h |
| Stage 45 | ⏳ 待开始 | Spend Logs 抽屉完整内容（messages/response/params/tools）+ CSV 导出 + 布局优化 | 前后端 | 5h |
| Stage 46 | ⏳ 待开始 | aigw-migrate --skip-columns / --skip-body 选择性迁移 | 后端 | 3h |

**依赖关系**: 全部独立。预估 10h。

参见 `docs/plans/2026-07-10-phase-14-feedback-round-2.md`（注意原 Phase 14 已重编号为 Phase 15）

### Phase 16：Playground 增强（P1）

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 47 | ⏳ 待开始 | Playground Virtual Key 配置 + Endpoint Type 选择 | 前端 | 3h |
| Stage 48 | ⏳ 待开始 | Playground 按钮组：Clean Session + Get Code（curl/OpenAI SDK/Enio） | 前端 | 2h |
| Stage 49 | ⏳ 待开始 | Playground Markdown 渲染 + 消息气泡边框 + 底部统计栏（原 Stage 41） | 前端 | 5h |

**依赖关系**: Stage 47 → 48；Stage 49 独立。预估 10h。

参见 `docs/plans/2026-07-11-phase-16-playground-enhancement.md`

### Phase 17：Provider 适配架构（P3，长期）

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 50 | ⏳ 待开始 | Provider 协议类型枚举 + Adapter Registry + ProviderAdapter trait 泛化 | 后端 | 4h |
| Stage 51 | ⏳ 待开始 | chat.rs / v1_messages.rs 协议感知路由（model → protocol → adapter dispatch） | 后端 | 3h |
| Stage 52 | ⏳ 待开始 | `proxy_models` 表增加 `protocol` 列迁移 + Provider UI 管理 | 前后端 | 3h |
| Stage 53 | ⏳ 待开始 | Gemini Adapter 实现（验证架构可扩展性） | 后端 | 4h |

**依赖关系**: Stage 50 → 51 → 52 → 53。**触发条件**: 有明确的非 OpenAI 厂商接入需求时启动。

---

## 已完成 Phase 回顾

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
| Stage 7 | ✅ 完成 | BDD 框架搭建 + 既有功能 .feature | 2026-07-04 |
| Stage 8 | ✅ 完成 | 模型管理 CRUD（BDD 驱动） | 2026-07-04 |
| Stage 9 | ✅ 完成 | Provider 适配转换层（BDD 驱动） | 2026-07-04 |
| Stage 10 | ✅ 完成 | Claude /v1/messages 端点 + SSE Streaming（BDD 驱动） | 2026-07-04 |
| Stage 11 | ✅ 完成 | Usage 用量查询增强（BDD 驱动） | 2026-07-04 |
| Stage 12 | ✅ 完成 | BDD 全量覆盖 + 集成测试体系 | 2026-07-05 |

### Phase 7：生产 litellm 迁移到 aigw

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 13 | ✅ 完成 | credentials 表 + CredentialsStore + 全量 Store PG/MySQL 补齐 | 2026-07-06 |
| Stage 14 | ✅ 完成 | NaCl 加密/解密 Rust 库 + aigw-migrate PostgreSQL 源 + master_key 提取 | 2026-07-06 |
| Stage 15 | ✅ 完成 | aigw-migrate 全量迁移（解密 litellm → 重加密 aigw）+ 端到端验证 | 2026-07-06 |
| Stage 16 | ✅ 完成 | aigw 运行时解密 + 凭证引用解析（litellm_credential_name） | 2026-07-07 |
| Stage 17 | ✅ 完成 | 生产迁移 SOP + pre-check 预检 + rollback.sh 回滚脚本 | 2026-07-08 |

### Phase 8：生产化基础

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 18 | ✅ 完成 | 结构化日志 — tracing + tracing-subscriber + JSON 格式 + request_id | 2026-07-08 |
| Stage 19 | ✅ 完成 | 多租户管理 API — /org/* /team/* /user/* CRUD（15 端点，BDD 驱动） | 2026-07-08 |
| Stage 20 | ✅ 完成 | 健康检查增强 — /health/metrics（DB 连接池、uptime、key/model 计数） | 2026-07-08 |

### Phase 9：前端管理控制台

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 21 | ✅ 完成 | 前端工程搭建 — Vite + React + shadcn/ui + rust-embed 集成 | 2026-07-08 |
| Stage 22 | ✅ 完成 | Key 管理页面 — 列表/搜索/创建/编辑/删除/复制 API key | 2026-07-08 |
| Stage 23 | ✅ 完成 | 用量 Dashboard — 支出卡片 + 图表 + spend logs 表格 + 日期筛选 | 2026-07-08 |
| Stage 24 | ✅ 完成 | Model 管理页面 — proxy_models 列表 + 详情展开 | 2026-07-08 |

### Phase 11：前端质量加固 + 安全达标

| Stage | 状态 | 目标 | 预估 |
|-------|------|------|------|
| Stage 25 | ✅ 完成 | 前端 BDD 测试基础设施 — Playwright + Gherkin + 截图/GIF + Mock API | 2026-07-08 |
| Stage 26 | ✅ 完成 | 登录安全对齐 Litellm — `/v2/login` JWT + Cookie + scrypt + 数据库用户认证 | 2026-07-08 |
| Stage 27 | ✅ 完成 | 移动端适配 — 全页面响应式改造 | 2026-07-08 |
| Stage 28 | ✅ 完成 | Key 创建 UX 修复 — Token 展示对话框 + 复制确认 + 一次性提示 | 2026-07-08 |
| Stage 29 | ✅ 完成 | 用户/组织/团队管理前端页面 | 2026-07-08 |
| Stage 30 | ✅ 完成 | Dashboard 数据接入 + 移动端图表 | 2026-07-08 |

### Phase 12：前端导航重构 + Playground（对齐 litellm）

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 31 | ✅ 完成 | 侧边栏分组重构 + Usage 重命名 — litellm 5 组结构 | 2026-07-08 |
| Stage 32 | ✅ 完成 | Spend Logs 独立页面 — 日期筛选 + 移动端 card list + 30s 自动刷新 | 2026-07-09 |
| Stage 33 | ✅ 完成 | Playground Chat 调试页 — 模型选择 + System/User 消息 + Temperature/MaxTokens + Streaming + SSE mock | 2026-07-09 |

### Phase 13：前端反馈改进 + SSE Streaming + TTFT

| Stage | 状态 | 目标 | 日期 |
|-------|------|------|------|
| Stage 34 | ✅ 完成 | SSE Streaming + completion_start_time + Spend Logs 增强（分页+request_id+TTFT） | 2026-07-10 |
| Stage 35 | ✅ 完成 | daily_spend 聚合表迁移 + 定时写入 | 2026-07-10 |
| Stage 36 | ✅ 完成 | 前端 Spend Logs 重构（Live Tail+时间预设+分页+细节抽屉） | 2026-07-10 |
| Stage 37 | ✅ 完成 | Users/Orgs 端到端修复 + Provider 解密 | 2026-07-10 |
| Stage 38 | ✅ 完成 | Usage 聚合端点 + 前端 Global 视图重构 | 2026-07-10 |
| Stage 39 | ✅ 完成 | Playground 聊天式多轮对话升级 | 2026-07-10 |

---

## 长期路线（Phase 10 + Phase 17）

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| Phase 17 | Provider 适配架构（Protocol Registry + Gemini Adapter） | P3 | 非 OpenAI 厂商接入需求 |
| LT-2 | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-3 | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
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

| 版本 | 日期 | 修订内容 |
|------|------|----------|
| v1.0-v12.0 | 2026-07-03~10 | 初始版本 ~ Phase 13 完成 |
| v13.0 | 2026-07-10 | 新增 Phase 14（Stage 40-43）原始反馈需求 |
| v14.0 | 2026-07-11 | **重构编号**：Phase 14 重排为 `/v1/messages` 修复（最高优先级）；原反馈需求移入 Phase 15；新增 Phase 16 Playground 增强；新增 Phase 17 Provider 适配架构 |
