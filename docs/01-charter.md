# 项目章程：aigw — AI Gateway (litellm Rust 最小兼容替代)

**项目**: aigw
**仓库**: github.com/aivpub/aigw
**创建日期**: 2026-07-03
**章程版本**: v2.0

---

## 1. 项目愿景

构建一个 **litellm proxy 的 Rust 最小兼容替代品**，在保持与 litellm 数据格式、API 接口和部署模式高度兼容的前提下，提供更低的资源消耗、更高的吞吐性能，并同时支持**云服务（SaaS）**和**企业自托管（On-Prem）**两种部署形态。最终成为 LLM API Gateway 领域的生产级 Rust 方案。

---

## 2. 项目目标

### 业务目标

| 编号 | 目标描述 | 衡量标准 | 目标值 | 截止时间 |
|------|----------|----------|--------|----------|
| G1 | 实现对 litellm v1.90.0 核心代理功能的 schema 级数据兼容 | 从 litellm SQLite DB 导出数据后可直接导入 aigw 启动 | 100% 字段覆盖 | Stage 2 |
| G2 | 支持 Claude Code / Codex 无缝切换连接 | 两个客户端全流程无报错 | 功能对等 | Stage 3 |
| G3 | 提供 OpenAPI 3.1 规范定义 | openapi.yaml 通过 spectral lint | 核心端点 100% 覆盖 | Stage 4 |
| G4 | 支持云服务 SaaS 和企业自托管两种部署 | 同一二进制文件通过配置切换部署模式 | 两种模式均可成功启动 | Stage 6 |

### 技术目标

| 编号 | 目标描述 | 衡量标准 | 目标值 | 截止时间 |
|------|----------|----------|--------|----------|
| T1 | Schema 100% 对齐 litellm 核心表（aigw 自有表名，通过 aigw-migrate 双向迁移） | 逐列对比通过 | 55+24 列 | Stage 1 |
| T2 | API 格式兼容 /key/*, /spend/*, /global/spend/* | 与 litellm 返回 JSON 结构一致 | ~25 端点 | Stage 2-3 |
| T3 | 多租户数据模型最小化兼容（保留 Org/Team/User/Project/Budget 表及 FK） | 表存在且字段对齐，双向迁移验证通过 | 9 张多租户表 | Stage 1 |
| T4 | OpenAPI 规范 + 前端控制台技术选型与架构规划 | 评审通过 | — | Stage 4 |
| T5 | 资源消耗比 litellm Python 版显著降低 | 内存 < 50MB idle, CPU < 1% idle | — | Stage 3 |

---

## 3. 非目标（Non-Goals）——明确声明不做的事项

> 以下标记为"最小化兼容"或"长期路线"的条目 **不等于不处理**，详见第 6 节《边界与迁移路线》和第 8 节《长期工作路线》。

| 编号 | 非目标描述 | 阶段 | 原因 |
|------|-----------|------|------|
| NG1 | SSO / OAuth / JWT 登录鉴权 | 永久跳过 | Claude Code / Codex 使用 Virtual Key 鉴权，不需要用户登录 |
| NG2 | Guardrails / Policy Engine / 内容审核 | 长期路线 Phase 3 | 不是最小版本的交付范围 |
| NG3 | MCP Server 管理 | 长期路线 Phase 3 | 不是当前使用场景 |
| NG4 | 语义缓存（Redis 缓存层） | 长期路线 Phase 2 | 性能优化项，非功能必须 |
| NG5 | Adaptive Router（贝叶斯统计路由） | 长期路线 Phase 3 | 高级路由策略 |
| NG6 | Prometheus / OTEL metrics 导出 | 长期路线 Phase 2 | 运维增强项 |
| NG7 | WebSocket 实时端点 | 长期路线 Phase 3 | — |
| NG8 | Workflow Run 追踪 | 长期路线 Phase 3 | — |
| NG9 | 30+ Provider 特定 handler | 永久跳过 | 只做 OpenAI 兼容 upstream 通用接入 |

---

## 4. 成功标准

### 功能成功标准

| 编号 | 成功标准 | 验证方式 | 负责人 |
|------|----------|----------|--------|
| SC1 | litellm 现有 SQLite 数据库可直接作为 aigw 数据源启动 | smoke test: 导出→导入→启动→查询 | — |
| SC2 | Claude Code 配置 `OPENAI_BASE_URL` 指向 aigw 后完整跑通 conversation | 手动 E2E 测试 | — |
| SC3 | Codex 配置后全流程可用（model list → chat → streaming） | 手动 E2E 测试 | — |
| SC4 | Virtual Key CRUD 返回值格式与 litellm 一致 | 对比测试脚本 | — |
| SC5 | openapi.yaml 可通过 Swagger UI / scalar 渲染 | 本地启动验证 | — |
| SC6 | 云服务模式启动（鉴权网关前置）和自托管模式启动（直连）均可工作 | 配置切换验证 | — |

### 质量成功标准

| 指标 | 目标值 | 最低可接受值 | 验证方式 |
|------|--------|-------------|----------|
| 内存占用（idle） | < 30 MB | < 50 MB | `ps` 或 cgroup 监控 |
| 内存占用（100 QPS） | < 100 MB | < 200 MB | 压测工具 |
| P99 响应延迟（无 upstream） | < 5 ms | < 10 ms | 压测工具 |
| 启动时间 | < 1 s | < 3 s | `time` 命令 |

---

## 5. 核心技术边界

| 边界类型 | 边界定义 |
|----------|----------|
| 技术栈 | Rust + axum + sqlx + tokio |
| 数据存储 | SQLite（首选，兼容 litellm SQLite schema）+ PostgreSQL（可选） |
| 部署环境 | 单二进制文件，支持 Docker 容器化 |
| API 规范 | OpenAPI 3.1 |
| 网络协议 | HTTP/1.1 + SSE streaming |
| 鉴权方式 | Virtual Key（Bearer Token）+ Master Key |
| 上游协议 | OpenAI Chat Completions API 兼容 |
| 前端技术 | 待定（Stage 4 技术选型，候选：Next.js / Vue 3 + Vite） |

---

## 6. 边界与迁移路线

### 部署模式边界

| 模式 | 架构特征 | 阶段 |
|------|---------|------|
| **企业自托管 (On-Prem)** | 单二进制、SQLite、直连 upstream、Docker Compose 部署 | Stage 1-5 |
| **云服务 (SaaS)** | 前置 API Gateway（nginx / kong）、多租户数据隔离、按量计费、管理控制台 | Stage 6 |

### 迁移工具：`aigw-migrate`

aigw 代码库使用自己的表名（`virtual_keys`, `spend_logs`, `organizations` 等），与 litellm 表名彻底解耦。`aigw-migrate` 是唯一知道双向映射的组件，保证 litellm ↔ aigw 数据可迁移：

```
# 正向迁移：litellm → aigw
aigw-migrate import --from litellm --from-db litellm.db --to aigw --to-db aigw.db

# 逆向迁移：aigw → litellm（回滚用）
aigw-migrate export --from aigw --from-db aigw.db --to litellm --to-db litellm-restored.db
```

这是 Stage 1 的核心交付件之一，详见 `docs/litellm-diff-baseline.md` §5-6。

### 表名映射

| litellm 表名 | aigw 表名 | 策略 |
|-------------|----------|------|
| `LiteLLM_VerificationToken` | `virtual_keys` | ✅ 完整保留列 + FK |
| `LiteLLM_SpendLogs` | `spend_logs` | ✅ 完整保留列 |
| `LiteLLM_OrganizationTable` | `organizations` | ✅ 完整保留列 + FK |
| `LiteLLM_TeamTable` | `teams` | ✅ 完整保留列 + FK |
| `LiteLLM_UserTable` | `users` | ✅ 完整保留列 + FK |
| `LiteLLM_ProjectTable` | `projects` | ✅ 完整保留列 + FK |
| `LiteLLM_BudgetTable` | `budgets` | ✅ 完整保留列 + FK |
| `LiteLLM_OrganizationMembership` | `organization_memberships` | ✅ 保留 + FK |
| `LiteLLM_TeamMembership` | `team_memberships` | ✅ 保留 + FK |

### 多租户体系边界

**原则：最小化但不残缺。** litellm 的多租户层级为 `Organization → Team → User → Key` + `Project`，四层 FK 关联。

| 层级 | 最小化策略 | 理由 |
|------|----------|------|
| organizations | ✅ 完整保留（含 FK） | Key 有 `organization_id` FK，去掉会导致导入失败 |
| teams | ✅ 完整保留（含 FK） | Key 有 `team_id` FK，SpendLogs 有 `team_id` |
| users | ✅ 完整保留（含 FK） | Key 有 `user_id` FK，SpendLogs 有 `user` 列 |
| projects | ✅ 完整保留（含 FK） | Key 有 `project_id` FK |
| budgets | ✅ 保留 | Key/Team/User/Org 均有 `budget_id` FK |
| CRUD API（org/team/user）| ⚠️ 保留 `GET /info` 读取端点，跳过 `POST/PUT/DELETE` 写入端点 | 数据兼容需要读，最小化版本不管理 |
| 写入端点 | ⚠️ 通过 `/key/generate` 时支持 `team_id`/`user_id`/`org_id` 参数绑定已有实体 | 满足使用需求 |

---

## 7. Phase 划分与 Stage 规划

### Phase 0：项目基础设施

**Stage 0：项目初始化与 RDD 框架搭建**（当前阶段）

- RDD 框架初始化
- 项目章程（本文档）
- 从现有 `rust-proxy/` 提取可复用代码
- 确定 Rust 工程结构
- 与 litellm v1.90.0 仓库建立代码级 diff 基线

### Phase 1：数据兼容（核心基础）

**Stage 1：Schema 100% 对齐 + 双向迁移工具**

- 按 litellm schema.prisma 逐列对齐以下表（使用 aigw 自己的表名）：
  - `virtual_keys`（55 列，对应 `LiteLLM_VerificationToken`）
  - `spend_logs`（24 列，对应 `LiteLLM_SpendLogs`）
  - `organizations` / `teams` / `users` / `projects`（多租户核心）
  - `budgets` / `organization_memberships` / `team_memberships`
- 补齐所有 index
- SQLite migration 脚本
- **`aigw-migrate` 双向迁移工具**（import + export + verify）
- Smoke test：litellm DB → import → aigw 启动 → 运行 → export → litellm 验证（完整往返）

**Stage 2：Key 管理 API 格式对齐 + SpendLog 写入对齐**

- `/key/generate` 返回值字段补齐（user_id, team_id, org_id, budget_id, permissions 等）
- `/key/info`, `/key/update`, `/key/delete`, `/key/list` 返回格式对齐
- 支持 `key` 参数（自定义 key 值，关键迁移能力）
- 补齐 auto_rotate / rotation_interval / key_rotation_at 字段
- SpendLog 写入对齐：全部 24 列正确填充
- SpendLog 查询端点对齐：`/spend/logs`, `/spend/keys`, `/spend/users`, `/spend/tags`
- `/global/spend/*` 系列端点补齐
- 对比测试：同一请求 litellm vs aigw spendlog 行内容对比

### Phase 2：功能对等

**Stage 3：Claude Code / Codex 兼容 + 路由 + 限流**

- Claude Code 全流程验证（SSE streaming 精确兼容）
- Codex 全流程验证
- 错误传递行为对齐（finish_reason, HTTP status code）
- usage-based-routing-v2 + latency-based + shuffle 三种路由
- Cooldown + fallback 机制
- Budget reset 逻辑（budget_reset_at 检查）
- RPM/TPM 限流（内存计数器 + 时间窗口）
- max_parallel_requests（信号量/atomic 计数器）

### Phase 3：接口规范化

**Stage 4：OpenAPI 3.1 规范 + 前端控制台规划**

- 生成 OpenAPI 3.1 规范文件（`docs/openapi.yaml`）
  - 所有 `/v1/chat/completions`, `/v1/models` 端点
  - 所有 `/key/*` 管理端点
  - 所有 `/spend/*`, `/global/spend/*` 端点
  - 所有 `/health/*` 端点
- 使用 `spectral` lint 校验规范
- Swagger UI / Scalar 挂载到 `/docs` 端点
- 前端控制台技术选型评估（Next.js vs Vue 3 vs htmx）
- 前端控制台功能规划文档：
  - Dashboard 概览（总 spend、活跃 key、QPS）
  - Key 管理界面（创建/编辑/删除/搜索）
  - Spend 分析界面（by key/user/model 图表）
  - Model 配置界面（upstream 管理）
- 前端 repo 结构规划（独立 vs monorepo）

### Phase 4：部署就绪

**Stage 5：Docker 化 + 自托管部署文档**

- Dockerfile（multi-stage build）
- Docker Compose 编排
- 自托管部署文档（README DEPLOYMENT.md）
- 健康检查端点完善
- 优雅关闭 + 热重载配置

**Stage 6：云服务 SaaS 架构支持**

- 前置鉴权网关兼容（nginx/kong auth request 模式）
- 多实例部署配置
- 多租户数据隔离策略文档
- 按量计费数据导出接口规划

---

## 8. 长期工作路线（兼容性演进）

此路线规划了最小化版本**不包含**但**后续必须推进**的能力，确保最小化版本不会变成"永远不兼容"的死胡同。

| 路线阶段 | 主题 | 内容 | 触发条件 | 预估时间 |
|---------|------|------|---------|---------|
| Long-term 1 | 多租户管理 API | 补齐 /org/*, /team/*, /user/* CRUD，完整多租户管理 | 有自托管客户需要 Web UI 管理团队 | 发布后 3 个月 |
| Long-term 2 | Redis 缓存 + 性能优化 | 语义缓存、响应缓存、连接池优化 | QPS 超过 1000 | 发布后 3-6 个月 |
| Long-term 3 | Observability | Prometheus metrics + OTEL tracing + 结构化日志 | 生产环境部署 | 发布后 6 个月 |
| Long-term 4 | 前端管理控制台 | 基于 Stage 4 规划的完整前端实现 | Stage 4 完成后持续推进 | 发布后 3-9 个月 |
| Long-term 5 | SSO/OAuth 鉴权 | 企业 SSO 集成，支持 OIDC/SAML | 企业客户需求 | 发布后 6-12 个月 |
| Long-term 6 | PostgreSQL 生产级支持 | 从 SQLite 升级到 PostgreSQL 的完整迁移工具 | 多实例部署 + 高可用需求 | 发布后 6-12 个月 |
| Long-term 7 | Kubernetes Operator | Helm Chart + K8s Operator 自动化运维 | 云原生客户需求 | 发布后 12 个月+ |

**原则**：长期路线中的每一项都在最小化版本的 schema 和代码结构中预留了扩展点（保留 FK 字段、保留表结构、保留接口前缀），确保后续扩展不需要破坏性迁移。

---

## 9. 核心假设

### 技术假设

| 编号 | 假设 | 验证状态 |
|------|------|---------|
| A1 | SQLite 可承载最小化版本的生产负载（< 500 QPS） | 待验证 |
| A2 | Rust axum + sqlx + tokio 可完整复刻 litellm Python FastAPI 的全部核心端点行为 | 待验证（原型已验证可行） |
| A3 | litellm schema.prisma 可通过 SQLite 完全复刻（无 PG 独有特性阻塞） | 待验证 |
| A4 | 单一二进制文件部署模式可覆盖 80% 自托管场景 | 待验证 |

### 业务假设

| 编号 | 假设 | 验证状态 |
|------|------|---------|
| B1 | Claude Code / Codex 仅依赖 OpenAI Chat API 标准格式，无需 litellm 特有扩展 | 已验证（原型测试） |
| B2 | 目标用户（k8z.dev 当前使用场景）仅需 Virtual Key + 用量统计，不需要完整多租户管理 UI | 待验证 |

---

## 10. 修订记录

| 版本 | 日期 | 修订内容 | 修订人 |
|------|------|----------|--------|
| v1.0 | 2026-07-02 | 初始版本（基于调研结果） | 全栈架构师 |
| v2.0 | 2026-07-03 | 基于用户反馈重规划：新增多租户兼容、长期路线、OpenAPI/前端、部署模式 | 全栈架构师 |
| v2.1 | 2026-07-03 | 落实表名决策（aigw 自有表名）+ aigw-migrate 双向迁移工具纳入 Stage 1 | 全栈架构师 |
| v2.2 | 2026-07-10 | 新增 Phase 13：用户反馈驱动的改进 + TTFT 实现差距修复（SSE streaming 代理、completion_start_time 捕获、Spend Logs/Usage/Users/Orgs/Playground 改进） | Claude Code |
