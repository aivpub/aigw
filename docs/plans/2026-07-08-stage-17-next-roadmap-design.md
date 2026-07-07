# Stage 17 收尾 + Phase 8-9 路线图设计

**日期**: 2026-07-08
**状态**: 已确认

---

## 1. Stage 17 收尾（剩余 20%）

### 1.1 `aigw-migrate pre-check` 命令

在 `aigw-migrate` 中新增 `pre-check` 子命令，一键运行 6 项预检：

```
aigw-migrate pre-check \
  --source-url postgres://... \
  --target-url postgres://... \
  --target-master-key "$AIGW_MASTER_KEY"
```

| # | 检查 | 失败输出 |
|---|------|---------|
| 1 | 源 DB 连通 + 13 张表存在 | `[FAIL] source: LiteLLM_OrganizationTable missing` |
| 2 | 源核心表行数 > 0（VerificationToken, SpendLogs, OrganizationTable, ProxyModelTable, CredentialsTable） | `[FAIL] source: LiteLLM_VerificationToken has 0 rows` |
| 3 | 目标 DB 连通 | `[FAIL] target: connection refused` |
| 4 | 源 master_key 可提取 | `[FAIL] source: master_key not found` |
| 5 | AIGW_MASTER_KEY 已设置且 >= 32 字符 | `[FAIL] target: AIGW_MASTER_KEY missing or too short` |
| 6 | 加密/解密抽样（源第一条 credentials 解密成功） | `[FAIL] source: decryption test failed` |

输出：每行 `[PASS]` / `[FAIL]` + 汇总 `N/6 checks passed`。有失败则建议中止迁移。

### 1.2 回滚脚本 `scripts/rollback.sh`

封装 SOP Phase 5 的 Shell 脚本：

```
scripts/rollback.sh --aigw-url <URL> --litellm-url <URL> --aigw-master-key <KEY>
```

流程：
1. 停止 aigw server
2. `aigw-migrate remote-export`（aigw → litellm）
3. 启动 litellm
4. litellm 健康检查

### 1.3 更新 `docs/migration-sop.md`

将 SOP 中手工步骤替换为对 `pre-check` 命令和 `rollback.sh` 的引用。

---

## 2. Phase 8：可观测性 + 多租户管理

Stage 18 和 19 可并行开发。

### Stage 18：结构化日志

| 项目 | 内容 |
|------|------|
| 目标 | 统一日志格式（JSON）、request_id 追踪、日志级别配置 |
| 技术 | tracing + tracing-subscriber |
| 预估 | 2-3h |

- 所有请求自动注入 `request_id`（UUID v7）
- 日志输出格式：`{"timestamp": "...", "level": "INFO", "request_id": "...", "target": "...", "message": "..."}`
- 通过 `AIGW_LOG_LEVEL` 环境变量控制（默认 info）
- 覆盖：HTTP 请求/响应、DB 查询、upstream 调用、错误

### Stage 19：多租户管理 API

| 项目 | 内容 |
|------|------|
| 目标 | `/org/*`, `/team/*`, `/user/*` 完整 CRUD |
| 技术 | BDD 驱动，基于现有 Store trait 扩展 |
| 预估 | 4-6h |

端点列表：
- `POST /org/new`, `GET /org/info`, `PUT /org/update`, `DELETE /org/delete`, `GET /org/list`
- `POST /team/new`, `GET /team/info`, `PUT /team/update`, `DELETE /team/delete`, `GET /team/list`
- `POST /user/new`, `GET /user/info`, `PUT /user/update`, `DELETE /user/delete`, `GET /user/list`

全部 BDD 驱动，覆盖 CRUD + 关联查询。

### Stage 20：健康检查增强

| 项目 | 内容 |
|------|------|
| 目标 | `/health/metrics` 端点输出 DB 状态、uptime、key 数量 |
| 预估 | 1-2h |

- 依赖 Stage 18（结构化日志）

---

## 3. Phase 9：前端管理控制台

### 技术栈（已确认）

| 维度 | 选型 |
|------|------|
| 框架 | React + TypeScript + Vite |
| 组件库 | shadcn/ui（Radix UI + Tailwind CSS v4） |
| 图表 | shadcn/ui chart（基于 Recharts） |
| 状态管理 | TanStack Query + Zustand |
| 表单 | react-hook-form + zod |
| 图标 | Lucide React |
| Toast | Sonner |
| 部署 | Vite SPA → rust-embed |

### Stage 21：前端工程搭建

| 项目 | 内容 |
|------|------|
| 目标 | Vite + React + shadcn/ui 初始化、rust-embed 集成 |
| 预估 | 2-3h |

- `crates/aigw-frontend/` 目录，Vite SPA
- shadcn/ui 组件初始化（button, card, input, dialog, sheet, table, chart）
- 前端 build 产物通过 `rust-embed` 嵌入 `aigw-server`
- `/admin` 路由返回 SPA

### Stage 22：Key 管理页

| 项目 | 内容 |
|------|------|
| 目标 | virtual_keys 列表、搜索、创建、编辑、删除 |
| 预估 | 3-4h |

- 表格：key_alias、key（脱敏）、models、max_budget、expires、spend
- 搜索：按 key_alias 过滤
- 创建/编辑 Dialog（表单含 key_alias、models、max_budget、budget_duration、metadata）
- 删除确认 Dialog
- API key 显示/隐藏 toggle

### Stage 23：用量 Dashboard

| 项目 | 内容 |
|------|------|
| 目标 | 总 spend 卡片、按 model/provider 聚合图表、spend logs 表格 |
| 预估 | 4-6h |

- 总 spend 统计卡片（本月、总计）
- 按 model 消费柱状图
- 按 provider 消费环形图
- spend_logs 表格（时间、key、model、tokens、cost）
- 日期范围筛选

### Stage 24：Model 管理页

| 项目 | 内容 |
|------|------|
| 目标 | proxy_models 列表查看、详情展示 |
| 预估 | 2-3h |

- 模型列表（model_name、provider、modality）
- litellm_params 详情展示（JSON 格式化或关键字段提取）

---

## 4. 长期路线（保持不变）

Phase 10+ 按需触发：

| ID | 主题 | 触发条件 |
|----|------|---------|
| LT-2 | Redis 缓存 | QPS > 1000 |
| LT-3 | Prometheus + OTEL | 生产环境 |
| LT-5 | SSO/OAuth | 企业客户需求 |
| LT-7 | Kubernetes Operator | 云原生需求 |

---

## 5. Stage 依赖图

```
Stage 17 收尾 (pre-check + rollback.sh)
  │
  ├── Stage 18 (结构化日志) ──┬── Stage 20 (健康检查增强)
  │                          │
  └── Stage 19 (多租户 API) ─┼── Stage 21 (前端工程搭建)
                             │      │
                             │      ├── Stage 22 (Key 管理)
                             │      ├── Stage 23 (Dashboard)
                             │      └── Stage 24 (Model 管理)
```
