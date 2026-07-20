# aigw-migrate 工具参考手册

> 双向数据库迁移 CLI，在 litellm 与 aigw 之间传输数据，支持加密密钥轮转。

## 目录

- [快速开始](#快速开始)
- [子命令](#子命令)
  - [remote-import](#remote-import) — litellm → aigw（主迁移）
  - [remote-export](#remote-export) — aigw → litellm（回滚）
  - [import / export](#import--export) — 本地 SQLite 文件互转
  - [verify](#verify) — 行数校验
  - [pre-check](#pre-check) — 迁移前连通性检查
- [环境变量](#环境变量)
- [表映射关系](#表映射关系)
- [分步执行与表级筛选](#分步执行与表级筛选)
- [spend_logs 字段跳过](#spend_logs-字段跳过)
- [支持的数据库](#支持的数据库)
- [加密密钥轮转](#加密密钥轮转)
- [故障排查](#故障排查)

---

## 快速开始

```bash
# 从 litellm 迁移到 aigw（环境变量方式）
export AIGW_UPSTREAM_DB_URL="postgres://user:pass@host:5432/litellm"
export AIGW_DATABASE_URL="postgres://user:pass@host:5432/aigw"
export AIGW_MASTER_KEY="sk-aigw-xxxxxxxxxxxxxxxxxxxxxxxxxxxx"

aigw-migrate remote-import

# 或通过 CLI 参数显式指定
aigw-migrate remote-import \
  --source-url postgres://user:pass@host:5432/litellm \
  --target-url postgres://user:pass@host:5432/aigw \
  --target-master-key "sk-aigw-xxxxxxxxxxxxxxxxxxxxxxxxxxxx"
```

详细的生产迁移流程参见 [`migration-sop.md`](migration-sop.md)。

---

## 子命令

### remote-import

**litellm → aigw 全量迁移（含加密密钥轮转）**

```
aigw-migrate remote-import [OPTIONS]
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `--source-url` | URL | 否* | litellm 源数据库连接 URL。未提供时读取 `AIGW_UPSTREAM_DB_URL` |
| `--target-url` | URL | 否* | aigw 目标数据库连接 URL。未提供时读取 `AIGW_DATABASE_URL` |
| `--source-master-key` | string | 否 | 源 litellm 加密密钥。不传则从 `LiteLLM_Config` 自动提取 |
| `--target-master-key` | string | 否* | 目标 aigw 加密密钥。未提供时读取 `AIGW_MASTER_KEY` |
| `--spend-log-limit` | usize | 否 | 限制 spend_logs 迁移行数，不传则全量迁移 |
| `--step-filter` | u8 | 否 | 只执行指定的迁移步骤（2/3/4/5），详见[分步执行](#分步执行与表级筛选) |
| `--skip-body` | flag | 否 | 跳过 spend_logs 的大字段：`messages`、`response`、`proxy_server_request` |
| `--skip-columns` | list | 否 | 跳过指定表.列的迁移，逗号分隔，如 `spend_logs.custom_llm_provider,credentials.credential_info` |

\* 表示可通过环境变量替代，见[环境变量](#环境变量)。

**执行流程：**

```
Step 1: 提取源 master_key（自动从 LiteLLM_Config 或手动指定）
Step 2: 迁移 plain tables（无加密字段）
Step 3: 迁移 credentials（解密 → 重新加密）
Step 4: 迁移 proxy_models（解密 → 重新加密）
Step 5: 迁移 spend_logs（批量模式，无可加密字段）
Step 6: 行数校验
```

**常用示例：**

```bash
# 全量迁移
aigw-migrate remote-import

# 只迁移前 10000 条 spend logs
aigw-migrate remote-import --spend-log-limit 10000

# 只迁移 spend_logs（跳过其他表）
aigw-migrate remote-import --step-filter 5

# 迁移所有表但跳过 spend_logs
aigw-migrate remote-import --step-filter 2

# 跳过 spend_logs 的大字段以加快迁移
aigw-migrate remote-import --skip-body

# 跳过指定列
aigw-migrate remote-import --skip-columns spend_logs.custom_llm_provider,credentials.credential_info
```

> **注意**：`remote-import` 期望目标数据库的表结构已存在（通过 aigw-server 自动建表或手动 DDL）。工具只插入数据，不创建表结构。

### remote-export

**aigw → litellm 反向迁移（回滚）**

```
aigw-migrate remote-export [OPTIONS]
```

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `--source-url` | URL | 否* | aigw 源数据库。未提供时读取 `AIGW_DATABASE_URL` |
| `--target-url` | URL | 否* | litellm 目标数据库。未提供时读取 `AIGW_UPSTREAM_DB_URL` |
| `--source-master-key` | string | 否* | aigw 源密钥。未提供时读取 `AIGW_MASTER_KEY` |
| `--target-master-key` | string | 否 | litellm 目标密钥。不传则从 `LiteLLM_Config` 自动提取 |

示例：
```bash
aigw-migrate remote-export \
  --source-url sqlite:///path/to/aigw.db \
  --target-url sqlite:///path/to/litellm.db
```

### import / export

**本地 SQLite 文件之间的迁移。** 仅用于开发测试，不涉及加密密钥轮转。

```bash
# litellm SQLite → aigw SQLite
aigw-migrate import --source /path/to/litellm.db --target /path/to/aigw.db

# aigw SQLite → litellm SQLite
aigw-migrate export --source /path/to/aigw.db --target /path/to/litellm.db
```

### verify

**对比源和目标数据库的每张表行数。**

```bash
aigw-migrate verify --source-db /path/to/litellm.db --target-db /path/to/aigw.db
```

输出示例：
```
  LiteLLM_OrganizationTable -> organizations: src=5 tgt=5 [OK]
  LiteLLM_SpendLogs -> spend_logs: src=100000 tgt=100000 [OK]
  ...
```

任一表不匹配，命令以 exit code 1 退出。

### pre-check

**迁移前预检**：验证源/目标数据库连通性、master_key 有效性、表结构存在性。

```bash
aigw-migrate pre-check \
  --source-url postgres://user:pass@host:5432/litellm \
  --target-url postgres://user:pass@host:5432/aigw \
  --target-master-key "sk-aigw-xxxxxxxx"
```

全部通过后输出 `All checks passed. Ready to migrate.`。

---

## 环境变量

所有 URL 和密钥相关参数都可通过环境变量设置，CLI 参数优先级更高。

| 环境变量 | 对应 CLI 参数 | 说明 |
|----------|-------------|------|
| `AIGW_UPSTREAM_DB_URL` | `--source-url`（remote-import）/ `--target-url`（remote-export） | litellm 侧数据库 URL |
| `AIGW_DATABASE_URL` | `--target-url`（remote-import）/ `--source-url`（remote-export） | aigw 侧数据库 URL |
| `AIGW_MASTER_KEY` | `--target-master-key`（remote-import）/ `--source-master-key`（remote-export） | aigw 的 master 加密密钥 |

支持 `.env` 文件（`dotenvy`），自动从当前目录向上查找。

---

## 表映射关系

| litellm 表 | aigw 表 | 特殊处理 |
|-----------|---------|---------|
| `LiteLLM_VerificationToken` | `virtual_keys` | plain |
| `LiteLLM_OrganizationTable` | `organizations` | plain |
| `LiteLLM_TeamTable` | `teams` | plain |
| `LiteLLM_UserTable` | `users` | plain |
| `LiteLLM_ProjectTable` | `projects` | plain |
| `LiteLLM_BudgetTable` | `budgets` | plain |
| `LiteLLM_OrganizationMembership` | `organization_memberships` | plain |
| `LiteLLM_TeamMembership` | `team_memberships` | plain |
| `LiteLLM_Config` | `config` | plain |
| `LiteLLM_CredentialsTable` | `credentials` | **加密密钥轮转**（`credential_values` 字段） |
| `LiteLLM_ProxyModelTable` | `proxy_models` | **加密密钥轮转**（`litellm_params` 字段） |
| `LiteLLM_SpendLogs` | `spend_logs` | 批量迁移，无加密字段 |

列名自动从 camelCase 转为 snake_case（如 `organizationAlias` → `organization_alias`）。

---

## 分步执行与表级筛选

`--step-filter` 参数控制执行哪些迁移步骤：

| step-filter 值 | 执行的步骤 | 包含的表 |
|---------------|-----------|---------|
| `2` | plain tables only | organizations, teams, users, projects, budgets, org/team memberships, virtual_keys, config |
| `3` | credentials only | LiteLLM_CredentialsTable → credentials + 密钥轮转 |
| `4` | proxy_models only | LiteLLM_ProxyModelTable → proxy_models + 密钥轮转 |
| `5` | spend_logs only | LiteLLM_SpendLogs → spend_logs |
| 不传（默认） | 全部步骤 | 2 + 3 + 4 + 5 顺序执行 |

**典型场景：**

| 场景 | 命令 |
|------|------|
| 全量迁移 | `aigw-migrate remote-import` |
| 只迁移 spend_logs | `aigw-migrate remote-import --step-filter 5` |
| 迁移所有但跳过 spend_logs | `aigw-migrate remote-import --step-filter 2`（再分别跑 3、4） |
| 只迁移 spend_logs 前 5000 条 | `aigw-migrate remote-import --step-filter 5 --spend-log-limit 5000` |

> **注意**：`--step-filter` 不影响 Step 6（行数校验）—— 校验始终对比所有表。

---

## spend_logs 字段跳过

### --skip-body

跳过三个最大的 TEXT 字段以加快迁移速度：

- `messages` — 请求消息体（可能非常大）
- `response` — 响应体（可能非常大）
- `proxy_server_request` — 代理服务器请求体

```bash
aigw-migrate remote-import --skip-body
```

### --skip-columns

精确控制跳过的字段，格式 `table.column`，逗号分隔：

```bash
# 跳过多个字段
aigw-migrate remote-import \
  --skip-columns spend_logs.custom_llm_provider,spend_logs.messages,credentials.credential_info
```

---

## 支持的数据库

| 数据库 | URL 格式 | 说明 |
|--------|---------|------|
| PostgreSQL | `postgres://user:pass@host:5432/dbname` | 推荐生产环境使用 |
| SQLite | `sqlite:///absolute/path/to/file.db` 或 `sqlite://relative/path.db` | 开发/单机部署 |
| MySQL / MariaDB | `mysql://user:pass@host:3306/dbname` | 支持 |

源和目标可以是不同的数据库类型（跨数据库迁移）。例如从 MySQL litellm 迁移到 PostgreSQL aigw。

### 数据库类型识别

工具根据 URL 前缀自动识别数据库类型：
- `postgres://` 或 `postgresql://` → PostgreSQL
- `mysql://` 或 `mariadb://` → MySQL
- 其他 → SQLite

> **PostgreSQL 用户注意**：工具内部自动处理 PG 特有类型（jsonb, boolean, timestamptz, text[]）与通用类型的互转，无需手动干预。

---

## 加密密钥轮转

迁移 `credentials` 和 `proxy_models` 表时，工具自动执行加密密钥轮转：

```
litellm DB（LITELLM_MASTER_KEY 加密）
  → 解密 → 明文 → 重新加密
  → aigw DB（AIGW_MASTER_KEY 加密）
```

加密字段：
- `credentials.credential_values` — 凭证值（API key、base URL 等）
- `proxy_models.litellm_params` — 模型参数（含 API key）

### 密钥来源

| 密钥 | 来源优先级 |
|------|----------|
| 源 litellm master key | 1. `--source-master-key` CLI 参数<br>2. 从 `LiteLLM_Config` 表自动提取（`param_name='litellm_master_key'` 或 `param_name='general_settings'` 的 JSON 中 `master_key` 字段） |
| 目标 aigw master key | 1. `--target-master-key` CLI 参数<br>2. `AIGW_MASTER_KEY` 环境变量 |

---

## 故障排查

### 常见错误

#### "Source URL required" / "Target URL required"

未提供数据库连接 URL。设置环境变量或传递 CLI 参数：

```bash
export AIGW_UPSTREAM_DB_URL="postgres://user:pass@host:5432/litellm"
export AIGW_DATABASE_URL="postgres://user:pass@host:5432/aigw"
```

#### "Target master key required"

未提供 aigw 的 master key：

```bash
export AIGW_MASTER_KEY="sk-aigw-xxxxxxxxxxxxxxxxxxxxxxxxxxxx"
```

#### "No source master_key found"

litellm 数据库中找不到加密密钥。手动指定：

```bash
aigw-migrate remote-import --source-master-key "sk-litellm-xxxxxxxx"
```

#### credential 解密失败：`[WARN] Skipped N credential rows due to crypto errors`

源 litellm master key 与加密数据不匹配。检查密钥是否正确，或加密数据是否被修改过。被跳过的行不会被迁移。

#### 行数校验不匹配：`[MISMATCH]`

部分表的源/目标行数不一致。原因可能是：
- 迁移执行中被中断
- 源库和目标库的冲突键导致 `INSERT OR IGNORE` 跳过了部分行
- 加密轮转失败跳过了部分行

解决：重新执行迁移（工具使用幂等写入，重复执行不会产生重复数据）。

---

## 限制与已知问题

1. **spend_logs 不保证按 ID 顺序迁移**：当前实现是 `SELECT * FROM LiteLLM_SpendLogs LIMIT N`，无 `ORDER BY`。数据按存储顺序返回，不同数据库实现行为不同。
2. **spend_logs 不支持断点续传**：无 `--spend-log-start-id` 参数。中断后只能从头重新迁移（幂等重复执行不会重复数据）。
3. **不创建目标表结构**：迁移前需要确保目标数据库的表已存在（通过 aigw-server 启动自动建表，或手动执行 DDL）。
4. **import/export 子命令无加密轮转**：仅 `remote-import` / `remote-export` 支持加密密钥轮转。本地 `import`/`export` 只做纯文本复制。

---

## 相关文档

- [`migration-sop.md`](migration-sop.md) — 生产迁移 SOP（分阶段操作流程）
- [`adr/004-native-pool-migration.md`](adr/004-native-pool-migration.md) — ADR：原生连接池替代 AnyPool 的决策
- [`research/cross-db-migration-libraries.md`](research/cross-db-migration-libraries.md) — Rust 生态跨数据库迁移方案调研
