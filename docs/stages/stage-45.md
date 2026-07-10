# Stage 45: Spend Logs 抽屉完整内容 + CSV 导出 + 布局优化

**Phase**: 15 — 第二轮反馈改进
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 5h

---

## 目标

1. 详情抽屉展示完整请求/响应内容，包括 messages（提示词）、response（响应内容）、请求参数、metadata、工具定义等
2. 新增 CSV / Excel 导出按钮，支持将当前时间范围内的日志数据导出
3. 布局优化：Request ID / Model 搜索框与 Time Range 行对齐

## 验收标准

- [ ] 抽屉展示 Messages 区域（按 role 分组的提示词/响应对话列表）
- [ ] 抽屉展示 Response 内容（Markdown 或 JSON 渲染）
- [ ] 抽屉展示 Request Parameters（model、temperature、max_tokens、stream 等）
- [ ] 抽屉展示 Metadata（API Key 前缀、User、Team、Org、Tags、Session ID、Tools/Tool Calls）
- [ ] 抽屉展示 Model Info（model_id、pricing、custom_llm_provider）
- [ ] CSV 导出按钮在 Toolbar 中可见
- [ ] 导出文件包含完整的列（Request ID / Time / Type / Model / Status / TTFT / Duration / Tokens / Cost / User / API Key）
- [ ] 文件名格式：`spend-logs-{startDate}-{endDate}.csv`
- [ ] Request ID 搜索框和 Model 过滤框宽度与其他控件对齐
- [ ] BDD：抽屉完整内容、CSV 导出、移动端抽屉

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/spend.rs` | 修改：响应中返回 `messages` / `response` blob 字段 |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 修改：DetailDrawer 内容增强 + 导出按钮 + 布局 |

## 技术方案

### A. 后端字段返回

当前 `/global/spend/logs` 响应中每个日志项未返回 `messages` 和 `response` JSON blob（出于体积考虑）。需要修改 spend 响应中的 data 构建：

```rust
json!({
    ...
    "messages": log.messages,   // ← 已有 DB 列，当前未返回到 API
    "response": log.response,   // ← 同上
    "session_id": log.session_id,
    "request_tags": log.request_tags,
    "custom_llm_provider": log.custom_llm_provider,
    "team_id": log.team_id,
    "organization_id": log.organization_id,
    "end_user": log.end_user,
    "user": log.user,
})
```

**性能考量**: `messages` 和 `response` 是 TEXT/JSON blob，可能较大。仅当 `page_size <= 50` 时返回（防止单次响应过大）。或新增 query param `?include_body=true`，默认 false（Spend Logs 页面打开抽屉时才发送第二个请求获取详情）。

**推荐**: 新增 `GET /global/spend/logs/{request_id}` 单个日志详情端点，抽屉打开时按需获取。这样不影响列表分页响应大小。

```rust
/// GET /global/spend/logs/{request_id} — 单个日志详情
pub async fn global_spend_log_detail(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let log = state.db.get_spend_log_by_request_id(&request_id).await?;
    Ok(Json(serde_json::to_value(&log)?))
}
```

### B. 抽屉内容增强

详情抽屉新增以下区域：

```
┌─ Request Details ──────────────────────┐
│ [Status Badge] [Type Badge]             │
│ Model: xxx (group: xxx)                 │
│ Cost: $x.xxxx (input $x / output $x)    │
│ Tokens: prompt 1,234 / completion 567   │
│ TTFT: 234ms  │  Duration: 1.2s         │
│                                         │
│ 📝 Messages                             │
│ ┌─ system ──────────────────────────┐   │
│ │ You are a helpful assistant       │   │
│ └───────────────────────────────────┘   │
│ ┌─ user ────────────────────────────┐   │
│ │ What is Rust?                     │   │
│ └───────────────────────────────────┘   │
│                                         │
│ 🤖 Response                             │
│ [Markdown rendered or JSON viewer]      │
│                                         │
│ 🔧 Request Parameters                   │
│ model / temperature / max_tokens / ...  │
│                                         │
│ 🏷️ Metadata                             │
│ API Key / User / Team / Org / Session   │
│ Tags / Tools / MCP Tools                │
└─────────────────────────────────────────┘
```

### C. CSV 导出

前端实现（无需后端改动）：

```typescript
function exportToCSV(logs: SpendLog[], filename: string) {
  const headers = [
    "Request ID", "Time", "Type", "Model", "Status",
    "Prompt Tokens", "Completion Tokens", "Total Tokens",
    "TTFT (ms)", "Duration (ms)", "Cost", "User", "API Key"
  ];
  const rows = logs.map(log => [
    log.request_id,
    log.start_time,
    log.call_type,
    log.model,
    log.status ?? "",
    log.prompt_tokens,
    log.completion_tokens,
    log.total_tokens,
    log.ttft_ms ?? "",
    log.request_duration_ms ?? "",
    log.spend,
    log.user ?? "",
    log.api_key.slice(0, 12) + "…"
  ]);
  const csv = [headers, ...rows].map(r => r.map(v => `"${v}"`).join(",")).join("\n");
  const blob = new Blob(['﻿' + csv], { type: "text/csv;charset=utf-8" }); // BOM for Excel
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url; a.download = filename; a.click();
  URL.revokeObjectURL(url);
}
```

导出范围：建议 fetch 所有满足当前筛选条件的数据（`page_size = total_count` 但需限制上限如 10,000 条以防止超时），或导出当前页。

### D. 布局优化

```
Toolbar 行:
  [🔍 Request ID    ]  [Model filter   ]  [📥 Export CSV]  [🔄 Fetch]

对齐要求: Request ID 输入框 w-44, Model filter w-40, 与上方 Time Preset 行视觉对齐。
```

## 依赖

- Stage 34（后端 spend logs 增强）
- Stage 36（前端 Spend Logs 页面）

## 风险

- `messages` / `response` blob 可能很大（单个请求可达几十 KB），拆分详情端点方案最安全
- CSV 前端导出受浏览器内存限制，数据量大需分批次或改为后端导出
