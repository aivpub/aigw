# litellm v1.90.0 → aigw Diff Baseline

**基线日期**: 2026-07-03
**litellm 版本**: v1.90.0
**基线用途**: 作为 Stage 1-3 代码对齐的参考文档

---

## 1. Schema Diff

### 需要完整保留的表（按优先级排序）

| litellm 表 | 列数 | aigw 状态 | 备注 |
|-----------|------|----------|------|
| LiteLLM_VerificationToken | 55 | ⏳ 模型已定义 | Key 管理核心，最高优先级 |
| LiteLLM_SpendLogs | 24 | ⏳ 模型已定义 | 用量数据核心 |
| LiteLLM_OrganizationTable | 12 | ⏳ 模型已定义 | 多租户 FK 链 root |
| LiteLLM_TeamTable | 28 | ⏳ 模型已定义 | 多租户 FK 链 |
| LiteLLM_UserTable | 22 | ⏳ 模型已定义 | 多租户 FK 链 |
| LiteLLM_ProjectTable | 15 | ⏳ 模型已定义 | Key-Project 关联 |
| LiteLLM_BudgetTable | 14 | ⏳ 模型已定义 | Budget FK |
| LiteLLM_OrganizationMembership | 7 | ⏳ 模型已定义 | 完整性 |
| LiteLLM_TeamMembership | 5 | ⏳ 模型已定义 | 完整性 |
| LiteLLM_DeprecatedVerificationToken | 4 | ⏳ 待添加 | Key 轮换灰度期 |
| LiteLLM_DeletedVerificationToken | 28 | ⏳ 待添加 | Key 审计 |

### 不需要的表（最小化版本）

| litellm 表 | 理由 |
|-----------|------|
| LiteLLM_CredentialsTable | Provider 凭证管理，仅 OpenAI 兼容 upstream 不需要 |
| LiteLLM_ProxyModelTable | Model 配置存 YAML，不需要 DB 表 |
| LiteLLM_EndUserTable | 终端用户追踪，超出范围 |
| LiteLLM_TagTable | Tag 管理系统，超出范围 |
| LiteLLM_AuditLog | 审计日志，超出范围 |
| LiteLLM_ErrorLogs | 错误日志，超出范围 |
| LiteLLM_UserNotifications | 用户通知，超出范围 |
| LiteLLM_InvitationLink | 邀请链接，超出范围 |
| LiteLLM_DailyUserSpend | 日统计表（可通过 SpendLogs 聚合） |
| LiteLLM_DailyOrganizationSpend | 同上 |
| LiteLLM_DailyEndUserSpend | 同上 |
| LiteLLM_DailyAgentSpend | 同上 |
| LiteLLM_DailyTeamSpend | 同上 |
| LiteLLM_DailyTagSpend | 同上 |
| LiteLLM_MCPServerTable | MCP 管理，超出范围 |
| LiteLLM_GuardrailsTable | Guardrails，超出范围 |
| LiteLLM_PolicyTable | Policy，超出范围 |
| LiteLLM_ToolTable | Tool 注册，超出范围 |
| LiteLLM_WorkflowRun | Workflow 追踪，超出范围 |
| LiteLLM_Config | Config 存 YAML，不需要 DB 表 |

---

## 2. API Endpoint Diff

### 必须实现（按优先级）

| litellm 端点 | 方法 | aigw 状态 | 备注 |
|-------------|------|----------|------|
| /v1/chat/completions | POST | ❌ 待实现 | 核心功能 |
| /v1/models | GET | ❌ 待实现 | Model list |
| /key/generate | POST | ❌ 待实现 | Key 生成 |
| /key/info | GET | ❌ 待实现 | Key 查询 |
| /key/update | POST | ❌ 待实现 | Key 更新 |
| /key/delete | POST | ❌ 待实现 | Key 删除 |
| /key/list | GET | ❌ 待实现 | Key 列表 |
| /key/regenerate | POST | ❌ 待实现 | Key 重新生成 |
| /user/info | GET | ❌ 待实现 | 只读 |
| /team/info | GET | ❌ 待实现 | 只读 |
| /org/info | GET | ❌ 待实现 | 只读 |
| /spend/logs | GET | ❌ 待实现 | Spend 查询 |
| /spend/keys | GET | ❌ 待实现 | By key |
| /spend/users | GET | ❌ 待实现 | By user |
| /spend/tags | GET | ❌ 待实现 | By tag |
| /global/spend/logs | GET | ❌ 待实现 | 全局 spend |
| /global/spend/keys | GET | ❌ 待实现 | 全局 by key |
| /global/spend/users | GET | ❌ 待实现 | 全局 by user |
| /global/spend/models | GET | ❌ 待实现 | 全局 by model |
| /health | GET | ❌ 待实现 | 健康检查 |
| /health/readiness | GET | ❌ 待实现 | 就绪检查 |
| /health/liveliness | GET | ❌ 待实现 | 存活检查 |
| /model/info | GET | ❌ 待实现 | Model 信息 |

### 跳过（最小化版本）

- /key/block, /key/unblock
- /team/* (CRUD，除 /team/info 只读)
- /org/* (CRUD，除 /org/info 只读)
- /user/* (CRUD，除 /user/info 只读)
- /project/* (CRUD)
- /customer/*
- /credentials/*
- /guardrails/*
- /mcp/*
- /cache/*
- /sso/*
- /fallback/*
- /budget/*
- /tag/*

---

## 3. Key 生成格式对齐检查清单

litellm `/key/generate` 返回值中 aigw 需对齐的字段：

- [ ] `token` — "sk-" + URL-safe base64 编码
- [ ] `key_name` / `key_alias`
- [ ] `user_id`, `team_id`, `organization_id`, `project_id`
- [ ] `budget_id`
- [ ] `models` — 字符串数组
- [ ] `max_budget`, `budget_duration`, `budget_reset_at`
- [ ] `tpm_limit`, `rpm_limit`, `max_parallel_requests`
- [ ] `expires`
- [ ] `metadata` — JSON object
- [ ] `permissions` — JSON object
- [ ] `auto_rotate`, `rotation_interval`, `key_rotation_at`
- [ ] `blocked`
- [ ] `spend`, `model_spend`, `model_max_budget`
- [ ] `created_at`, `created_by`, `updated_at`, `updated_by`

---

## 4. SpendLog 格式对齐检查清单

litellm SpendLog 中 aigw 需精确填充的列：

- [ ] `request_id` — UUID v4
- [ ] `call_type` — "completion" 等
- [ ] `api_key` — SHA256 hex hash
- [ ] `spend` — 浮点 cost
- [ ] `total_tokens`, `prompt_tokens`, `completion_tokens`
- [ ] `startTime`, `endTime` — RFC3339 DateTime
- [ ] `model` — 用户请求的 model name
- [ ] `model_id` — proxy model DB 中的 id
- [ ] `model_group` — public model group
- [ ] `custom_llm_provider` — "openai" 等
- [ ] `api_base` — upstream URL
- [ ] `user` — user_id (注意 litellm 列名就是 "user")
- [ ] `team_id`, `organization_id`
- [ ] `messages` — JSON
- [ ] `response` — JSON
- [ ] `session_id`
- [ ] `status` — "success" / "failure"

---

## 5. Table Name Mapping

aigw 代码库内部使用自己的表名，与 litellm 彻底解耦。
迁移工具 (`aigw-migrate`) 是唯二知道 litellm 表名映射的地方（import + export）。

| litellm 表名 | aigw 表名 | 备注 |
|---|---|---|
| `LiteLLM_VerificationToken` | `virtual_keys` | Key 管理核心 |
| `LiteLLM_SpendLogs` | `spend_logs` | 用量数据 |
| `LiteLLM_OrganizationTable` | `organizations` | 多租户 FK root |
| `LiteLLM_TeamTable` | `teams` | 多租户 FK |
| `LiteLLM_UserTable` | `users` | 多租户 FK |
| `LiteLLM_ProjectTable` | `projects` | Key-Project 关联 |
| `LiteLLM_BudgetTable` | `budgets` | Budget FK |
| `LiteLLM_OrganizationMembership` | `organization_memberships` | 完整性 |
| `LiteLLM_TeamMembership` | `team_memberships` | 完整性 |
| `LiteLLM_DeprecatedVerificationToken` | `deprecated_keys` | Key 轮换灰度期 |
| `LiteLLM_DeletedVerificationToken` | `deleted_keys` | Key 审计 |

列级别也是逐列映射的——不是简单的 `SELECT *`。
例如 aigw 使用 `spend` 列而 litellm 可能用 `spend` 列名相同但语义细微差异；
映射层吸收这些差异。

---

## 6. Bidirectional Migration Strategy

### 6.1 设计原则

**aigw 代码库内部使用自己的表名，与 litellm 彻底解耦。**
迁移工具是唯二知道 litellm 表名映射的地方（import + export）。

这样做：
1. aigw 代码不被 litellm 的历史包袱污染
2. 正向迁移是一次性的，迁移完 aigw 独立运行
3. 逆向迁移（回滚到 litellm）跑一次 `aigw-migrate export`，恢复 litellm 表名和格式
4. 未来 aigw schema 独立演进，只需要更新迁移工具中的映射表

### 6.2 迁移工具 CLI

```
# 正向迁移：litellm → aigw
aigw-migrate import \
  --from litellm --from-db /path/to/litellm.db \
  --to aigw --to-db /path/to/aigw.db

# 逆向迁移：aigw → litellm
aigw-migrate export \
  --from aigw --from-db /path/to/aigw.db \
  --to litellm --to-db /path/to/litellm-restored.db

# 校验模式：对比两边数据确认迁移正确
aigw-migrate verify \
  --from litellm --from-db /path/to/litellm.db \
  --to aigw --to-db /path/to/aigw.db
```

### 6.3 双向保证矩阵

| 方向 | 是否可行 | 保障方式 |
|------|---------|---------|
| litellm → aigw | ✅ 保证 | `aigw-migrate import`，逐列映射 |
| aigw → litellm | ✅ 保证 | `aigw-migrate export`，反向映射 |
| 往返 (litellm → aigw → litellm) | ⚠️ 有损 | aigw 跳过 ~20 张 litellm 表（DailySpend 等），这些在往返中丢失 |
| 运行后回滚 (aigw 运行 N 天后 → litellm) | ✅ 保证 | aigw 产生的核心表数据可完整逆向 |

### 6.4 不保证双向的场景

- **DailySpend 系列**（5 张表）：aigw 不保留，通过 SpendLogs 聚合，逆向时无法重建
- **Credential / Guardrail / Policy / MCP** 等 10+ 张表：aigw 不需要，导出时跳过
- **spend 计算精度**：aigw 用 model_info 单价计算，litellm 用 cost callback，同一条请求两边的 `spend` 列值可能有浮点误差（< 0.0001）

### 6.5 迁移步骤 (litellm → aigw)

```
1. 停止 litellm proxy
2. 复制 litellm SQLite DB 文件（保留原文件不动，安全回退）
3. 运行 aigw-migrate import（逐列映射，表名转换）
4. 启动 aigw 指向新生成的 aigw DB 文件
5. 执行端到端验证（Claude Code / Codex 测试）
```

### 6.6 回退步骤 (aigw → litellm)

```
1. 停止 aigw
2. 运行 aigw-migrate export（反向映射，恢复 litellm 表名）
3. 启动 litellm proxy 指向恢复后的 DB
4. aigw 运行期间产生的新 key 和 spendlog 数据已逆向写入 litellm 格式
5. 唯一丢失的是 aigw 不维护的 20 张 litellm 独有表（这些表在 aigw 运行期间无写入）
```

### 6.7 Stage 1 交付扩展

原交付：smoke test 导入→启动→查询
扩展为：导入→启动→运行→导出→litellm 验证

即 Stage 1 的 smoke test 需要覆盖完整的 往返流程：
1. 从 litellm DB 导入
2. aigw 启动成功，数据可查询
3. 运行一次 `aigw-migrate export`
4. 导出的 DB 在 litellm proxy 中启动成功
5. 数据（key、spendlog）完整可读
