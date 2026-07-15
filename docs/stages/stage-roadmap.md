# aigw — AI Gateway Stage Roadmap

**项目**: aigw (litellm Rust 最小兼容替代)
**最后更新**: 2026-07-15

---

## 当前状态

- **当前 Phase**: Phase 18 — Spend Logs & Usage 质量修复（P0）
- **状态**: 52/52 Stages 已完成，Phase 18 规划完成
- **下一里程碑**: Phase 18 Stage 53（Spend Logs 时间过滤修复 + Usage 当天数据修复）

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
Phase 14:   ████████████████████ 100% (4/4 Stages)
Phase 15:   ████████████████████ 100% (3/3 Stages)
Phase 16:   ████████████████████ 100% (3/3 Stages)
Phase 17:   ████████████████████ 100% (3/3 Stages)  ✅
Phase 18:   ░░░░░░░░░░░░░░░░░░░░   0% (0/2 Stages)  🔄 Spend Logs & Usage 质量修复
```

---

## 当前 Phase 详情

### Phase 14：`/v1/messages` 接口修复 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 40 | ✅ 完成 | 复用 `resolve_upstream_params` + Key 校验对齐 | 2026-07-11 |
| Stage 41 | ✅ 完成 | 流式 SSE 格式转换（OpenAI→Anthropic） | 2026-07-11 |
| Stage 42 | ✅ 完成 | SpendLog api_key/user_id 修复 + 错误码修正 | 2026-07-11 |
| Stage 43 | ✅ 完成 | stream_options include_usage + 流式 token 计数 | 2026-07-11 |

参见 `docs/plans/2026-07-11-v1-messages-fix-plan.md`

### Phase 15：第二轮反馈改进 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 44 | ✅ 完成 | Models 页面 Cost 列 | 2026-07-11 |
| Stage 45 | ✅ 完成 | Spend Logs 抽屉完整内容 + CSV 导出 + 布局优化 | 2026-07-11 |
| Stage 46 | ✅ 完成 | aigw-migrate --skip-columns / --skip-body 选择性迁移 | 2026-07-11 |

参见 `docs/plans/2026-07-10-phase-14-feedback-round-2.md`

### Phase 16：Playground 增强 ✅ 已完成

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 47 | ✅ 完成 | Playground Virtual Key 配置 + Endpoint Type 选择 | 2026-07-11 |
| Stage 48 | ✅ 完成 | Playground Clear Session + Get Code（curl/SDK） | 2026-07-11 |
| Stage 49 | ✅ 完成 | Playground Markdown 渲染 + 气泡边框 + 底部统计栏 | 2026-07-11 |

参见 `docs/plans/2026-07-11-phase-16-playground-enhancement.md`

### Phase 17：代理转发架构重构（P1）

> **背景**: `chat.rs` 和 `v1_messages.rs` 各自独立 resolve upstream，逻辑重复 ~230 行；`DefaultAdapter` 写死单一实现；`provider_registry`/`router_state` 在 `AppState` 中定义但从未使用。需要先重构架构再继续功能增强。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 50 | ✅ 完成 | **ModelResolver + Deployment** — 新建 `deployment.rs` + `resolver.rs`，迁移 `resolve_upstream_params` 为 `ModelResolver::resolve() → Vec<Deployment>`，替换 chat.rs 调用点。TDD: UT 覆盖查表/解密/credential/env fallback。门禁：全量 BDD 回归通过 | 后端+测试 | 4h |
| Stage 51 | ✅ 完成 | **MessageAdapter + tool 转换** — 拆分 adapter trait 为 `MessageAdapter` + `StreamAdapter`，实现 `OpenAIPassthrough` + `AnthropicToOpenAI`（含 tool_use/tool_result ↔ tool_calls 双向转换），新增 `select_adapter()`。TDD: UT 覆盖 4 种转换方向 + 流式 tool chunk。BDD: /v1/messages 含 tool_use 场景 | 后端+测试 | 5h |
| Stage 52 | ✅ 完成 | **Handler 瘦身** — chat.rs / v1_messages.rs 通用逻辑下沉，handler 只做：校验→resolve→adapt→upstream call→spend log。清理死代码。门禁：全量 UT+BDD+前端测试回归 | 后端+测试 | 3h |

**依赖关系**: Stage 50 → 51 → 52（串行，渐进式重构）。预估 12h。

**TDD 要求**: 每个 Stage 先写测试（UT + BDD scenario），RED → GREEN → REFACTOR 循环，测试全部通过后才可 commit。

**设计文档**: `docs/plans/2026-07-13-arch-refactor-plan.md`

**新增核心组件**:

| 组件 | 命名 | 职责 |
|------|------|------|
| 模型解析层 | `ModelResolver` | model_name → `Vec<Deployment>`（查 proxy_models、解密、解析 credential、提取定价） |
| 消息格式转换 | `MessageAdapter` trait | OpenAI Chat ↔ Anthropic Messages 双向转换（含 tool_use/tool_result ↔ tool_calls） |
| 上游配置 | `Deployment` | 纯值对象：api_base / api_key / upstream_model / provider_type / 定价 / raw_params（解密后完整 litellm_params） |
| 流式转换器 | `StreamAdapter` trait | SSE chunk 逐块转换（`&mut self` 维护跨 chunk 状态如 tool_use index） |

### Phase 18：Spend Logs & Usage 质量修复（P0）

> **背景**: Spend Logs 页面时间过滤器和 Usage 页面有 4 个已确认的 bug（详见 `docs/14-spend-logs-usage-bugs.md`），影响数据正确性和用户体验。依赖 Phase 17 Handler 瘦身完成后执行。

| Stage | 状态 | 目标 | 类型 | 预估 |
|-------|------|------|------|------|
| Stage 53 | ⏳ 待开始 | **时间过滤 + Usage 当天数据修复** — 前端 `spend-logs/index.tsx` `presetRange()` 改用 `toISOString()` 发送 UTC 时间戳；`usage/index.tsx` `presetRange()` 用 UTC 日期；后端 `db.rs` `query_activity_*` 两处 `WHERE start_time` 比较改为 `date(start_time) >= date(?) AND date(start_time) <= date(?)` 解决纯日期截断；后端 `spend.rs` 新增 `normalize_date_for_query()` 防御不同前端日期格式。TDD: UT 覆盖 UTC 解析/date 比较/边界场景 + BDD 覆盖 15min/4h 预设过滤 + Usage 当天数据显示 | 前后端+测试 | 6h |
| Stage 54 | ⏳ 待开始 | **end_user 提取 + 复制按钮反馈** — `v1_messages.rs` 和 `chat.rs` handler 中从请求体 `metadata.user_id` 提取 end_user，写入 SpendLog；可选解析 JSON 拆出 `session_id`；从 `X-Forwarded-For` 提取 `requester_ip_address`；修复 `v1_messages.rs:455-460` 流式路径 SpendLog 误用 `req_` 前缀改为纯 UUID；新建 `useCopyToClipboard` hook 替换 3 个页面的 `copyToClipboard()`。TDD: UT 覆盖 metadata.user_id 提取/String vs JSON/无 metadata 场景 + BDD 覆盖复制按钮反馈 | 前后端+测试 | 5h |

**依赖关系**: Stage 53 → 54 无硬依赖，可并行；依赖 Phase 17 完成。预估 11h。

**TDD 要求**: UT 先行（RED → GREEN → REFACTOR），BDD feature 补充验收。门禁：全量 UT+BDD+前端测试回归通过。

**设计文档**: `docs/14-spend-logs-usage-bugs.md`

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

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
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

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
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

| Stage | 状态 | 目标 | 完成日期 |
|-------|------|------|----------|
| Stage 34 | ✅ 完成 | SSE Streaming + completion_start_time + Spend Logs 增强（分页+request_id+TTFT） | 2026-07-10 |
| Stage 35 | ✅ 完成 | daily_spend 聚合表迁移 + 定时写入 | 2026-07-10 |
| Stage 36 | ✅ 完成 | 前端 Spend Logs 重构（Live Tail+时间预设+分页+细节抽屉） | 2026-07-10 |
| Stage 37 | ✅ 完成 | Users/Orgs 端到端修复 + Provider 解密 | 2026-07-10 |
| Stage 38 | ✅ 完成 | Usage 聚合端点 + 前端 Global 视图重构 | 2026-07-10 |
| Stage 39 | ✅ 完成 | Playground 聊天式多轮对话升级 | 2026-07-10 |

---

## 长期路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-Router | Router 负载均衡（多 deployment 选择 + cooldown + fallback） | P2 | 多实例 upstream 需求 |
| LT-Usage | Usage 多视角聚合（Global/Team/Org/Key 切换） | P2 | 前端用户反馈 |
| LT-Native | Anthropic 原生上游适配（OpenAIToAnthropic + AnthropicPassthrough） | P3 | 需直接调 Anthropic Messages API |
| LT-Redis | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-Observ | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-SSO | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-PG | PostgreSQL 生产级支持 + 迁移工具 | P2 | 多实例 + 高可用 |
| LT-K8s | Kubernetes Operator + Helm Chart | P3 | 云原生客户需求 |

### 状态图标说明

- ✅ 完成 - Stage 已完成所有验收标准
- 🔄 进行中 - Stage 正在开发中
- ⏳ 待开始 - Stage 尚未开始
- ❌ 已取消 - Stage 被取消

---

## 修订记录

| 版本 | 日期 | 修订内容 |
|------|------|----------|
| v1.0-v14.0 | 2026-07-03~11 | 初始版本 ~ Phase 17 规划 |
| v15.0 | 2026-07-14 | **架构重构规划**：修正 Phase 14-16 状态为已完成；移除旧 Stage 50-51（Usage 多视角聚合移入长期路线）；Phase 17 替换为代理转发架构重构（Stage 50-52: ModelResolver + MessageAdapter + Handler 瘦身）；每个 Stage 内置 TDD+BDD 测试；Stage 51 新增 tool_use/tool_calls 双向转换 |
| v16.0 | 2026-07-15 | **Spend Logs & Usage 质量修复规划**：Phase 17 Stage 50-52 已全部完成，状态更新为 ✅；新增 Phase 18（Stage 53: 时间过滤+Usage 当天数据修复，Stage 54: end_user 提取+复制按钮反馈），共 2 Stage，预估 11h |
