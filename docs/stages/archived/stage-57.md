# Stage 57: Spend Logs 下拉过滤器 + model_group 修复 + User Agent + device_id

**Phase**: 20 — Spend Logs 可观测性（过滤器增强 + Overhead 评估 + 修复）
**状态**: ⏳ 待开始
**预估**: 7-8h
**依赖**: 无硬依赖（可与 Stage 55/56/58 并行）

---

## 目标

1. **model_group/custom_llm_provider/model_id 修复** — 将 resolve 时已查到的 proxy_models 数据写入 SpendLog
2. **Model 过滤器改为下拉** — 新增 distinct-models API + Select 组件
3. **Session ID 下拉过滤器** — 新增 distinct-sessions API + Select 组件
4. **User Agent 提取与展示** — 从 HTTP header 提取写入 metadata.user_agent
5. **device_id 提取与展示** — 从 metadata.user_id JSON 解析写入 metadata.device_id

## 根因摘要

- `chat.rs` 和 `v1_messages.rs` 中所有 4 个 SpendLog 创建点都写了 `model_group: None, custom_llm_provider: None, model_id: None`，但 resolve 时已从 proxy_models 查到这些数据，只是未写入 SpendLog
- Model 过滤器是文本框（`<Input placeholder="Model filter…">`），无法直观看到可选模型
- `session_id` 已在 DB 中存储，但无过滤 UI
- `user_agent` 和 `device_id` 从未提取/存储

## 验收标准

- [ ] resolve 后 `model_id`（proxy_models UUID）、`model_group`（litellm_params.model 上游模型名）、`custom_llm_provider` 正确写入 SpendLog
- [ ] `GET /global/spend/logs/distinct-models` 返回所有唯一模型名
- [ ] `GET /global/spend/logs/distinct-sessions` 返回所有唯一 session_id
- [ ] Model 过滤器改为下拉选择（searchable），默认 "All Models"
- [ ] Session ID 过滤器改为下拉选择，默认空
- [ ] `user_agent` 从 User-Agent HTTP header 提取，存入 `metadata.user_agent`
- [ ] `device_id` 从 metadata.user_id JSON 解析，存入 `metadata.device_id`
- [ ] DetailDrawer metadata 区显示 User-Agent 和 Device ID
- [ ] SpendLogsQuery 新增 `session_id` 过滤参数
- [ ] `query_spend_logs_filtered` 增加 session_id 过滤
- [ ] **门禁**: 全量 UT + BDD + 前端 Playwright（4 个 scenario × 3 viewports）

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs` | **修改** — model_group/custom_llm_provider/model_id 写入；user_agent/device_id 提取 |
| `crates/aigw-server/src/routes/v1_messages.rs` | **修改** — 同上 |
| `crates/aigw-server/src/routes/spend.rs` | **修改** — 新增 distinct-models/sessions API；SpendLogsQuery 扩展 session_id；响应增加 user_agent/device_id |
| `crates/aigw-core/src/db.rs` | **修改** — query_spend_logs_filtered 增加 session_id 过滤；query_spend_logs_count 同步扩展 |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | **修改** — model/session 过滤器改为下拉；类型扩展；DetailDrawer 展示 UA/device_id |

## 技术方案

### 1. model_group/custom_llm_provider/model_id 写入修复

在 `chat.rs` 的 resolve 阶段后，将 ProxyModel 信息传递给 SpendLog 构造：

```rust
// 当前（chat.rs resolve 后已有 these fields）:
// proxy_model.model_id, params_json["model"], params_json["custom_llm_provider"]

// SpendLog 构造改为：
let sl = SpendLog {
    model_id: proxy_model_id.clone(),     // 原来: None
    model_group: upstream_model.clone(),   // 原来: None  → 改为 litellm_params.model 的上游标识
    custom_llm_provider: provider.clone(), // 原来: None  → 改为 "openai" / "anthropic" 等
    // ...
};
```

影响范围：
- `chat.rs`: 4 个 SpendLog 构造点（流式成功/失败 + 非流式成功/失败）
- `v1_messages.rs`: 同上模式

### 2. distinct-values API

```rust
/// GET /global/spend/logs/distinct-models
pub async fn global_spend_distinct_models(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let models = state.db.get_distinct_models().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR,
         Json(json!({"error": {"message": format!("{}", e)}})))
    })?;
    Ok(Json(json!({ "data": models })))
}

/// GET /global/spend/logs/distinct-sessions
pub async fn global_spend_distinct_sessions(…) {
    // SELECT DISTINCT session_id FROM spend_logs WHERE session_id IS NOT NULL ORDER BY session_id
}
```

### 3. User Agent 提取（对齐 litellm）

```rust
// 在 chat.rs / v1_messages.rs handler 中
let user_agent = headers
    .get("user-agent")
    .and_then(|v| v.to_str().ok())
    .map(|s| s.to_string());

// 写入 metadata JSON（对齐 litellm，不新建列）
let mut metadata_map = serde_json::Map::new();
if let Some(ref ua) = user_agent {
    metadata_map.insert("user_agent".to_string(), json!(ua));
}
if let Some(ref did) = device_id {
    metadata_map.insert("device_id".to_string(), json!(did));
}
let metadata = if metadata_map.is_empty() {
    None
} else {
    Some(Value::Object(metadata_map))
};
```

### 4. 前端 Filter 改造

```tsx
// 旧:
<Input placeholder="Model filter…" value={modelFilter}
  onChange={(e) => { setModelFilter(e.target.value); setPage(1); }} />

// 新:
const { data: distinctModels } = useQuery({
  queryKey: ["distinct-models"],
  queryFn: () => apiGet("/global/spend/logs/distinct-models"),
});

<Select value={modelFilter} onValueChange={(v) => { setModelFilter(v); setPage(1); }}>
  <SelectTrigger className="w-40 h-8">
    <SelectValue placeholder="All Models" />
  </SelectTrigger>
  <SelectContent>
    <SelectItem value="">All Models</SelectItem>
    {(distinctModels?.data ?? []).map((m: string) => (
      <SelectItem key={m} value={m}>{m}</SelectItem>
    ))}
  </SelectContent>
</Select>
```

### 5. DetailDrawer metadata 展示

```tsx
// 在 DetailDrawer 的 Metadata 区新增：
{log.user_agent && (
  <div>
    <span className="text-muted-foreground">User Agent:</span>
    <span className="text-xs truncate block">{log.user_agent}</span>
  </div>
)}
{log.device_id && (
  <div>
    <span className="text-muted-foreground">Device ID:</span> {log.device_id}
  </div>
)}
```

## TDD 测试用例

### UT (Rust)

```rust
#[test]
fn test_model_group_populated_in_spend_log() {
    // resolve 时 proxy_models.litellm_params.model = "gpt-4"
    // assert: spend_log.model_group == Some("gpt-4")
}

#[test]
fn test_custom_llm_provider_populated() {
    // proxy_models.litellm_params.custom_llm_provider = "openai"
    // assert: spend_log.custom_llm_provider == Some("openai")
}

#[test]
fn test_user_agent_extracted_from_headers() {
    // headers = {"user-agent": "Claude-Code/1.0"}
    // assert: metadata.user_agent == "Claude-Code/1.0"
}

#[test]
fn test_device_id_from_metadata_user_id_json() {
    // metadata.user_id = "{\"device_id\":\"d1\",\"session_id\":\"s1\"}"
    // assert: metadata.device_id == "d1"
}

#[test]
fn test_distinct_models_returns_unique_list() {
    // DB 有 3 条日志: gpt-4, gpt-3.5, gpt-4
    // assert: distinct models == ["gpt-3.5", "gpt-4"]
}
```

### BDD (Gherkin)

```gherkin
Scenario: Model filter dropdown filters spend logs
  Given spend logs 页面已加载
  When 从 model 下拉选择 "gpt-4"
  Then 表格仅显示 model 为 "gpt-4" 的记录

Scenario: Session ID filter filters spend logs
  Given spend logs 页面有 session_id 不同的记录
  When 从 session 下拉选择某个 session_id
  Then 表格仅显示该 session 的记录

Scenario: User Agent displayed in detail drawer
  Given 发送 /v1/chat/completions 请求，User-Agent = "Claude-Code/1.0"
  When 在 spend logs 页面点击该日志
  Then 详情中显示 "User Agent: Claude-Code/1.0"

Scenario: Device ID displayed in detail drawer
  Given 请求 metadata.user_id = "{\"device_id\":\"abc123\"}"
  When 在 spend logs 详情中查看
  Then Metadata 区显示 "Device ID: abc123"
```

## 风险与回滚

| 风险 | 应对 |
|------|------|
| model_group 从 litellm_params.model 提取可能为空 | 空时 fallback 为空字符串，不影响展示 |
| distinct-models 在大量日志时查询慢 | SQLite 有 model 索引（需确认），否则加 index |
| 旧日志无 user_agent/device_id | metadata 为 None 时不展示，前端优雅降级 |
| metadata.user_id JSON 解析失败 | fallback 为 None，不影响 end_user 原始值 |

回滚方式：`git revert` 该 commit。
