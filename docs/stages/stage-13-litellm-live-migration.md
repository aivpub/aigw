# Stage 13: 线上 litellm 生产迁移到 aigw

**创建日期**: 2026-07-06
**状态**: 规划中
**优先级**: P0（解锁真实生产迁移）

---

## 1. 背景与目标

### 1.1 当前能力

aigw 已支持从 litellm **离线 SQLite DB 文件**双向迁移，覆盖 9 张表（keys、spend_logs、orgs、teams、users、projects、budgets、memberships）。

### 1.2 不能迁移的

| 未覆盖表 | 原因（基线文档） | 实际影响 |
|---------|----------------|---------|
| `LiteLLM_ProxyModelTable` | "模型配置存 YAML" | **生产环境用 `store_model_in_db: true`，模型全在 DB 里，`litellm_params` 含 `api_key`** |
| `LiteLLM_CredentialsTable` | "仅 OpenAI 兼容 upstream 不需要" | **模型通过 `litellm_credential_name` 引用凭证获取上游 API Key** |

**结果：当前迁移后 aigw 有 keys 和 spend_logs，但没有模型配置和凭证，无法实际代理 LLM 请求。**

### 1.3 额外障碍

- **`litellm_params` 加密存储**：直接读 DB 拿到的是密文，只有通过 litellm REST API（`/model/list`）才能拿到解密后的配置
- **`aigw-migrate` 只支持 SQLite**：生产 litellm 常用 PostgreSQL
- **无 HTTP 客户端**：aigw 没有调用 litellm 管理 API 的能力
- **aigw `credentials` 表不存在**：完全没有凭证管理的数据模型

### 1.4 目标

实现从**运行中的生产 litellm 实例**（PostgreSQL/SQLite）迁移全部必要数据到 aigw：

1. Virtual Keys（55 列完整对齐）
2. 模型部署配置（含 `litellm_params.api_key`）
3. Provider 凭证（`credential_values` 含上游 API Key）
4. Spend Logs/请求记录
5. 多租户结构（Org/Team/User/Project/Budget）

**验收标准**：迁移完成后 aigw 能代理全部模型请求，用户无需重新配置。

---

## 2. 数据流全景

```
┌─────────────────────────────────────────────────────────────┐
│ 生产 litellm 实例 (PostgreSQL)                              │
│                                                             │
│ LiteLLM_CredentialsTable                                    │
│   credential_values → {api_key:"sk-xxx", api_base:"..."}    │
│         ↓ (litellm_credential_name 引用)                    │
│ LiteLLM_ProxyModelTable                                     │
│   litellm_params → {model:"openai/gpt-4o",                  │
│                     litellm_credential_name:"my-key"}        │
│         ↓ (models 字段引用)                                 │
│ LiteLLM_VerificationToken → users' virtual keys             │
│                                                             │
│ LiteLLM_SpendLogs → 请求记录                                │
│ Multi-tenant: Org → Team → User → Project → Budget          │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   ▼  Stage 13 实现
┌──────────────────────────────────────────────────────────────┐
│ aigw (PostgreSQL / SQLite)                                   │
│                                                              │
│ credentials (新增)  → credential_name + credential_values     │
│ proxy_models (已存在) → litellm_params + model_info           │
│ virtual_keys (已存在) → 55 列完整对齐                         │
│ spend_logs (已存在) → 24 列完整对齐                           │
│ organizations, teams, users, projects, budgets (已存在)       │
└──────────────────────────────────────────────────────────────┘
```

---

## 3. Stage 拆分

### Stage 13a: 基础设施补齐（2 张新表 + 凭证 CRUD）

**前置条件**：无
**目标**：补齐 aigw 缺失的数据模型

#### 交付

1. **`credentials` 表** — 列对齐 `LiteLLM_CredentialsTable`
   - `credential_id` (UUID PK)
   - `credential_name` (unique)
   - `credential_values` (JSON: `{api_key, api_base, api_version, ...}`)
   - `credential_info` (JSON, optional)
   - `created_at`, `created_by`, `updated_at`, `updated_by`

2. **凭证管理 CRUD 端点**
   - `POST /credentials` — 创建凭证
   - `GET /credentials` — 列表
   - `GET /credentials?credential_name=xxx` — 查询
   - `PUT /credentials/{credential_name}` — 更新
   - `DELETE /credentials/{credential_name}` — 删除

3. **路由层凭证引用解析**
   - `proxy_models.litellm_params` 中 `litellm_credential_name` 引用 `credentials` 表
   - 请求时替换 `api_key`/`api_base` 等凭证值

4. **`aigw-migrate` 扩展**
   - `TABLE_MAPPINGS` 加入 `LiteLLM_CredentialsTable` → `credentials`
   - `TABLE_MAPPINGS` 加入 `LiteLLM_ProxyModelTable` → `proxy_models`
   - 支持 PostgreSQL source（直接连接 litellm PG）

5. **数据库层**
   - `CredentialsStore` trait
   - SQLite + PostgreSQL 实现
   - `proxy_models` PostgreSQL 支持（当前仅 SQLite）

#### 门禁

- 新增表 migration SQL 正确执行
- `/credentials/*` CRUD BDD 测试通过
- 凭证引用解析：模型配置中 `litellm_credential_name` 能正确替换 `api_key`
- `aigw-migrate` 验证新表映射正确

**预估**: 4-6h

---

### Stage 13b: litellm API 客户端 + 线上提取

**前置条件**：Stage 13a 完成
**目标**：从运行中的 litellm 实例通过 API 提取数据

#### 核心问题

litellm 将 `litellm_params` 加密后存储。**直接读 DB 拿到密文**，只有通过 API 才能拿解密值。

#### 交付

1. **litellm 管理 API HTTP 客户端**（aigw 调用 litellm）
   - `GET /key/list` → 提取所有 virtual keys
   - `GET /model/list` → 提取所有模型配置（含解密后的 `litellm_params`）
   - `GET /credentials/list` → 提取凭证（如果暴露）

2. **`aigw-migrate remote-import` CLI 子命令**
   ```
   aigw-migrate remote-import \
     --source-url https://litellm-prod:4000 \
     --source-master-key sk-xxx \
     --target-db postgres://aigw:pass@localhost/aigw
   ```

3. **数据提取流程**
   ```
   Step 1: 调用 /key/list → 提取 keys（明文 token + 55 列配置）
   Step 2: 调用 /model/list → 提取 models（解密后的 litellm_params）
   Step 3: 直接读 DB 或调用 API → 提取 credentials + spend_logs + 多租户
   Step 4: 逐行写入 aigw 数据库（通过 KeyStore/CredentialsStore/ProxyModelStore）
   ```

4. **混合模式**：API 拿 keys/models（解密），DB 拿 spend_logs（量大，不需要解密）

#### 关键风险

| 风险 | 影响 | 缓解 |
|------|------|------|
| `api_key` 在 API 返回中被 mask（`sk-***`） | 无法迁移完整凭证 | 检查 litellm API 是否暴露明文；如不暴露，需要用 master key 直接读 DB + 解密 |
| `/credentials/list` 不存在 | 凭证无法通过 API 提取 | DB-level 提取 + litellm 加密解密函数调用 |
| PostgreSQL 连接需要网络权限 | 迁移工具需要能访问 litellm 的 PG | CLI 参数支持 `--source-db-url` |

#### 门禁

- `aigw-migrate remote-import` 能从测试 litellm 实例提取全部数据
- 提取的模型配置中 `api_key` 为明文（非 mask）
- 写入 aigw 后 `/v1/chat/completions` 能正常代理请求
- BDD 测试：完整迁移 + 验证端点

**预估**: 6-8h

---

### Stage 13c: 端到端迁移验证 + 回滚方案

**前置条件**：Stage 13b 完成
**目标**：完整的生产迁移流程 + 回滚能力

#### 交付

1. **迁移 SOP 文档**（`docs/migration-sop.md`）
   ```
   1. 准备阶段：确认 litellm 版本、DB 类型、数据量
   2. 预检：aigw-migrate verify（对比数据行数）
   3. 执行：aigw-migrate remote-import（全量导入）
   4. 验证：aigw 启动 → 健康检查 → 端到端测试
   5. 切换：DNS/负载均衡指向 aigw
   6. 回滚：回切到 litellm（5 分钟内）
   ```

2. **完整 BDD 迁移测试**
   ```gherkin
   Scenario: 从 litellm 迁移到 aigw 后模型代理正常
     Given litellm 实例已配置模型 "gpt-4o" 和凭证
     And litellm 已有 virtual key "migrate-test-key"
     When 运行 aigw-migrate remote-import
     And 启动 aigw 并加载迁移后的数据库
     Then aigw /v1/models 返回与前 litellm 相同的模型列表
     And 使用迁移后的 key 调用 /v1/chat/completions 成功

   Scenario: 迁移后 spend_logs 可查
     Given 迁移已完成
     When 查询 /spend/logs
     Then 日志数量与 litellm 一致
     And 每条日志 tokens > 0
   ```

3. **回滚工具**
   - `aigw-migrate export` 增强（支持 PostgreSQL 目标，包含 models + credentials）
   - 验证：aigw → litellm 回写后，litellm 能正常启动和代理

#### 门禁

- 迁移 SOP 对 litellm 测试实例执行，全部步骤通过
- BDD 迁移测试覆盖全流程
- 回滚测试：aigw 运行 N 分钟后，export 回 litellm，验证数据完整

**预估**: 4-6h

---

### Stage 13d: 凭证加密存储 + 安全增强

**前置条件**：Stage 13c 完成
**目标**：确保凭证数据在 aigw 中安全存储

#### 交付

1. **凭证加密存储**（对标 litellm 的 `encrypt_helper.py`）
   - 使用 `aigw-migrate` 的 master key 派生加密密钥
   - `credential_values` JSON 中的 `api_key` 加密存储
   - 运行时解密使用

2. **迁移时加密**
   - 从 litellm API 拿到明文 api_key
   - 写入 aigw DB 前用 aigw 的密钥加密

3. **运行时解密**
   - 请求到达时，从 `credentials` 表读取加密值
   - 解密后注入 `litellm_params` 发送给 upstream

#### 门禁

- 凭证在 DB 中以密文存储（验证方式：sqlite3 直接读看不到明文）
- 运行时能正常使用凭证代理请求

**预估**: 3-4h

---

## 4. 依赖关系

```
Stage 13a (基础设施: credentials 表 + proxy_models 迁移)
    │
    ├── Stage 13b (API 客户端 + remote-import)
    │       │
    │       └── Stage 13c (端到端验证 + 回滚)
    │               │
    │               └── Stage 13d (安全增强)
    │
    └── (并行) Stage 13a 中 PostgreSQL 支持是 13b 的前置
```

- 13a **必须**先完成（没有 credentials 表，无法存储导入的凭证）
- 13b 和 13c 是强依赖
- 13d 可以延后（不影响功能，属于安全增强）

---

## 5. 总预估

| Stage | 内容 | 预估 |
|-------|------|------|
| 13a | credentials 表 + 凭证 CRUD + proxy_models 迁移映射 + PG 支持 | 4-6h |
| 13b | litellm API 客户端 + remote-import CLI | 6-8h |
| 13c | 端到端验证 + 回滚 + SOP 文档 | 4-6h |
| 13d | 凭证加密存储 | 3-4h |
| **合计** | | **17-24h** |

---

## 6. 关键决策点 (需确认)

1. **litellm API 是否暴露明文 `api_key`？**
   - 如果 `/model/list` 返回 `api_key: "sk-***"`（masked），则需要走 DB 解密路径
   - 如果返回明文，API 提取方案直接可用
   - **建议**：先手动测试 `/model/list` 返回格式

2. **目标数据库类型？**
   - 如果用户生产 litellm 用 PostgreSQL，aigw 也要支持 PostgreSQL
   - `proxy_models` 当前仅 SQLite（`DbError::InvalidUrl`），需要补齐 PG 实现

3. **凭证独立存储还是内联？**
   - litellm 模式：凭证独立存储在 `credentials` 表，模型通过 name 引用
   - 简单模式：直接放 `proxy_models.litellm_params.api_key` 里
   - **建议**：先做简单模式（内联），后续按需扩展引用模式。13a 中建 `credentials` 表但优先支持内联 api_key
