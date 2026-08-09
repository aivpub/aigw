# 技术方案：request_id → call_id 改名 & 新增 upstream request_id

- **日期**: 2026-07-25
- **状态**: 待实施（v6.1 — v6 基础上补齐 Gate-2 评审 6 项：①export override 被 direct-match 抢占→源行剥离 request_id；②失败路径 upstream_id 走 INSERT 非 UPDATE（核心预期关键）；③Anthropic 流式提取位置须在 choices 分支前 + borrow；④Anthropic 失败响应头须预提取 request-id；⑤双列搜索 bind 机械 per-impl；⑥migrate NULL 语义澄清。详见 `docs/research/2026-07-27-stage-85-design-review-consolidated.md`）
- **类型**: 数据库 Schema 变更 + 全链路字段重命名
- **审计纪要**: v1-v5 见历史；v6 022→023 + import override 方向 + 测试清单；v6.1 export override direct-match + 失败路径 INSERT + Anthropic 流式/失败提取位置 + 双列 bind + migrate NULL 语义。Gate-2 评审：lead 独立 + 3 路 subagent（migration / migrate-frontend-tracing-tests / extraction-protocol）。

---

## 1. 背景与动机

### 1.1 现状问题

当前 aigw 将自身生成的 UUID v7 存储在 `spend_logs.request_id`（同时也是表主键），语义为"网关调用标识"。
但 `request_id` 这个名字在行业惯例（包括 litellm）中通常指**上游 LLM provider 返回的请求 ID**（如 Anthropic 的 `msg_xxx`、OpenAI 的 `chatcmpl-xxx`）。

当前设计导致两个问题：

| 问题 | 说明 |
|------|------|
| **语义混淆** | aigw 的 `request_id` 到底是网关 ID 还是上游 ID？代码中无法区分 |
| **无法与上游对账** | SpendLog 中没有存储上游 provider 的请求 ID，售后退款/问题排查时断链 |

### 1.2 litellm 对照

litellm 有两套 ID，各司其职：

| ID | 生成方 | 存储位置 | 用途 |
|----|--------|----------|------|
| `litellm_call_id` | litellm 内部 UUID | `kwargs` → `metadata` | 内部调用追踪，请求发起时即生成 |
| `request_id`（DB PK） | **上游响应 body 的 `id`** | `SpendLogs.request_id` | 与上游 provider 对账，fallback 到 `litellm_call_id` |

```python
# litellm/proxy/spend_tracking/spend_tracking_utils.py:163-169
def get_spend_logs_id(call_type, response_obj, kwargs):
    if call_type in ("aretrieve_batch", "acreate_file"):
        id = generate_hash_from_response(response_obj)
    else:
        id = response_obj.get("id") or kwargs.get("litellm_call_id")
    return id
```

aigw 的设计与 litellm 的差异在于：aigw 在请求刚发起时就 INSERT spend_log，用于实时展示 streaming 状态；litellm 在请求完成后才写入 DB。因此 aigw 不能照搬 litellm 的做法（上游 id 在响应回来前不可知）。

### 1.3 目标与约束

> **核心预期（v5 对齐）**：任意一条 SpendLog 记录都能用上游 `request_id` 去 provider 侧对上账，无论成功还是 4xx/5xx 失败。这是本方案唯一业务目标，其余均为支撑项。

**目标**（按主次排序）：

| 目标 | 说明 | 与核心预期的关系 |
|------|------|------------------|
| **打通对账链路** | SpendLog 存储上游 `request_id`（含成功与 4xx/5xx 失败路径），可直接用此 ID 与 provider 对账 | **核心本体** |
| **消除语义混淆** | 网关注册的调用 ID 叫 `call_id`，上游返回的请求 ID 叫 `request_id` | 支撑项——让 `request_id` 名字回归"上游 ID"语义，否则两个 request_id 又混 |
| **存量 litellm 兼容** | migrate 工具导入时，`call_id` = litellm 的 `request_id` | 支撑项——历史数据不丢，历史对账可回查 |

**约束/不变量**（非目标，改名时不可破坏）：

| 约束 | 说明 |
|------|------|
| **保持实时状态** | 主键仍然是 aigw 自己的 ID（`call_id`），请求发起时即 INSERT，streaming 状态不受影响 |
| **HTTP 层 `x-request-id` 头行为不变** | 见 §2.2，与 DB 字段改名完全独立 |
| **对外 API 响应体 `request_id` 协议字段不变** | 见 §6.3，与 DB 字段改名解耦 |

---

## 2. 方案概览

### 2.1 语义映射

```
旧：
  request_id = aigw 自身的 UUID v7（PK，所有链路的主标识）

新：
  call_id  = aigw 自身的 UUID v7（PK，网关调用唯一标识）   ← 请求发起时 INSERT
  request_id  = 上游 JSON body 的 "id"（如 msg_xxx）          ← 响应回来后 UPDATE
```

### 2.2 与 HTTP 头的关系（**不改名边界,务必区分**）

**不改变**。HTTP 层 `x-request-id` 头的行为保持不变:
- aigw 生成 UUID v7 → 写入 `x-request-id` 请求头 → 发送给上游
- 上游可能返回不同的 `x-request-id` → aigw 检测 mismatch 并 warn（已有逻辑）
- HTTP 头的 `x-request-id` 与 DB 字段 `call_id` / `request_id` 是**完全独立的两层**

> ⚠️ **关键边界**: Rust 代码里大量标识符也叫 `request_id`,但分属三层,改名时**绝不能混改**:
>
> | 层 | 标识符来源 | 是否改名 | 典型位置 |
> |----|-----------|---------|---------|
> | HTTP 中间件层 | `tower_http::request_id::{RequestId, MakeRequestId, SetRequestIdLayer}` | **不改** | `main.rs:55/102/110/116/124`、`chat.rs:24` 的 `use` |
> | HTTP 局部变量 | 路由函数内 `let request_id = extensions.get(...)` | **不改**(变量名是 HTTP 标识,值透传给 `call_id`) | `chat.rs:677/928/1073` |
> | DB / 模型层 | `SpendLog.request_id` 字段、SQL 列名、`get_spend_log_by_request_id` 方法 | **改** | `models.rs`、`db.rs`、`body_archive/*` |
>
> 路由层把 HTTP 变量值赋给 DB 字段时,写法是 `call_id: request_id.clone()`(变量名保留 `request_id`,字段名用 `call_id`),见 §4.3。

---

## 3. 数据库变更

### 3.1 迁移策略（**幂等：只加 023，不改 002/015**）

> ⚠️ **v1 硬伤修正**: v1 同时「改原迁移 002/015 的列名」+「新增 023 做 RENAME」，会导致**新装库**先跑 002（列已是 `call_id`）再跑 023 时 `RENAME` 报错（列不存在）。v2 改为**只加 023 迁移，不动 002/015**，存量库与新装库都靠 023 收敛。
>
> ⚠️ **v3 修正（v2 遗留）**：v2 的幂等条件单查 `EXISTS(request_id)`，与 Phase 2 新增的同名列冲突（重跑即报错）；MySQL Phase 2 误用原生 MySQL 不支持的 `ADD COLUMN IF NOT EXISTS`。v3 已全部修正，见下文。

**策略**：023 用条件探测，只在**旧列 `request_id` 存在且新列 `call_id` 不存在**时才 RENAME。`002/015` 保持原样不动 —— 它们仍以 `request_id` 建表，由 023 统一改名收敛。

> ⚠️ **v3 修正（v2 硬伤）**：v2 的探测条件只查 `EXISTS(request_id)`。但 Phase 2 会新增一个同样叫 `request_id` 的列（存上游 ID），导致 023 重跑时 Phase 1 探测为 TRUE、再次执行 RENAME 报错（`call_id` 已存在）。v3 改为 `EXISTS(request_id) AND NOT EXISTS(call_id)` 双重条件，保证 SQL 层面真正幂等（不依赖迁移器版本表）。

**Postgres** (`023_rename_request_id_to_call_id.sql`):

```sql
-- Phase 1: spend_logs 主键改名（旧列存在且新列不存在时才执行；零数据迁移，仅改元数据）
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'spend_logs' AND column_name = 'request_id'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'spend_logs' AND column_name = 'call_id'
    ) THEN
        ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id;
    END IF;
END $$;

-- Phase 2: 新增上游请求 ID（可空，响应回来后才填写）
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS request_id TEXT;

-- Phase 3: daily_tag_spend 关联字段同步改名（同样双重条件探测）
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'daily_tag_spend' AND column_name = 'request_id'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'daily_tag_spend' AND column_name = 'call_id'
    ) THEN
        ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id;
    END IF;
END $$;

-- Phase 4: 为上游 request_id 建索引（对账点查场景，见 §3.3）
CREATE INDEX IF NOT EXISTS idx_spend_logs_request_id ON spend_logs(request_id);
```

**MySQL**（无 `DO` 块，统一用 `INFORMATION_SCHEMA` + `PREPARE`）：

> ⚠️ **v3 修正（v2 硬伤）**：v2 的 Phase 2 写 `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`，**原生 MySQL 不支持该语法**（仅 MariaDB 支持），执行即语法错误。v3 全部改为 `PREPARE` 探测写法，与 Phase 1 风格统一。

```sql
-- Phase 1: spend_logs 主键改名（旧列存在且新列不存在时才执行）
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'request_id');
SET @new_col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'call_id');
SET @sql = IF(@col_exists > 0 AND @new_col_exists = 0,
    'ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 2: 新增上游请求 ID（不存在时才 ADD）
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'request_id');
SET @sql = IF(@col_exists = 0,
    'ALTER TABLE spend_logs ADD COLUMN request_id TEXT',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 3: daily_tag_spend 同理（旧列存在且新列不存在时 RENAME）
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'daily_tag_spend' AND column_name = 'request_id');
SET @new_col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'daily_tag_spend' AND column_name = 'call_id');
SET @sql = IF(@col_exists > 0 AND @new_col_exists = 0,
    'ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 4: request_id 索引（不存在时才建）
SET @idx_exists = (SELECT COUNT(*) FROM information_schema.statistics
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND index_name = 'idx_spend_logs_request_id');
SET @sql = IF(@idx_exists = 0,
    'CREATE INDEX idx_spend_logs_request_id ON spend_logs(request_id)',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;
```

**SQLite**（无 `DO`、无 `IF EXISTS` on `RENAME`；SQLite 3.35+ 支持 `ALTER TABLE ... RENAME COLUMN`，但无条件探测，需在应用层 / 迁移器侧判断 `PRAGMA table_info` 后再执行）：

```sql
-- Phase 1（由迁移器在 Rust 侧 PRAGMA table_info(spend_logs) 探测后决定是否执行：
--          仅当 request_id 存在且 call_id 不存在时执行）
ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id;
-- Phase 2（迁移器侧先检查 PRAGMA 避免重复 ADD）
ALTER TABLE spend_logs ADD COLUMN request_id TEXT;
-- Phase 3（同 Phase 1 的双重条件探测）
ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id;
-- Phase 4
CREATE INDEX IF NOT EXISTS idx_spend_logs_request_id ON spend_logs(request_id);
```

> SQLite 的幂等性靠 sqlx 迁移版本表（`_sqlx_migrations`）保证每个迁移文件只应用一次——SQLite 的 RENAME/ADD COLUMN SQL 本身不可重入（无 IF NOT EXISTS），但版本表保证单次应用，故无需 PRAGMA 探测。PG/MySQL 的双重条件探测作为防御性措施，若 SQL 被直接重应用也可幂等。

### 3.2 原迁移文件（**不改**）

`002_spend_logs.sql` / `015_daily_spend.sql`（pg/mysql/sqlite 共 6 个文件）**保持原样不动**，仍以 `request_id` 建表。所有库统一由 023 收敛为 `call_id`。这样：

- 存量库：002/015 已应用（列名 `request_id`）→ 023 RENAME 生效。
- 新装库：002/015 应用（列名 `request_id`）→ 023 RENAME 生效。
- 重跑库：023 已应用（列已是 `call_id` + 新 `request_id`）→ 双重条件探测 `EXISTS(request_id) AND NOT EXISTS(call_id)` 为 FALSE，跳过 RENAME；`ADD COLUMN IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS` 幂等。SQL 层面自身即可重入，不依赖迁移器版本表。

### 3.3 索引影响

| 索引 | 变更 |
|------|------|
| `spend_logs` 主键（`call_id`，原 `request_id`） | RENAME 不影响索引，无需额外操作 |
| **新增** `idx_spend_logs_request_id ON spend_logs(request_id)` | 023 Phase 4 创建。对账是本方案核心动机，对账查询形态是 `WHERE request_id = 'msg_xxx'` 点查，单列索引直接命中；可空列索引开销很小 |

### 3.4 回滚说明

回滚 = 反向执行 023，无数据迁移、仅元数据操作，可随时执行：

```sql
-- Postgres
DROP INDEX IF EXISTS idx_spend_logs_request_id;
ALTER TABLE spend_logs DROP COLUMN IF EXISTS request_id;          -- 删上游 ID 列（其数据无法恢复，回滚前确认）
ALTER TABLE spend_logs RENAME COLUMN call_id TO request_id;
ALTER TABLE daily_tag_spend RENAME COLUMN call_id TO request_id;
-- 代码回滚到改名前版本即可（字段名一一对应，无残留状态）
```

> 回滚会**丢失** 023 之后积累的上游 `request_id` 数据（该列被 DROP）。生产执行回滚前先导出备份。MySQL/SQLite 语句同理（MySQL `DROP COLUMN IF EXISTS` 需 MariaDB；原生 MySQL 用 §3.1 的 PREPARE 探测写法）。

### 3.5 历史数据说明

迁移后，**存量 spend_logs 行的 `request_id`（上游 ID）字段为 NULL** —— 历史调用从未存储上游 ID。售后对账涉及历史记录时，需回查 litellm 原库（migrate 工具导入时也只把 litellm 的 `request_id` 映射到 `call_id`，上游 ID 留空）。支持同学需知晓此限制，避免误判「上游 ID 缺失 = 故障」。

---

## 4. Rust 代码变更

### 4.1 模型层（aigw-core/src/models.rs）

```rust
// 变更前
pub struct SpendLog {
    pub request_id: String, // UUID, PK
    ...
}

// 变更后
pub struct SpendLog {
    pub call_id: String,          // aigw 自身 UUID v7, PK
    pub request_id: Option<String>,  // 新增：上游 JSON body 的 "id"，如 msg_xxx
    ...
}
```

> ⚠️ **v3 补漏**：`models.rs:185` 还有 `Tag { tag: String, request_id: String }` 枚举变体（对应 `daily_tag_spend` 的关联字段，与 §3.1 Phase 3 改的是同一张表），需同步改为 `Tag { tag: String, call_id: String }`，并核对 `Tag` 的所有构造点/匹配点一并改名。

### 4.2 DB 层（aigw-core/src/db.rs）

**列名映射**：所有 SQL 中的 `request_id` → `call_id`

```sql
-- 变更前
SELECT request_id, call_type, api_key, ... FROM spend_logs WHERE request_id = $1

-- 变更后
SELECT call_id, call_type, api_key, ... FROM spend_logs WHERE call_id = $1
```

**新增字段处理**：INSERT/UPDATE 时写入 `request_id`

```sql
INSERT INTO spend_logs (call_id, call_type, api_key, ..., request_id)
VALUES ($1, $2, $3, ..., NULL)  -- 请求发起时 request_id 为 NULL

-- 响应回来后 UPDATE
UPDATE spend_logs SET request_id = $2 WHERE call_id = $1
```

**关键方法改名**：

| 旧名称 | 新名称 |
|--------|--------|
| `get_spend_log_by_request_id()` | `get_spend_log_by_call_id()` |
| `update_spend_log()` 参数 `request_id` | 参数 `call_id` |
| `create_spend_log()` / `insert_spend_log()` 参数 `request_id` | 参数 `call_id` |

**新增：上游 id 写入方法（配合 §4.3 流式提取）**：

```rust
// 方案 A:扩展 update_spend_log,新增 upstream_request_id 参数
async fn update_spend_log(
    &self,
    call_id: &str,
    upstream_request_id: Option<&str>,  // 新增
    spend: f64,
    /* ...其余参数不变... */
) -> Result<()>;

// 对应 SQL 增加 request_id 列:
// UPDATE spend_logs SET spend=$1, ..., request_id=COALESCE($new, request_id) WHERE call_id=$N
// 用 COALESCE 避免上游无 id 时把已写值清空(NULL 不覆盖)
```

> `COALESCE($new, request_id)` 保证：流式提取到 id 就写入，没提取到（上游没返回 id 或解析失败）时不覆盖已有值。

**搜索兼容**：SpendLog 列表查询的 `request_id` 搜索参数需同时匹配两列：

```sql
-- 用户输入搜索时，同时匹配 call_id 和 request_id
WHERE (call_id = $search OR request_id = $search)
-- 或 LIKE 模糊匹配
WHERE (call_id LIKE $pattern OR request_id LIKE $pattern)
```

> ⚠️ `db.rs` 中有**三套 DB 实现**（Sqlite/Mysql/Postgres，各 ~7-8 处），加上 `Database` 枚举的转发层（`db.rs:2162/2273/2299/2309`），搜索逻辑改一处要同步改三套 + 转发层，共 ~25 处。实施时注意 `query_spend_logs_filtered` / `query_spend_logs_count` 在三个 impl 里的重复 SQL。

### 4.3 路由层

#### chat.rs

```rust
// 变更前：构造 SpendLog
SpendLog {
    request_id: request_id.clone(),
    ...
}

// 变更后
SpendLog {
    call_id: request_id.clone(),  // 变量名 request_id 是 HTTP 层标识，值不变
    request_id: None,                 // 响应回来后 UPDATE
    ...
}
```

> ⚠️ `chat.rs` / `v1_messages.rs` 中 `let request_id = extensions.get(...)` 这个**局部变量名保持 `request_id` 不变**（它是 HTTP 层 `x-request-id` 的标识，见 §2.2）。改的只是 `SpendLog` 字段名。同样的，`SpendLog { request_id: fail_request_id, ... }` 失败路径也要改字段名为 `call_id`。

**上游 id 提取 —— 非流式**：

```rust
// 非流式：响应 body 是单个 JSON，直接取 "id"
let upstream_id = response_json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
// UPDATE spend_logs SET request_id = $upstream_id WHERE call_id = $call_id
```

**上游 id 提取 —— 流式（v1 遗漏，v2 补；v4 修正伪代码：复用现有 chunk_jsons 收集循环，不新开循环）**：

> ⚠️ **v1 硬伤修正**: v1 只写了 `response_json.get("id")`,对流式不成立。aigw 流式是 SSE 透传(`bytes_stream`),没有单个 JSON body。每个 OpenAI chunk 的 `data:` 行里都带 `id` 字段(`chatcmpl-xxx`),Anthropic 流式的 `id` 在 `message_start` 事件里。必须从 chunk 解析,而非 body。
>
> ⚠️ **v4 伪代码修正**：v2/v3 的示例是「新开 `while let Some(chunk_result) = stream.next().await { ... }` 独立循环」，与现有代码不符。实际 `chat.rs:1228` / `v1_messages.rs:790` 在 stream 过程中已把 chunk 收集进 `chunk_jsons: Vec<Value>`（用于 Phase 2 assembled_response），upstream id 应在**同一个收集循环里顺手提取**，不要新开循环。

```rust
// chat.rs 流式分支(OpenAI 协议):在现有 chunk_jsons 收集循环里顺手提取 id
// 现有代码形态：let mut chunk_jsons: Vec<Value> = Vec::new();
//               while let Some(chunk_result) = stream.next().await { ... chunk_jsons.push(val); ... }
let mut upstream_id: Option<String> = None;
// 在 push chunk_jsons 的同一循环体内，加：
if upstream_id.is_none() {
    if let Some(id) = val.get("id").and_then(|v| v.as_str()) {
        upstream_id = Some(id.to_string());
    }
}
// Phase 2 UPDATE 时:
let _ = state_clone.db.update_spend_log_upstream_id(
    &request_id /* call_id */, upstream_id.as_deref(),
).await;
```

```rust
// v1_messages.rs 流式分支(Anthropic 协议):id 在 message_start 事件的 message.id 里
// event: message_start\ndata: {"type":"message_start","message":{"id":"msg_xxx",...}}
if upstream_id.is_none() {
    if let Some(msg) = chunk_json.get("message") {
        if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
            upstream_id = Some(id.to_string());
        }
    }
    // 或直接 chunk_json.get("id") 兼容 message_start
}
```

**DB 层需新增方法**（§4.2 补充）：流式 Phase 2 除了现有的 `update_spend_log`（更新 spend/tokens/...），还要把上游 id 写进去。两种做法选一：

- **方案 A（推荐）**：扩展 `update_spend_log` 签名，新增 `upstream_request_id: Option<&str>` 参数，在 UPDATE 语句里一并写 `request_id`。
- **方案 B**：新增独立方法 `update_spend_log_upstream_id(call_id, upstream_id)`,单独一条 UPDATE。流式/非流式最终都调它。

无论哪种，非流式分支（`chat.rs:1184` 的 `SpendLog { request_id: ... }` 构造后 INSERT，再在响应回来后 UPDATE）也要把上游 id 带上。

> ⚠️ **v5 补 — 失败路径 UPDATE 调用需补 `upstream_id` 参数**：方案 A 扩展 `update_spend_log` 签名后，所有调用点都要补这个参数，否则编译失败。失败路径 ×3（`chat.rs` 的 `:1002` 超时 / `:1108` 流式 4xx-5xx / `:1478` 流式末尾失败）和流式成功/失败 Phase 2 UPDATE（`:1360`/`:1386`）均需补：失败时传已提取的 `upstream_id`（按下方"失败路径上游 id 提取"），连接/超时无 body 时传 `None`。`COALESCE($new, request_id)` 保证 `None` 不覆盖已有值（流式部分成功后失败时已提取的 id 不被抹掉）。

**失败路径上游 id 提取（v5 新增 — 4xx/5xx 也提取存储）**：

> ⚠️ v4 §8 把"失败路径上游 id 未存储"列为后续增强、不阻塞发布。v5 按评审反馈改为本次一并实现：上游 4xx/5xx 时也从错误响应提取并存储上游 id，让失败请求也能对账。连接/超时失败（无响应 body）仍无可提取 id，留 NULL（无解）。

现状核验：`chat.rs:1093` 流式 4xx/5xx 分支已用 `upstream_resp.text().await` 拿到 `error_body` 字符串；非流式失败分支同理。`chat.rs:1067` 已有上游响应头 `x-request-id` 提取逻辑可复用作 fallback。

```rust
// OpenAI 失败（chat.rs，4xx/5xx）：error_body 已是字符串，解析后取 id
// error body 形如 {"error":{...},"id":"chatcmpl-xxx"} 或顶层带 id
let fail_upstream_id = serde_json::from_str::<Value>(&error_body)
    .ok()
    .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
    .or_else(|| upstream_req_id.clone());  // fallback：上游响应头 x-request-id
// 失败 SpendLog 构造时 call_id: fail_request_id（同值），上游 id 待 Phase 2 UPDATE
// 或失败分支也调 update_spend_log(call_id, upstream_id=fail_upstream_id, ...) 写入

// Anthropic 失败（v1_messages.rs，4xx/5xx）：error body 形如
//   {"type":"error","error":{...},"request_id":"req_xxx"}  ← 字段名就叫 request_id（协议）
// 另 Anthropic 在响应头带 request-id / x-request-id，可作 fallback
let fail_upstream_id = serde_json::from_str::<Value>(&error_body)
    .ok()
    .and_then(|v| v.get("request_id").and_then(|x| x.as_str()).map(|s| s.to_string()))
    .or_else(|| upstream_resp.headers().get("request-id")
        .or_else(|| upstream_resp.headers().get("x-request-id"))
        .and_then(|v| v.to_str().ok()).map(|s| s.to_string()));
```

> 注：Anthropic 错误 body 的 `request_id` 是**协议字段名**（§6.3），这里提取的是其**值**（上游 provider 分配的 id），赋给 DB 的 `spend_logs.request_id`（上游 ID 列）——字段名碰巧相同但语义衔接正确：上游协议返回的值 → aigw 上游 ID 列。不与 §6.3"对外响应体字段名保留"冲突。
>
> 流式部分成功后失败（已收首 chunk 后断开）：§4.3 的 `chunk_jsons` 收集循环里已提取首 chunk id 到 `upstream_id`，失败 Phase 2 UPDATE 用 `COALESCE($new, request_id)` 不会抹掉——已覆盖，无需额外处理。
>
> aigw 侧失败（鉴权失败、预算超限，没调上游）与连接/超时失败（无响应 body）：无上游 id 可提，`upstream_id = None`，DB 留 NULL。属不可避免，不算缺陷。

#### spend.rs

```rust
// API JSON 返回
serde_json::json!({
    "call_id": log.call_id,
    "request_id": log.request_id,  // 新增，上游 ID
    ...
})
```

URL 路径参数：

```rust
// 变更前：GET /global/spend/logs/{request_id}
// 变更后：GET /global/spend/logs/{call_id}
async fn global_spend_log_detail(Path(call_id): Path<String>) -> ... {
    db.get_spend_log_by_call_id(&call_id).await
}
```

### 4.4 Body Archive（aigw-core/src/body_archive/）

所有 `request_id` → `call_id`，包括：

| 位置 | 变更 |
|------|------|
| `BodyRow.request_id: String` | → `call_id: String` |
| SQL 查询中的 `request_id` 列 | → `call_id` |
| 函数参数 `request_id: &str` | → `call_id: &str` |

> ⚠️ **v3 新增 — 存量 parquet 兼容性**：body_archive 落盘的是 **parquet（带列名的 schema）**。`BodyRow.request_id` 改名后，新代码读取改名**前**已写出的 parquet 文件会 schema 不匹配（旧文件列名是 `request_id`，新 reader 找 `call_id`）。当前 body_archive 尚在 `feat/body-archive` 分支开发中、无线上存量数据，处置方式为：
>
> 1. **上线前**：清空开发/测试环境的 body_archive 存储目录与 `body_rows` 表，以新 schema 重新积累；
> 2. **代码侧**：parquet reader 增加列名兼容（读到 `request_id` 列时映射为 `call_id`）作为防御，低成本且避免未来再踩同类问题；
> 3. 若实施时 body_archive 已上线有真实数据，则改为在 023 中同步做一次 parquet 文件批量重写（重命名列），并在本文档补充该步骤。

### 4.5 Migrate 工具（aigw-migrate/）

> ⚠️ **v1 硬伤修正**: v1 没区分**读写 litellm 端的 SQL** 与**写入 aigw 的映射**。migrate 工具中大量 `request_id` 是 **litellm 源/目标表的列名**,绝不能改成 `call_id`,否则读不到 litellm 数据或导出的库不兼容 litellm。
>
> ⚠️ **v4 硬伤修正（v3 虚构映射处）**：v3 写「写入 aigw `SpendLog` 结构体的映射处」并给出 `SpendLog { call_id: litellm_record.request_id, ... }` 代码示例——**该构造处实际不存在**。migrate 工具走通用行流管线（`UnifiedRow = Vec<(String, Value)>`），全程不构造 `aigw-core::models::SpendLog` 结构体。真实机制见下方「映射机制」，真实改动点是列名 override 规则。

**映射机制（v4 实测）**：`remote_import.rs:482 migrate_spend_logs` 的实际链路是
1. `target.column_types("spend_logs")`（`native.rs:350`）从**目标库真实 schema** 取列名 → 023 迁移把列改名后，这里自动拿到 `call_id`；
2. `source.column_types("LiteLLM_SpendLogs")` 取源列名（含 `request_id`）；
3. `build_snake_overrides`（`remote_import.rs:38-43`）生成 `camelCase→snake` override map；
4. `native::insert_rows_batch`（`native.rs:1297`）→ `build_row_values`（`native.rs:1181/1275`）按 override 在源行里按列名查值，拼 `INSERT INTO spend_logs (...) VALUES ...`。

**致命点（v3 漏判）**：源端只有一个 `request_id` 列，目标端改名后有 `call_id`(PK, NOT NULL) + `request_id`(上游, nullable) **两个**同源同名列。列名批量映射会把源 `request_id` 直接写进目标 `request_id`（上游列），目标 `call_id`（PK）拿不到值 → **INSERT 失败**。必须显式加一条 override 把源 `request_id` 重定向到目标 `call_id`，目标上游 `request_id` 列在 migrate 路径下置 NULL（与 §3.5 历史数据说明一致）。

**真正要改的（v4 修正）**：在 `remote_import.rs` 导入路径注入一条源到目标的列名重定向 override。

```rust
// remote_import.rs — migrate_spend_logs 调用 insert_rows_batch 前，
// 在 overrides（build_snake_overrides 产物，key=目标列名 / value=源列名，由 `native.rs::build_row_values` 以 `column_override.get(target_col).and_then(|src| row_map.get(src))` 消费）上补一条显式重定向：
//   key   = 目标列名 "call_id"
//   value = 源列名 "request_id"（camel_to_snake 后仍是 "request_id"）
// 使源端 LiteLLM_SpendLogs.request_id 的值写入目标 spend_logs.call_id (PK)。
// 目标 spend_logs.request_id (上游 ID 列) 在 migrate 路径下不赋值，保持 NULL（见 §3.5）。
//
// remote_export.rs 反向（aigw spend_logs → LiteLLM_SpendLogs）同理：
// 源端 call_id → 目标 litellm request_id 的列名映射在 export 路径补一条 override。
```

> 注：override 注入点在 `migrate_spend_logs` 拿到 `overrides` 之后、`insert_rows_batch` 调用之前（`remote_import.rs:566` `let overrides = build_snake_overrides(...)` 紧接着），以 `overrides.insert("call_id".to_string(), "request_id".to_string())` 形式追加（key=目标 call_id、value=源 request_id）。export 侧在 `remote_export.rs:384` 对应位置追加**反向** override `overrides.insert("request_id".to_string(), "call_id".to_string())`（目标=litellm request_id、源=aigw call_id PK）。**方向必须核对**：`build_row_values`（native.rs:1291）以 target 列名为 key 查 override，错向则 PK 仍 NULL。建议跑现有 migrate idempotent 测试（`test_migrate_spend_logs_resume_idempotent`）验证。

**边界：litellm 源/目标表 SQL 不改**（仅 override 与 aigw 测试夹具改）：

| 文件 | `request_id` 用途 | 是否改名 |
|------|------------------|---------|
| `native.rs:482/562/575/579/585` — keyset 分页 `(startTime, request_id)` | **litellm 源表列** | **不改** |
| `native.rs:617` — `if name == "request_id"` 列名探测 | **litellm 源表列** | **不改** |
| `native.rs:1474/1482` — `SELECT "startTime", "request_id", "spend" FROM "LiteLLM_SpendLogs"` | **litellm 源表列**（测试断言） | **不改** |
| `native.rs:25` — 注释「idempotent on `request_id`」 | **litellm 源表语义** | **不改** |
| `remote_import.rs:865/1023/1041/1200/1211` — `CREATE TABLE "LiteLLM_SpendLogs"(...request_id...)` 测试夹具 | **litellm 源表 schema** | **不改** |
| `remote_import.rs:1224` — `CREATE TABLE "spend_logs"(request_id TEXT PRIMARY KEY...)` 测试夹具 | **aigw 目标表测试夹具** | **改**（`request_id`→`call_id`，与 023 收敛一致；否则测试跑 023 后列名不符） |
| `remote_export.rs:547` — `CREATE TABLE "spend_logs"(request_id TEXT PRIMARY KEY...)` 测试夹具 | **aigw 源表测试夹具** | **改**（同上） |
| `remote_export.rs:573/574` — `CREATE TABLE "LiteLLM_SpendLogs"(request_id...)` 测试夹具 | **litellm 目标兼容表** | **不改** |
| `main.rs:128` — 注释「idempotent on request_id」 | **litellm 源表语义** | **不改** |

### 4.6 改动范围统计（**v2 重估：区分目标端改 / 源端不改 / HTTP 层不改**）

> ⚠️ v1 的统计把 HTTP 层和 litellm 源端的 `request_id` 也算进改动量,会误导实施。v2 按**实际要改的**(目标端 aigw schema + 调用点)重估,并单列**不改的**两类。

**A. 需要改动（目标端 aigw schema / 调用点 / 路由 / 前端）**：

| 文件 | 改动性质 | 实测/预估改动量 | 备注 |
|------|----------|----------------|------|
| `aigw-core/src/models.rs` | 字段改名 + 新增 | 3 处 | `SpendLog.request_id` → `call_id` + 新增 `request_id: Option<String>` + **`Tag` 枚举变体（:185）`request_id` → `call_id`**（v3 补，对应 `daily_tag_spend`） |
| `aigw-core/src/db.rs` | SQL 列名 + 方法名 + 参数名 + 新增上游 id 方法 | ~90 处 | 含三套 DB 实现 + Database 转发层；实测 86 处 grep + 新增方法 |
| `aigw-core/src/body_archive/mod.rs` | 字段 + SQL + 函数参数 | 26 处 | 实测 |
| `aigw-core/src/body_archive/writer.rs` | 字段 + 函数参数 | 8 处 | 实测 |
| `aigw-core/src/body_archive/query.rs` | 字段 + SQL | 14 处 | 实测（v1 标 11 偏低） |
| `aigw-core/src/adapter.rs` | 测试/适配器代码 | ~5 处 | |
| `aigw-core/src/daily_spend_queue.rs:196` | **SQL 字符串列名**（非结构体字段） | 1 处 | `daily_tag_spend` UNIQUE 约束列串 `"request_id, tag, ..."` → `"call_id, tag, ..."`；与 §3.1 Phase 3 配套 |
| `aigw-server/src/main.rs:379` | 路由路径参数 | 1 处 | `/global/spend/logs/{request_id}` → `/{call_id}`；handler 参数名同步 |
| `aigw-server/src/routes/chat.rs` | 字段名 + 新增流式上游 id 提取 + 失败路径提取（v5） | ~6 处字段 + 新增逻辑 | 见 §4.3；含失败路径 `SpendLog{...}` ×3 + Phase 2 UPDATE 调用补 upstream_id 参数 |
| `aigw-server/src/routes/v1_messages.rs` | 字段名 + 新增流式 Anthropic id 提取 + 失败路径提取（v5） | ~45 处 + 新增逻辑 | 实测 45 处（含 SSE 解析 + 失败 error body/响应头提取） |
| `aigw-server/src/routes/spend.rs` | API JSON + SQL + URL 参数 | ~16 处 | `spend.rs:44/248/260/295/338/342/348/384/554/566` |
| `aigw-server/src/openapi.rs:255` | 返回字段定义 | 1 → 2 行 | 拆成 `call_id`（string, required）+ `request_id`（string, nullable） |
| `aigw-migrate/src/native.rs` | litellm 源端 SQL/列探测（不改，见 §4.5） | 0 处 | keyset 分页/列探测/测试断言均属 litellm 源表，保持 `request_id` |
| `aigw-migrate/src/remote_import.rs` | 注入 `request_id→call_id` override + aigw 测试夹具改列名 | 1 处 override + 1 处夹具(:1224) | 见 §4.5；litellm 源表夹具(:865/1023/1041/1200/1211)不改 |
| `aigw-migrate/src/remote_export.rs` | 注入反向 override + aigw 测试夹具改列名 | 1 处 override + 1 处夹具(:547) | 见 §4.5；litellm 目标表夹具(:573/574)不改 |
| 测试文件（BDD + steps，**10 个**；另含 5 个非 BDD 单测文件，见下） | 字段名 + mock 数据 + step 定义 | ~75 处 | BDD 47 处 + 非 BDD 单测 28 处；见下「测试文件清单」 |
| **合计** | | **~290 处 + 新增流式提取逻辑** | |

**测试文件清单（v1 漏列，v2 补全 9 个 BDD；v4 补全 5 个非 BDD 单测 + 修正路径前缀 + 补 common_steps.rs）**：

BDD（`crates/aigw-server/tests/`）：

| 文件 | 性质 | request_id 处数 |
|------|------|------|
| `features/spend.feature` | BDD 业务文本 | 2 |
| `features/spend_aggregation.feature` | BDD 业务文本 | 4 |
| `features/body_archive_write.feature` | BDD 业务文本 | 2 |
| `bdd_steps/body_archive_steps.rs` | step 定义 | 13 |
| `bdd_steps/spend_end_user_steps.rs` | step 定义 | 2 |
| `bdd_steps/real_db_seed.rs` | mock 数据 | 8 |
| `bdd_steps/messages_steps.rs` | step 定义 | 1 |
| `bdd_steps/spend_steps.rs` | step 定义 | 14 |
| `bdd_steps/common.rs` | 公共 step | 1 |
| `bdd_steps/common_steps.rs` | 公共 step（v4 补） | 见实测 |
| `features/body_archive_read.feature` | BDD 业务文本（v6 补，Stage 83 增） | 6 |

非 BDD 单元/集成测试（v4 补，方案 v3 完全漏列）：

| 文件 | 测试模块 | request_id 处数 | 备注 |
|------|----------|------|------|
| `aigw-core/tests/integration_test.rs` | 集成测试 | 1 | :220 `SpendLog{request_id: Uuid::new_v4()...}` 构造 |
| `aigw-core/src/body_archive/query.rs` | `#[cfg(test)]` (:88) | 7 | `BodyRow{request_id:"req-001"...}` |
| `aigw-core/src/body_archive/writer.rs` | `#[cfg(test)]` (:147) | 2 | `BodyRow{...}` + `format!("req-{:04}", i)` |
| `aigw-core/src/db.rs` | `#[cfg(test)]` (:4230) | 1 | :4599 `SpendLog{request_id:...}` 构造 |
| `aigw-server/src/routes/spend.rs` | `#[cfg(test)]` (:1152) | 5 | `SpendLog{...}` + 路由路径 `/global/spend/logs/{request_id}` 断言 + `assert!(val.get("request_id")...)` |
| `aigw-core/tests/stage82_state_machine.rs` | 集成测试（v6 补，Stage 82 增） | 1 | async_job 步骤 SpendLog/request_id 构造 |
| `aigw-core/tests/stage83_read_path.rs` | 集成测试（v6 补，Stage 83 增） | 3 | BodyRow `request_id` 构造 |

> BDD `.feature` 是业务文本（如 `Then the spend log has request_id "xxx"`），改字段名要同步改对应 step 定义，两端要对齐。非 BDD 单测里的 `SpendLog{request_id:...}` 构造与 `BodyRow{request_id:...}` 构造改名后必编译失败，务必同步。

**B. 不改动（务必跳过，否则破坏功能）**：

| 文件 / 位置 | 原因 |
|------------|------|
| `main.rs:55/102/110/116/124` — `tower_http::request_id::{RequestId, MakeRequestId, SetRequestIdLayer}` + tracing | HTTP 中间件层（§2.2） |
| `chat.rs:24` — `use tower_http::request_id::RequestId` | HTTP 层 import |
| `chat.rs:677/928/1073/1075/1076` — `let request_id = extensions.get(...)`、`x-request-id` 头、mismatch warn | HTTP 层局部变量 + 头逻辑 |
| `aigw-migrate/src/native.rs` 8 处 litellm 源端 SQL/列探测 | litellm 源表 schema（§4.5） |
| `aigw-migrate/src/remote_import.rs` 测试夹具 `CREATE TABLE "LiteLLM_SpendLogs"`（:865/1023/1041/1200/1211） | litellm 源表 schema |
| `aigw-migrate/src/remote_export.rs` litellm 兼容导出表 DDL（:573/574） | litellm 目标表 schema |
| `v1_messages.rs:48/141/165/179/213` + `chat.rs` 对外响应 body 的 `request_id` 字段 | Anthropic/OpenAI 协议字段（§6.3） |
| `v1_messages.rs:1424/1428` 测试断言 Anthropic 上游错误响应含 `request_id` | 上游协议字段断言（§6.3） |

---

## 5. 前端变更

### 5.1 API 响应类型

```typescript
// 变更前
interface SpendLog {
  request_id: string;
  ...
}

// 变更后
interface SpendLog {
  call_id: string;          // aigw 调用 ID（原 request_id，始终有值）
  request_id?: string | null;   // 上游返回的 ID（新增，可为空）
  ...
}
```

### 5.2 展示列调整

| 列位置 | 旧列名 | 新列名 | 数据来源 |
|--------|--------|--------|----------|
| 列表第一列 | Request ID | **Call ID** | `call_id` |
| 详情页 | Request ID | **Call ID** | `call_id` |
| 列表新增列 | — | **Upstream ID** | `request_id`（可为空则显示 "—"） |

### 5.3 搜索框

搜索框标签和 placeholder 保持不动：
- 搜索框输入的值同时搜索 `call_id` 和 `request_id`
- API 端已处理：`WHERE call_id LIKE $1 OR request_id LIKE $1`

### 5.4 CSV 导出

```typescript
// 变更前
const headers = ["Request ID", ...];
rows = logs.map(l => [l.request_id, ...]);

// 变更后
const headers = ["Call ID", "Upstream ID", ...];
rows = logs.map(l => [l.call_id, l.request_id ?? "", ...]);
```

### 5.5 前端改动散落点（v4 新增 — v3 低估）

> ⚠️ v3 步骤 13 估 20min 偏低。v4 实测：request_id 在前端散落 2 个页面文件、3 个独立重复定义的 interface（不在共享类型文件）、5 个本地 state 变量。

| 文件 | 改动点 | 处数 |
|------|--------|------|
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | interface SpendLog(:35) + interface SpendLogDetail(:47) + CSV headers/rows(:119-127) + 详情抽屉(:350-351) + 列表表头/cell(:639/668) + 移动端(:700-701) + 行 key/onClick(:648/680) + 搜索框 placeholder/state(:481-482/504-508/538) + queryKey(:532) + 详情端点 URL(:559) | ~16 处 |
| `crates/aigw-frontend/src/pages/dashboard/index.tsx` | interface SpendLog(:43) + 行 key(:398/432) | 3 处 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` + `spend-logs.steps.ts` | mock 数据 + step 定义 | 前端 BDD 同步 |

> 5 个本地 state 变量：requestIdFilter / requestIdInput / detailRequestId / handleRequestIdInput / 搜索 URL 参数 ?request_id=。变量名是否同步改为 gwCallId* 取决于前端代码风格偏好（不影响功能），但 API 字段 request_id→call_id 必须改。§7 步骤 13 耗时已修正为 40-60min。

---

## 6. API 兼容性

### 6.1 不兼容变更

| API | 旧 | 新 |
|-----|----|----|
| `GET /global/spend/logs` 返回 | `"request_id"` | `"call_id"` + `"request_id"` |
| `GET /global/spend/logs/{id}` | `{request_id}` | `{call_id}` |

当前 aigw 尚未对外发布，无外部依赖方，不兼容变更可接受。

### 6.2 查询参数兼容

```
GET /global/spend/logs?request_id=xxx   ← 参数名不变，行为改为搜索两列
```

> ⚠️ **已知妥协**：参数名 `request_id` 却在搜 `call_id` + `request_id` 两列，API 文档与直觉有偏差。已在 OpenAPI（§4.6 openapi.rs）注明「同时匹配网关调用 ID 与上游返回 ID」。后续若反馈混淆，可考虑改名 `?call_id=` 并保留 `?request_id=` 别名一段时间做平滑迁移。当前不阻塞发布（aigw 未对外）。

### 6.3 对外协议字段边界（v4 新增 — 与 DB 改名解耦）

> ⚠️ v3 遗漏：v3 只讨论了 /global/spend/logs 的兼容性，没提对外 LLM API 响应体里的 request_id 字段——这是 Anthropic / OpenAI 协议契约，不是 aigw 自有字段。

边界：aigw 在 chat.rs / v1_messages.rs 对外返回的 LLM API 响应体里带 request_id 字段（v1_messages.rs:48/141/165/179/213 错误体、chat.rs 成功/错误响应），字段名必须保留 request_id 不改（协议要求，客户端用它对账/排错），但其值来源是 HTTP 层 request_id 变量（即 aigw 生成的 UUID v7，语义=call_id）。

| 位置 | 字段名 | 值 | 是否改名 |
|------|--------|-----|---------|
| v1_messages.rs:48/141/165/179/213 Anthropic 错误响应 body 的 request_id | request_id | HTTP 层 request_id（= call_id 语义） | 字段名不改（协议） |
| chat.rs OpenAI 错误/成功响应 body 的 request_id | request_id | 同上 | 字段名不改（协议） |
| v1_messages.rs:1424/1428 测试断言 Anthropic 上游错误响应含 request_id | request_id | 上游 Anthropic 返回的 request_id | 不改（断言上游协议字段；与本次新增 DB request_id 列语义重叠但不冲突——前者是上游错误 body 字段名，后者是 DB 列） |

> 即：对外 API 响应体的 request_id 字段名保留、值= call_id，与 DB 字段改名完全解耦。实施时勿把响应体协议字段也改成 call_id，否则破坏客户端契约。§4.6 B 表已补此条为不改清单。

---

## 7. 实施步骤

| 步骤 | 内容 | 预计耗时 |
|------|------|----------|
| 1 | 创建 DB 迁移脚本 `023_rename_request_id_to_call_id.sql`（pg/mysql/sqlite；双重条件探测 `EXISTS(request_id) AND NOT EXISTS(call_id)`；MySQL 全用 PREPARE 写法；含 Phase 4 索引） | 25min |
| 2 | **不改** `002_spend_logs.sql` / `015_daily_spend.sql`（由 023 统一收敛，见 §3.2） | 0min |
| 3 | 修改模型层 `models.rs`：`SpendLog.request_id` → `call_id` + 新增 `request_id: Option<String>` + `Tag` 变体（:185）改名 | 10min |
| 4 | 修改 DB 层 `db.rs`：SQL 列名、方法名、参数名（三套实现 ~86 处）+ 新增/扩展上游 id 写入方法（§4.2 方案 A） | 40min |
| 5 | 修改 body_archive：字段 + SQL（~48 处）+ parquet reader 列名兼容（§4.4）；清空开发/测试环境存量 parquet | 25min |
| 6 | 修改 `daily_spend_queue.rs:196` 的 `daily_tag_spend` UNIQUE 列串 | 5min |
| 7 | 修改路由层 `chat.rs`：字段名（含失败路径 ×3 + Phase 2 UPDATE 调用补 upstream_id 参数）+ 新增流式 OpenAI chunk id 提取（§4.3）+ 失败 4xx/5xx 从 error body 提取 id（§4.3 失败路径，v5） | 40min |
| 8 | 修改路由层 `v1_messages.rs`：字段名 + 新增流式 Anthropic `message_start.id` 提取（§4.3）+ 失败 4xx/5xx 从 error body/响应头提取 id（§4.3 失败路径，v5） | 40min |
| 9 | 修改路由层 `spend.rs`：API JSON 字段 + URL 路径参数 | 10min |
| 10 | 修改 `main.rs:379` 路由路径参数（**勿动** `tower_http` 相关 5 处） | 5min |
| 10b | 处置 `main.rs:126` tracing span 字段（§10 方案 A：span field `request_id` → `call_id`，变量名不改） | 5min |
| 11 | 修改 `openapi.rs:255`：拆成 `call_id` + `request_id` 两个字段 | 5min |
| 12 | 修改 migrate 工具：remote_import.rs/remote_export.rs 各注入一条列名 override + 改 aigw 侧测试夹具列名（native 源端 SQL 不改，见 §4.5） | 15min |
| 13 | 修改前端类型 + 展示列 + CSV + 搜索逻辑（3 个独立 interface 定义 + 5 个 state 变量，散落 2 个页面文件，v4 修正耗时） | 40-60min |
| 14 | 更新所有测试文件（10 个 BDD + 5 个非 BDD 单测，见 §4.6 测试清单）的字段名和 mock 数据 | 35min |
| 15 | 迁移脚本联调：pg/mysql/sqlite 三端各验证「存量库 / 新装库 / 重跑库」三种路径均通过（§3.1 幂等性） | 30min |
| 16 | `cargo check` 编译通过 + `cargo test` 全部通过 | — |
| 17 | 端到端验证：①非流式 → DB `request_id` 写入 → 前端 Upstream ID 列显示；②流式 SSE → 首 chunk 提取 id → DB `request_id` 非空；③搜索同时命中 `call_id` 与 `request_id`；④对账点查 `WHERE request_id = ?` 走索引（EXPLAIN 验证）；⑤**失败 4xx/5xx（v5）**：上游返回错误 → 从 error body/响应头提取 id → DB `request_id` 非空、`call_id` 非空、status=failure；⑥连接/超时失败 → DB `request_id` NULL（不可避免）、`call_id` 非空 | — |

---

## 8. 风险评估

| 风险 | 可能性 | 影响 | 缓解 |
|------|--------|------|------|
| SQL 改名遗漏 | 中 | 编译报错 | 编译器强制检查，遗漏即编译失败 |
| 前端字段遗漏 | 低 | 展示空白 | TypeScript 类型检查 + 手动验证 |
| migrate 导入错位 | 低 | 存量数据 call_id 不正确 | migrate 有对应测试用例 |
| API 不兼容影响外部 | 无 | 无 | 未对外发布，无外部调用方 |
| **HTTP 层 `request_id` 误改** | 中 | 编译失败 / HTTP 中间件失效 | §2.2 明确分层边界；§4.6 B 单列不改清单 |
| **migrate litellm 源端 SQL 误改** | 中 | 读不到 litellm 数据 / 导出不兼容 | §4.5 明确源端/目标端边界；§4.6 B 单列 |
| **migrate 源端 request_id 未 override 到 call_id**（v4 新增） | 高 | 存量导入 PK 为 NULL，INSERT 失败 | §4.5 注入 `request_id→call_id` override；§7 步骤 12 |
| **对外协议响应体 request_id 误改成 call_id**（v4 新增） | 中 | 客户端契约破坏，对账断链 | §6.3 明确协议字段边界；§4.6 B 补不改清单 |
| **tracing span/log 字段 request_id 与 DB 改名语义不一致**（v4 新增） | 中 | 排障时日志字段名误导（reader 误以为是上游 ID） | §10 明确处置：span 字段同步改名或加 call_id 别名 |
| **023 迁移非幂等导致新装库报错** | 低（v3 已修） | 新部署失败 | §3.1 双重条件探测 `EXISTS(request_id) AND NOT EXISTS(call_id)` + `IF NOT EXISTS`；§7 步骤 15 三端三路径联调验证 |
| **MySQL `ADD COLUMN IF NOT EXISTS` 语法报错** | 低（v3 已修） | 迁移失败 | §3.1 MySQL 全部改 PREPARE 探测写法 |
| **存量 parquet schema 不匹配** | 低（v3 已覆盖） | body_archive 读历史数据报错 | §4.4：上线前清空 + reader 列名兼容防御 |
| **流式上游 id 未提取** | 高 | 上游 ID 永远 NULL，对账断链 | §4.3 补流式 SSE chunk 解析（OpenAI chunk.id / Anthropic message_start.id） |
| **OR+双列 LIKE 搜索性能退化** | 低（当前量级）| 大数据量下全表扫 | §4.2 注释；§3.3 已对 `request_id` 建索引（点查场景覆盖），LIKE 前缀匹配可走索引；未来量大会再拆两次查询 |
| **历史行 request_id 为 NULL** | 确定 | 历史对账需回查 litellm | §3.5 已说明，支持同学知晓 |
| **失败路径上游 id 未存储** | 低（v5 已实现） | 失败请求无法对账 | §4.3「失败路径上游 id 提取」：4xx/5xx 从 error body 取 id（OpenAI body 的 `id` / Anthropic body 的 `request_id`）+ 响应头 fallback；连接/超时无 body 留 NULL（不可避免） |
| **失败路径 error body 解析失败/上游无 id 字段**（v5 新增） | 中 | 失败请求上游 id 仍 NULL | 解析失败 `.ok()` 静默降级为 None，不报错；`COALESCE` 保证不覆盖；属上游协议差异，可接受 |

---

## 9. 决策记录

- **为什么本次要把上游 request_id 存进 SpendLog**（v5 新增，顶层业务动机）：售后退款/问题排查时需用 provider 返回的 request_id 与上游对账，当前 SpendLog 未存该 ID，对账断链。这是本方案唯一业务驱动；改名 `call_id`、流式提取、失败路径提取（v5）均为支撑此目标。
- **为什么不用 `id` 做主键名**：`id` 语义太泛，在 Rust 代码中 `log.id` 不如 `log.call_id` 清晰
- **为什么不保持 `request_id` 做 PK 名**：行业惯例中 `request_id` 指向上游 provider 的 ID，litellm 就是如此。保留这个名字会持续造成混淆
- **为什么不新增 `upstream_response_id` 而用 `request_id`**：语义上就是上游的请求 ID，与 litellm 保持一致，减少学习成本
- **为什么查询参数保持 `request_id` 不变**：对用户来说，「通过 request ID 搜索」是自然语义，不需要区分是搜索网关 ID 还是上游 ID（已知妥协见 §6.2）
- **为什么只加 023 迁移而不改 002/015**（v2 新增）：避免「新装库 002 已建 call_id → 023 RENAME 报错」的幂等冲突；存量/新装/重跑三类库都靠 023 条件探测统一收敛，工作量更小、行为更可预测
- **为什么 023 探测条件是双重条件**（v3 新增）：Phase 2 新增的列也叫 `request_id`，单查 `EXISTS(request_id)` 在重跑时恒为 TRUE，会再次触发 RENAME 报错。必须 `EXISTS(request_id) AND NOT EXISTS(call_id)` 才能在 SQL 层面自身幂等，不依赖迁移器版本表
- **为什么 MySQL 不用 `ADD COLUMN IF NOT EXISTS`**（v3 新增）：原生 MySQL 不支持该语法（仅 MariaDB 支持），统一用 `INFORMATION_SCHEMA + PREPARE` 探测写法，三端行为一致
- **为什么 023 就建 `request_id` 索引而非推迟**（v3 新增）：对账是本方案核心动机，对账查询形态是 `WHERE request_id = ?` 点查，单列索引直接命中；可空列索引开销极小，没必要留到「未来」
- **为什么流式要从 chunk 提取 id 而非 body**（v2 新增）：aigw 流式是 SSE 透传，没有整体 JSON body；OpenAI 每个 chunk 带 `id`，Anthropic 在 `message_start` 事件带 `message.id`，只能在 chunk 级别解析

---

## 10. 可观测性影响（v4 新增 — v3 完全未讨论）

> ⚠️ v3 全文未触及 tracing span / metrics label / 结构化日志字段的同步改名问题。v4 经代码核验补充。

经核验 aigw 当前可观测性栈，逐项结论：

| 层 | 是否含 `request_id` | 是否需同步改名 | 依据 |
|----|---------------------|---------------|------|
| **Prometheus 指标**（`aigw-core/src/metrics.rs`） | 否（14 个指标家族 label 仅 model/user/status_code/error_type/api_base/token_type） | 否 | 无 label 受影响 |
| **OpenTelemetry span attribute**（`aigw-core/src/otel_tracing.rs`） | 否（仅做 W3C traceparent 注入/提取，无 `set_attribute` 设 request_id） | 否 | grep `set_attribute` 全 crate 零命中 |
| **tracing span 字段**（`main.rs:110-130 RequestIdMakeSpan`） | **是**：`tracing::span!(... "request", request_id = %request_id, method, uri)` | **需处置**（见下） | 配合 `main.rs:170` JSON fmt layer，所有请求作用域内 tracing 事件都带 `request_id` 字段 |
| **结构化日志 message 文本**（`chat.rs:1074-1080`、`v1_messages.rs:631-637`） | 是：`tracing::warn!("mismatch request_id: ours={} theirs={}")` | 可保留（属 HTTP 头 mismatch 语义，非 DB 字段） | 该 `request_id` 指出站请求头，与 §2.2 HTTP 层同源 |
| **响应头回写客户端** | 否（未配置 `PropagateRequestIdLayer`，grep 零命中） | 否（但提示对账缺口，见下） | 客户端无法从响应头拿到 aigw 调用 ID |

**需处置项 — tracing span 字段语义不一致**：

`main.rs:123-130` 的 `RequestIdMakeSpan` 把 HTTP 层 `request_id`（值=aigw 生成的 UUID v7，与即将改名的 DB `spend_logs.request_id` 同值）记为 span 字段 `request_id`。DB 改名后：
- 日志里每条请求事件的字段名仍叫 `request_id`，而 DB 同值字段叫 `call_id`；
- 日志 reader 会误以为该字段是「上游 provider 的 request_id」（行业惯例语义），而实际是网关调用 ID。

**处置方案（二选一，推荐 A）**：

- **方案 A（推荐）**：span 字段同步改名为 `call_id = %request_id`，与 DB 字段语义对齐。改动点仅 `main.rs:126` 一处（`request_id = %request_id` → `call_id = %request_id`），变量名保留 `request_id`（HTTP 层，§2.2）。代价：日志字段名变更，若有下游日志采集/告警规则按 `request_id` 过滤需同步（aigw 未上线，无存量依赖）。
- **方案 B**：保留 `request_id` 字段名，额外加 `call_id` 别名字段（`request_id = %request_id, call_id = %request_id`），双写过渡。代价：日志冗余，但向后兼容。

> 无论哪种，`chat.rs`/`v1_messages.rs` 里 `tracing::warn!("mismatch request_id: ...")` 的 message 文本可保留不改——它描述的是 HTTP 出站头 mismatch，属 §2.2 HTTP 层语义，与 DB 字段无关。

**对账缺口提示（非本次改动范围，记录供后续跟进）**：aigw 未用 `PropagateRequestIdLayer` 把 `x-request-id` 回写响应头，客户端无法从响应拿到调用 ID 对账。本次改名后，若要让客户端用 `call_id` 对账，需后续单独加 `PropagateRequestIdLayer`（或自定义响应头 `x-call-id`）。不阻塞本次发布。

---

## 11. v6.1 Gate-2 评审增量（2026-07-27）

> Gate-2 多模型评审（lead 独立 + 3 路 subagent：migration / migrate-frontend-tracing-tests / extraction-protocol）发现 6 项需在实施前补齐的设计缺陷。详见 `docs/research/2026-07-27-stage-85-design-review-consolidated.md`。本节为权威增量，覆盖前文与之冲突的描述。

### 11.1 export override 被 direct-match 抢占（Lens B C2 — High，静默数据丢失）

§4.5 的 export 反向 override `["request_id"] = "call_id"` **不会被消费**。`insert_rows`（`native.rs:1222-1236`）的查找顺序是 **direct match 优先，override 仅作 fallback**：

```rust
let v = row_map.get(col_name.as_str())           // (1) direct: row_map[target_col]
    .or_else(|| column_override.get(col_name.as_str())   // (2) fallback only
        .and_then(|mapped| row_map.get(mapped.as_str())))
```

export 场景：aigw 源有 `call_id` + `request_id`；litellm 目标只有 `request_id`。对目标 `request_id`：direct match `row_map["request_id"]` 命中 aigw 的上游 `request_id`（按 §3.5 历史 litellm 导入行该列为 NULL）→ 把 **NULL 写进 litellm 的 `request_id` PK**。override 永远到不了。

**修复**：在 `remote_export.rs:384` `insert_rows` 调用前，**从每条 aigw 源行剥离 `request_id` 列**（使 direct match 失败、override 生效）；或改 `insert_rows` 让 override 优先（侵入其他表，不推荐）。本设计采用**源行剥离**：

```rust
// remote_export.rs migrate_spend_logs，insert_rows 前：
let rows_stripped: Vec<UnifiedRow> = rows.iter().map(|r| {
    r.iter().filter(|(n, _)| n != "request_id").cloned().collect()
}).collect();
// 再 overrides.insert("request_id".to_string(), "call_id".to_string())
// insert_rows(target, "LiteLLM_SpendLogs", &tgt_col_info, &rows_stripped, &overrides)
```

### 11.2 失败路径 upstream_id 走 INSERT，非 UPDATE（Lens C C1 — Critical，核心预期 breaker）

§4.3 v5 把失败路径 ×3 当作 `update_spend_log` 调用点，**错误**。实际三处失败路径只调 `insert_spend_log(&sl)`：
- `chat.rs:1047`（超时）、`chat.rs:1148`（流式 4xx/5xx）、`chat.rs:1518`（非流式 4xx/5xx）
- `v1_messages.rs:421/609/705` — 全是 `insert_spend_log`

`COALESCE($new, request_id)` UPDATE 保护**不覆盖**失败行 → 失败 `request_id` 仍 NULL → **v5 核心预期"失败请求也能对账"静默失败**。

**修复**：失败路径把 `upstream_id` 直接放进 `SpendLog.request_id` 字段在 INSERT 时写入：

```rust
// chat.rs / v1_messages.rs 失败分支：
let fail_upstream_id = extract_upstream_id_from_error(&error_body, &resp_headers);
let sl = SpendLog {
    call_id: fail_request_id,
    request_id: fail_upstream_id,   // ← INSERT 时即写入，不走 UPDATE
    ...
};
state.db.insert_spend_log(&sl).await;
```

`update_spend_log` 的 `upstream_request_id` 参数 + `COALESCE` 保护**仅用于流式 Phase 2 UPDATE**（成功路径：首 chunk 提取 id 后回填）。`update_spend_log` 实际调用点 3 处（非设计原称的 5 处）：`chat.rs:1359`（流式失败 Phase 2）、`chat.rs:1385`（流式成功 Phase 2）、`v1_messages.rs:897`（流式成功 Phase 2）。

### 11.3 Anthropic 流式提取位置（Lens C C2 — High）

§4.3 v4 伪代码"在 push chunk_jsons 的同一循环体内提取"。但 `v1_messages.rs:814` 只在 `raw.get("choices")` 非空时 push；Anthropic-native `message_start` 事件无 `choices` → push 不触发 → 提取永远不执行。且 `raw` 在 `push(raw)` 被 move，提取放 push 之后是 use-after-move。

**修复**：提取放在 `serde_json::from_str::<Value>(data)` 成功后、`if choices` 分支**之前**，用 `raw.get("message")`（borrow，不 move）：

```rust
// v1_messages.rs:805 之后、:814 if choices 之前：
if upstream_id.is_none() {
    if let Some(msg) = raw.get("message") {
        if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
            upstream_id = Some(id.to_string());
        }
    }
}
// 再走原有 if choices 分支 push(raw)
```

chat.rs（OpenAI）同样放 push 之前 borrow 提取，但 OpenAI chunk 恒有 `choices`，hazard 较低。

### 11.4 Anthropic 失败响应头须预提取 request-id（Lens C C4 — Medium）

§4.3 v5 Anthropic 失败 fallback 到响应头 `request-id` / `x-request-id`。但 `v1_messages.rs:713/949` 的 `upstream_resp.text().await` **消费了 `upstream_resp`**，之后 `upstream_resp.headers()` 不可达。预提取的 `upstream_req_id`（`:624-628`）只有 `x-request-id`，没有 Anthropic 官方头 `request-id`。

**修复**：在 `:624-628` 预提取 `x-request-id` 时一并预提取 `request-id`：

```rust
// v1_messages.rs:624-628 扩展：
let upstream_req_id = upstream_resp.headers().get("x-request-id")
    .or_else(|| upstream_resp.headers().get("request-id"))  // ← 新增
    .and_then(|v| v.to_str().ok()).map(|s| s.to_string());
```

失败 fallback 链：error body `request_id` 字段 → `upstream_req_id`（含 `request-id`/`x-request-id`）。

### 11.5 双列搜索 bind 机械 per-impl（Lens C C5 — Medium）

§4.2 `WHERE call_id LIKE $1 OR request_id LIKE $1` 未说明 per-impl 差异 + Postgres count 占位符计数 hazard。

| impl | 位置 | 改法 |
|------|------|------|
| Sqlite filtered | `db.rs:1482-1521` | SQL-level：`AND (call_id = ? OR request_id = ?)`，bind 两次 |
| Sqlite count | `db.rs:1534-1548` | 同上 |
| Mysql count | `db.rs:1826-1851` | 同上 |
| Postgres count | `db.rs:2107-2132` | SQL-level，但占位符用计数器 `i`，`OR` 要 `i` 递增两次（连续 push `$i` `$i+1` 再 `i+=2`） |
| Mysql filtered | `db.rs:1788-1814` | **内存过滤**非 SQL：filter closure 改 `log.call_id != rid && log.request_id != rid` |
| Postgres filtered | `db.rs:2070-2095` | 同 Mysql filtered，内存过滤 |

### 11.6 migrate NULL 语义澄清（Lens B M1 — Medium）

§3.5 "历史调用从未存储上游 ID" 误判。litellm 的 `request_id` **就是**上游 provider id（§1.2 `response_obj.get("id")`）。修正 import override 后，litellm 源 `request_id` 经 direct match **同时**写入 aigw 目标 `call_id`（PK，经 override）+ 目标 `request_id`（上游列，经 direct match）——**两列同值，语义正确**，不是缺陷。

§3.5 修订：migrate 导入的存量行，`call_id` = `request_id` = litellm 原 `request_id`（同值）。售后对账历史记录时，用任一列都能回查 litellm。**不再追求上游列 NULL**（既做不到也不该做）。

### 11.7 实施时 line-number 修正（Lens B L1 / Lens C C6/C8 — Low，记录给实施者）

- `remote_import.rs` aigw `spend_logs` 测试夹具实际在 `:1243-1260`（非 `:1224`，:1224 在 litellm 夹具内）。
- chat.rs 非流式 success SpendLog 在 `:1536-1582`（非 `:1184`，:1184 是流式 Phase 1 占位 INSERT）。
- `update_spend_log` 共 5 处（trait `:1192` + Sqlite `:1341` + Mysql `:1629` + Postgres `:1904` + Database dispatch `:2166`），非"3 处"。
- chat.rs 对外响应 body **不含** `request_id` 字段（仅 `v1_messages.rs` 的 `anthropic_error` 注入）——§6.3 该条事实有误，但属"不改"边界，无实施影响。
- MySQL `RENAME COLUMN` 是本仓首次使用的新模式（现有 018 用 DROP+rebuild）；目标 8.4 支持，但 README 应显式标注最低 MySQL 8.0 / MariaDB 10.5.2。
- parquet reader（`body_archive/query.rs:60-70`）按 **projection mask 列名** 定位、`batch.column(N)` 按位置取值。`BodyRow.request_id` 改名后，projection 需先尝试 `call_id` 再 fallback `request_id`，否则读旧 parquet 文件会找不到列。§4.4 处置（清空存量 + reader 列名兼容）正确。

### 11.8 §7 步骤同步

- 步骤 7（chat.rs）：失败路径 `SpendLog{ call_id, request_id: fail_upstream_id, .. }` 在 INSERT 写入（非补 UPDATE 参数）；流式 Phase 2 UPDATE 才补 `upstream_id`。
- 步骤 8（v1_messages.rs）：同上；额外 `:624-628` 预提取 `request-id` 头；流式提取放 `if choices` 之前 borrow。
- 步骤 12（migrate）：import override `("call_id","request_id")` + export 源行剥离 `request_id` + export override `("request_id","call_id")`。

**§7 步骤补充**：在步骤 10（改 `main.rs`）后追加一步——「处置 `main.rs:126` tracing span 字段（§10 方案 A：`request_id` → `call_id`）」，耗时 5min。
