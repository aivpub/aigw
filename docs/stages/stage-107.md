# Stage 107: Handler Integration + SpendLog + Migration 025 + BDD

**Phase**: 43 — Image Token Usage Tracking
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: Stage 106（Core Engine）
**后置**: Stage 108（前端 + Real API BDD + Docs）

---

## 核心预期

1. **chat.rs / v1_messages.rs handler 集成**：在上游 response 解析 + SpendLog 写入路径中接入 image_tokens 引擎。"上游优先 + fallback 估算" 模式。
2. **DB Migration 025**：`spend_logs` + 6 张 `daily_*_spend` 表新增 `image_tokens` 列。
3. **DailySpendLog 聚合**：daily_spend_queue 写入 image_tokens。
4. **BDD 覆盖**：8 个 mock BDD 场景验证上游解析、fallback 估算、多图片、daily 聚合、向后兼容。
5. **零回归**：`calc_spend` 不改动 — image_tokens 是 prompt_tokens 的子集。

---

## 背景

Stage 106 提供了 `extract_image_tokens_from_usage()` 和 `estimate_image_tokens_from_body()` 两个函数。本 Stage 将它们接入实际的请求-响应-SpendLog 写入链路，使每条多模态请求的 SpendLog 都包含 `image_tokens` 字段。

核心逻辑：
```
image_tokens = extract_from_usage(upstream_response)
    .or_else(|| estimate_from_body(request_body, model_name))
```

计费策略明确：image_tokens 不改 `calc_spend`。image_tokens 是 prompt_tokens 的子集，上游已经按总 prompt_tokens 收费。新增字段仅用于分析与对账。

---

## 设计

### 1. Handler 集成 (chat.rs)

**非流式路径**（参考现有 `extract_cache_read_tokens` / `extract_cache_creation_tokens` 位置）：

```rust
// ━━━ After parsing usage from upstream response ━━━

// Try upstream first
let image_tokens = image_tokens::extract_image_tokens_from_usage(&usage);
let mut image_tokens_source: Option<&str> = if image_tokens.is_some() { Some("upstream") } else { None };

// Fallback estimate
let image_tokens = image_tokens.or_else(|| {
    let est = image_tokens::estimate_image_tokens_from_body(
        &body, &deployment.upstream_model,
    );
    if est > 0 {
        image_tokens_source = Some("estimated");
        Some(est as i32)
    } else {
        None
    }
});

// ━━━ In SpendLog construction ━━━
let mut meta = existing_metadata.unwrap_or(json!({}));
if let Some(src) = image_tokens_source {
    meta["image_tokens_source"] = json!(src);
}
```

**流式路径**：
- Phase 1 INSERT：`image_tokens = NULL`（streaming 开始前 usage 未知）
- Phase 2 UPDATE：收到 final usage chunk 后，按上述逻辑填充 `image_tokens` + `image_tokens_source`

### 2. Handler 集成 (v1_messages.rs)

Anthropic protocol：

```rust
// Anthropic usage has no image_tokens → extract always returns None
let image_tokens = image_tokens::extract_image_tokens_from_usage(&usage)
    .or_else(|| {
        let blocks = request_body.get("messages").and_then(|v| v.as_array())
            .cloned().unwrap_or_default();
        let est = image_tokens::estimate_image_tokens_from_blocks(
            &blocks, &deployment.upstream_model,
        );
        if est > 0 { Some(est as i32) } else { None }
    });
```

### 3. DB Migration 025

三个方言文件（`data/sqlite/025_image_tokens.sql`, `data/postgres/025_image_tokens.sql`, `data/mysql/025_image_tokens.sql`）：

```sql
-- 025_image_tokens.sql

ALTER TABLE spend_logs ADD COLUMN image_tokens INTEGER;
ALTER TABLE daily_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_team_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_organization_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_end_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_agent_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_tag_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
```

- SQLite: `INTEGER` 而非 `BIGINT`
- MySQL: `BIGINT` + 尾部 `;` 一致
- PostgreSQL: `BIGINT` + 尾部 `;` 一致
- Migration 号 025（前一 migration `023_start_time_is_millis`）

### 4. SpendLog Model 变更

```rust
// models.rs — add to SpendLog:
/// Image tokens consumed in this request.
/// Source: upstream value (Qwen/Gemini) → client-side estimate (OpenAI/Anthropic) → NULL (unknown).
/// Stored in metadata.image_tokens_source as "upstream" or "estimated".
pub image_tokens: Option<i32>,

// models.rs — add to DailySpendLog:
/// Accumulated image tokens per day.
pub image_tokens: i64,
```

### 5. daily_spend_queue 变更

跟随 Stage 90 cache_tokens 模式：

- `BatchEntry` 新增 `image_tokens: i64` 字段
- `aggregate_daily_spend()` 累加 `image_tokens`
- `batch_upsert_daily_spend()` SQL 三方言 INSERT/UPDATE 包含 `image_tokens` 列

### 6. metadata 增强

```json
{
  // ... existing fields ...
  "image_tokens_source": "upstream"
}
```

- 不在 `spend_logs` 上新增独立列 —— metadata JSON 已有，只在对账时需要溯源
- `image_tokens_source` 取值：`"upstream"` | `"estimated"` | absent（无图片请求）

---

## 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/models.rs` | 修改 | SpendLog + DailySpendLog 新增 image_tokens |
| `crates/aigw-core/src/daily_spend_queue.rs` | 修改 | BatchEntry + SQL 写入 |
| `crates/aigw-core/src/db.rs` | 修改 | insert_spend_log / update_spend_log / select 包含 image_tokens |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | 非流式 + 流式路径集成 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改 | Anthropic 路径集成 |
| `data/sqlite/025_image_tokens.sql` | **新建** | SQLite 迁移 |
| `data/postgres/025_image_tokens.sql` | **新建** | PostgreSQL 迁移 |
| `data/mysql/025_image_tokens.sql` | **新建** | MySQL 迁移 |
| `crates/aigw-server/tests/features/spend.feature` | 修改 | 8 BDD 场景 |
| `crates/aigw-server/tests/bdd_steps/spend_steps.rs` | 修改 | Step 实现 |

---

## BDD Scenario（8 个）

```gherkin
Feature: Image Token Tracking

  Scenario: Qwen returns image_tokens — stored as upstream
    Given 一个 qwen2.5-vl 模型已配置
    And 上游返回 prompt_tokens_details.image_tokens = 400
    And 请求体包含一张 base64 图片
    When 发送 POST /v1/chat/completions 请求
    Then SpendLog 中 image_tokens 为 400
    And metadata.image_tokens_source 为 "upstream"

  Scenario: OpenAI doesn't return — fallback estimate used
    Given 一个 gpt-4o 模型已配置
    And 上游不返回 image_tokens
    And 请求体包含一张 1024x1024 的 base64 JPEG 图片
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 765
    And metadata.image_tokens_source 为 "estimated"

  Scenario: Anthropic doesn't return — fallback estimate used
    Given 一个 claude-sonnet 模型已配置
    And messages 请求包含一个 image content block
    When 发送 POST /v1/messages 请求
    Then SpendLog 中 image_tokens > 0
    And metadata.image_tokens_source 为 "estimated"

  Scenario: Text-only request has NULL image_tokens
    Given 一个 qwen2.5-vl 模型已配置
    And 请求体为纯文本
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 NULL

  Scenario: Multiple images summed correctly
    Given 一个 gpt-4o 模型已配置
    And 请求体包含 3 张 512x512 的 base64 图片
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 255

  Scenario: Daily spend aggregation includes image tokens
    Given 3 个带图片的请求已发送（image_tokens = 100 each）
    When daily_spend 队列刷新
    Then daily_user_spend 的 image_tokens 为 300

  Scenario: Old SpendLog records are NULL
    Given 一条 image_tokens 功能上线前的支出记录
    When 查询该记录
    Then image_tokens 字段为 NULL

  Scenario: Streaming path writes image_tokens in Phase 2 UPDATE
    Given 一个 gpt-4o 模型已配置
    And 请求体包含图片
    And 请求使用 streaming=true
    When 流式响应完成
    Then SpendLog 中 image_tokens > 0
```

### MockUpstream 扩展

需要在 `mock_upstream.rs` 中支持：
1. Mock 上游返回 `prompt_tokens_details.image_tokens`（模拟 Qwen）
2. Mock 上游不返回 `image_tokens`（模拟 OpenAI，当前默认行为）

---

## 关键决策

1. **Qwen 不触发估算**：`extract` 先运行，Qwen 返回 image_tokens → 直接使用。fallback 永远不会对 Qwen 触发。
2. **Anthropic 的估算用 OpenAI 公式近似**：Claude 的 tile 公式与 OpenAI 类似但未公开确切参数。用 OpenAI 公式做近似估算，标记 source="estimated"。
3. **流式路径 Phase 1 INSERT 写 NULL**：streaming 开始时 usage 未知。Phase 2 UPDATE 时填充。
4. **metadata 存 source，不建独立列**：metadata 已有 JSON 字段，只在对账时溯源，不需要索引。

---

## Migrate 影响

`aigw-migrate` 跨实例同步工具：`spend_logs` 和 6 张 `daily_*_spend` 的 `DEFAULT_TABLES` 列表中已包含这些表。新增列后，`INSERT OR IGNORE` / `ON CONFLICT` 同步自动携带 `image_tokens` 列（值来自源表）——无需改 migrate 代码。

---

## TDD

新增 UT（aigw-server）：
- `test_image_tokens_upstream_priority` — extract 有值时跳过估算
- `test_image_tokens_fallback_estimate` — extract 无值且 request 有图片 → 估算写入
- `test_image_tokens_text_only_null` — 无图片 → NULL
- `test_image_tokens_source_metadata` — metadata 标记正确

BDD：8 个 mock 场景。

---

## Gate 门禁

- `task check` 通过
- `task test` aigw-core + aigw-server 全量 UT 绿
- `task bdd` mock BDD 8 新场景绿 + 全量回归
- `task bdd-real-sqlite` 三后端 real BDD 绿（验证 migration 应用）
