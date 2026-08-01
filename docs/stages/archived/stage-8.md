# Stage 8: 模型管理 CRUD + 凭证管理（BDD 驱动）

**Status**: Planning
**Phase**: Phase 5 — 最小化后端补齐（RGR 驱动）
**预估工时**: 6-8h
**依赖**: Stage 7（BDD 框架）

## Goal

实现 litellm 兼容的模型管理 CRUD 和**独立的凭证（credential）管理**。新增 `proxy_models` 表和 `credentials` 表，字段对齐 litellm v1.90.3（`LiteLLM_ProxyModelTable` + `LiteLLM_CredentialsTable`）。模型可引用凭证而非直接嵌 api_key。全程 RGR 驱动。

## litellm v1.90.3 兼容性

### litellm 的凭证管理机制

litellm 有**独立的 credentials 表**（`schema.prisma:37-41`）：

```prisma
model LiteLLM_CredentialsTable {
  credential_id     String  @id @default(uuid())
  credential_name   String  @unique
  credential_values Json    // 含 api_key 等
  credential_info   Json?
}
```

模型表 `LiteLLM_ProxyModelTable` 通过 `litellm_credential_name`（schema.prisma:970）引用凭证。litellm 支持两种凭据方式：

1. **直接嵌 api_key**：`litellm_params: {"model":"gpt-4","api_key":"sk-xxx"}`
2. **引用 credential**：`litellm_params: {"model":"gpt-4","litellm_credential_name":"my-aws-cred"}`
   - 网关启动时从 `LiteLLM_CredentialsTable` 查 `credential_values` 注入

两者不互斥，可同时存在（credential 注入后覆盖 api_key）。

### 表结构映射

| litellm | aigw | 类型 | 说明 |
|---------|------|------|------|
| `LiteLLM_ProxyModelTable` | `proxy_models` | — | 模型表 |
| `model_id` (String, PK) | `model_id` (TEXT, PK) | String | UUID |
| `model_name` | `model_name` | TEXT | 模型别名 |
| `litellm_params` (Json) | `model_params` (JSON) | JSON | **aigw 重命名**，含 model/provider/api_base 等 |
| `model_info` (Json?) | `model_info` (JSON) | JSON | 元信息 |
| `litellm_credential_name` | `credential_name` (TEXT) | String | **aigw 重命名**，引用 credentials 表 |
| `blocked` (Boolean) | `blocked` (BOOLEAN) | Bool | 是否禁用 |
| `created_at` | `created_at` | TIMESTAMP | |
| `created_by` | `created_by` | TEXT | |
| `updated_at` | `updated_at` | TIMESTAMP | |
| `updated_by` | `updated_by` | TEXT | |

| litellm | aigw | 类型 | 说明 |
|---------|------|------|------|
| `LiteLLM_CredentialsTable` | `credentials` | — | 凭证表（新增） |
| `credential_id` | `credential_id` (TEXT, PK) | String | UUID |
| `credential_name` | `credential_name` (TEXT, UNIQUE) | String | 引用名 |
| `credential_values` | `credential_values` (JSON) | JSON | 含 api_key 等 |
| `credential_info` | `credential_info` (JSON) | JSON | 元信息 |

> **字段重命名理由**：litellm_params → model_params 更清晰（aigw 不依赖 litellm 命名）；litellm_credential_name → credential_name 简化。aigw-migrate 双向映射时处理字段名转换。

### model_params JSON 结构

```json
{
  "model": "gpt-4",
  "provider": "openai",
  "api_base": "https://api.openai.com/v1",
  "api_key": "sk-...",              // 方式 1：直接嵌
  "custom_llm_provider": "openai",
  "use_in_pass_through": false
}
```

或引用凭证（方式 2）：
```json
{
  "model": "gpt-4",
  "provider": "openai",
  "api_base": "https://api.openai.com/v1"
  // 不含 api_key，运行时从 credentials 表注入
}
```

## 关键交付件

1. `crates/aigw-core/src/models/proxy_model.rs` — `ProxyModel` 实体
2. `crates/aigw-core/src/models/credential.rs` — `Credential` 实体
3. `crates/aigw-core/src/repositories/proxy_model_repo.rs` — CRUD 仓库
4. `crates/aigw-core/src/repositories/credential_repo.rs` — 凭证 CRUD
5. `crates/aigw-core/src/credential_resolver.rs` — 运行时 credential 注入
6. `migrations/007_create_proxy_models.sql` — 三方言迁移
7. `migrations/008_create_credentials.sql` — 三方言迁移
8. `crates/aigw-server/src/routes/models.rs` — `/model/*` 路由
9. `crates/aigw-server/src/routes/credentials.rs` — `/credential/*` 路由（新增）
10. `crates/aigw-server/src/routes/v1_models.rs` — 增强 `/v1/models`
11. `tests/bdd/features/model.feature` — 模型管理 BDD 场景
12. `tests/bdd/features/credential.feature` — 凭证管理 BDD 场景
13. `tests/bdd/steps/model_steps.rs` / `credential_steps.rs`
14. `crates/aigw-migrate` — 新增 `proxy_models` + `credentials` 双向映射

## 端点契约

### POST /model/new
```json
// Request
{
  "model_name": "gpt-4",
  "model_params": {"model": "gpt-4", "api_key": "sk-..."},
  "model_info": {"description": "main gpt-4"},
  "credential_name": null
}
// 或引用凭证
{
  "model_name": "aws-claude",
  "model_params": {"model": "claude-3", "provider": "bedrock"},
  "credential_name": "my-aws-cred"
}
// Response
{ "model_id": "uuid-...", "model_name": "gpt-4" }
```

### GET /model/info
```json
// Query: ?model_id=uuid 或 ?model_name=gpt-4
// Response
{
  "data": [
    {
      "model_id": "uuid-...",
      "model_name": "gpt-4",
      "model_params": {...},
      "model_info": {...},
      "credential_name": "my-cred",
      "blocked": false,
      "created_at": "...",
      "updated_at": "..."
    }
  ]
}
```

### PATCH /model/{model_id}/update
```json
{ "model_params": {"api_key": "sk-new"} }
// 或切换为引用凭证
{ "credential_name": "new-cred", "model_params": {"api_key": null} }
```

### POST /model/delete
```json
{ "id": "uuid-..." }
// Response
{ "deleted": true, "model_id": "uuid-..." }
```

### POST /credential/new（新增）
```json
// Request
{
  "credential_name": "my-openai-key",
  "credential_values": {"api_key": "sk-xxx"},
  "credential_info": {"description": "prod key"}
}
// Response
{ "credential_id": "uuid-...", "credential_name": "my-openai-key" }
```

### GET /credential/info
```json
// Response（credential_values 中 api_key 脱敏）
{
  "data": [{
    "credential_id": "uuid-...",
    "credential_name": "my-openai-key",
    "credential_values": {"api_key": "sk-***"},
    "credential_info": {...}
  }]
}
```

### DELETE /credential/delete
```json
{ "credential_name": "my-openai-key" }
// Response
{ "deleted": true }
```

### GET /v1/models（增强）
- 优先从 `proxy_models` 表读取（过滤 blocked=true）
- 运行时通过 `credential_resolver` 注入 api_key
- 若 DB 为空，回退 config.yaml 的 `models`
- OpenAI 兼容格式：`{ "data": [{"id":"gpt-4","object":"model","owned_by":"openai"}] }`

## BDD 场景

### model.feature

```gherkin
Feature: 模型管理 CRUD
  作为管理员
  我需要管理模型配置
  以便动态添加、查询、修改、删除模型

  Scenario: 新增模型（直接嵌 api_key）
    Given 管理员已认证
    When 发送 POST /model/new 请求
      """
      {"model_name":"gpt-4","model_params":{"model":"gpt-4","api_key":"sk-xxx"}}
      """
    Then 响应状态码为 200
    And 响应包含 model_id 字段
    And model_id 以 uuid 格式

  Scenario: 新增模型（引用凭证）
    Given 已存在凭证 "my-cred"
    When 发送 POST /model/new 请求
      """
      {"model_name":"gpt-4","model_params":{"model":"gpt-4"},"credential_name":"my-cred"}
      """
    Then 响应状态码为 200
    And 该模型的 credential_name 为 "my-cred"

  Scenario: 查询所有模型
    Given 已存在 3 个模型
    When 发送 GET /model/info
    Then 响应包含 3 个模型

  Scenario: 按 model_name 查询
    Given 已存在模型 "gpt-4"
    When 发送 GET /model/info?model_name=gpt-4
    Then 响应仅包含 "gpt-4"

  Scenario: 更新模型配置
    Given 已存在模型 "gpt-4" 的 api_key 为 "sk-old"
    When 发送 PATCH /model/{model_id}/update 请求
      """
      {"model_params":{"api_key":"sk-new"}}
      """
    Then 该模型的 api_key 已更新为 "sk-new"

  Scenario: 切换凭据引用
    Given 已存在模型 "gpt-4" 直接嵌 api_key
    And 已存在凭证 "new-cred"
    When 发送 PATCH /model/{model_id}/update 请求
      """
      {"credential_name":"new-cred","model_params":{"api_key":null}}
      """
    Then 该模型引用 credential_name "new-cred"

  Scenario: 删除模型
    Given 已存在模型 "to-delete"
    When 发送 POST /model/delete 请求
      """
      {"id":"<model_id>"}
      """
    Then 该模型不再存在

  Scenario: 阻塞模型
    Given 已存在模型 "blocked-model"
    When 发送 PATCH /model/{model_id}/update 请求
      """
      {"blocked":true}
      """
    Then 该模型 blocked 字段为 true
    And /v1/models 不再返回该模型

  Scenario: /v1/models 动态列表
    Given DB 中存在 2 个未阻塞模型
    When 发送 GET /v1/models
    Then 响应包含 2 个模型
    And 响应格式符合 OpenAI 规范

  Scenario: DB 空时回退 config.yaml
    Given DB 中无模型记录
    And config.yaml 中配置了 1 个模型
    When 发送 GET /v1/models
    Then 响应包含 1 个模型

  Scenario: 凭证注入运行时生效
    Given 模型 "gpt-4" 引用凭证 "my-cred"
    And 凭证 "my-cred" 的 credential_values 含 api_key "sk-real"
    When 通过该模型发起调用
    Then 上游收到的 Authorization 包含 "sk-real"

  Scenario: 未认证请求被拒绝
    When 发送 POST /model/new 请求
      """
      {"model_name":"x"}
      """
    Then 响应状态码为 401
```

### credential.feature

```gherkin
Feature: 凭证管理 CRUD

  Scenario: 新增凭证
    Given 管理员已认证
    When 发送 POST /credential/new 请求
      """
      {"credential_name":"my-key","credential_values":{"api_key":"sk-xxx"}}
      """
    Then 响应状态码为 200
    And 响应包含 credential_id

  Scenario: 凭证名唯一约束
    Given 已存在凭证 "my-key"
    When 发送 POST /credential/new 请求
      """
      {"credential_name":"my-key","credential_values":{"api_key":"sk-yyy"}}
      """
    Then 响应状态码为 409

  Scenario: 查询凭证脱敏
    Given 已存在凭证 "my-key" 含 api_key "sk-secret"
    When 发送 GET /credential/info
    Then 响应的 api_key 字段为脱敏值
    And 不包含完整的 "sk-secret"

  Scenario: 删除凭证
    Given 已存在凭证 "to-delete"
    When 发送 DELETE /credential/delete 请求
      """
      {"credential_name":"to-delete"}
      """
    Then 该凭证不再存在

  Scenario: 删除被引用的凭证失败
    Given 模型 "gpt-4" 引用凭证 "in-use"
    When 发送 DELETE /credential/delete 请求
      """
      {"credential_name":"in-use"}
      """
    Then 响应状态码为 409
    And 错误信息表明凭证被引用
```

## RGR 循环

1. **Red**: 写 `model.feature`（12 场景）+ `credential.feature`（5 场景）→ 失败
2. **Green**: 实现 ProxyModel + Credential 实体、仓库、路由、credential_resolver → 逐场景通过
3. **Refactor**: 提取 JSON 字段处理 + credential 注入到 `aigw-core::credential_resolver`

## 数据库迁移

### SQLite — proxy_models
```sql
CREATE TABLE IF NOT EXISTS proxy_models (
  model_id TEXT PRIMARY KEY NOT NULL,
  model_name TEXT NOT NULL,
  model_params TEXT NOT NULL,        -- JSON
  model_info TEXT,
  credential_name TEXT,              -- 引用 credentials.credential_name
  blocked INTEGER NOT NULL DEFAULT 0,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  created_by TEXT NOT NULL DEFAULT 'system',
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_by TEXT NOT NULL DEFAULT 'system',
  FOREIGN KEY (credential_name) REFERENCES credentials(credential_name)
);
CREATE INDEX idx_proxy_models_name ON proxy_models(model_name);
```

### SQLite — credentials
```sql
CREATE TABLE IF NOT EXISTS credentials (
  credential_id TEXT PRIMARY KEY NOT NULL,
  credential_name TEXT NOT NULL UNIQUE,
  credential_values TEXT NOT NULL,   -- JSON
  credential_info TEXT,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
```

### MySQL / PostgreSQL
对应方言：`TEXT`→`JSON`、`INTEGER`→`BOOLEAN`、`TIMESTAMP` 用 `DEFAULT NOW()`。

## aigw-migrate 双向映射

- litellm → aigw:
  - `LiteLLM_ProxyModelTable.litellm_params` → `proxy_models.model_params`（字段重命名）
  - `LiteLLM_ProxyModelTable.litellm_credential_name` → `proxy_models.credential_name`
  - `LiteLLM_CredentialsTable` → `credentials`（字段直转）
- aigw → litellm: 反向映射

## 验收标准

- [ ] `model.feature` ≥ 12 个 Scenario 全部通过
- [ ] `credential.feature` ≥ 5 个 Scenario 全部通过
- [ ] `proxy_models` + `credentials` 表结构对齐 litellm v1.90.3
- [ ] `/model/new` `/model/info` `/model/{id}/update` `/model/delete` 可用
- [ ] `/credential/new` `/credential/info` `/credential/delete` 可用
- [ ] `/v1/models` 优先读 DB，空时回退 config.yaml
- [ ] credential_resolver 运行时注入 api_key
- [ ] 凭证查询时 api_key 脱敏
- [ ] 删除被引用凭证返回 409
- [ ] `aigw-migrate` 可双向同步 `proxy_models` + `credentials`
- [ ] 三方言迁移文件存在且可执行
- [ ] 管理员鉴权生效

## 风险

| 风险 | 缓解 |
|------|------|
| litellm_params 字段变化 | 锁定 v1.90.3 字段集，新增字段标记 optional |
| credential 注入时机 | 在 provider 调用前通过 credential_resolver 统一注入 |
| 凭证脱敏 | GET 响应统一 redact api_key，仅创建/更新时接受明文 |
| /v1/models 回退边界 | DB 查询失败也回退 config，记录 warning |
