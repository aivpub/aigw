# Spend Logs & Usage 页面问题根因分析与修复方案

> 日期: 2026-07-15

---

## 问题 1: Spend Logs 页面的 15 分钟/4 小时过滤器无效

### 现象

前端点击"15 min"或"4 hours"预设按钮后，API 请求的时间参数为本地时区格式：
```
/global/spend/logs?start_date=2026-07-15T06:34:38&end_date=2026-07-15T10:34:38
```
但数据没有按这个时间范围正确过滤。

### Litellm 对比分析

Litellm UI 也有同样的问题。其前端发送给后端的请求格式为：
```
/spend/logs/ui?start_date=2026-07-15+03%3A22%3A00&end_date=2026-07-15+03%3A37%3A58&page=1&page_size=50&sort_by=startTime&sort_order=desc
```
即 `start_date=2026-07-15 03:22:00&end_date=2026-07-15 03:37:58`（URL 编码的空格和冒号），时间参数用的是**本地时区时间**（无 UTC 标记）。

### 根因

**两个层面的问题：**

**第一层：前端发送本地时区时间。** `presetRange()`（`spend-logs/index.tsx:105-119`）使用 `date-fns` 的 `format()` 函数：

```ts
function presetRange(p: TimePreset): { start: string; end: string } {
  const now = new Date();  // 本地时区
  switch (p) {
    case "15m":
      return { start: format(subMinutes(now, 15), "yyyy-MM-dd'T'HH:mm:ss"),
               end: format(now, "yyyy-MM-dd'T'HH:mm:ss") };
    // ...
  }
}
```

`format(new Date(), "yyyy-MM-dd'T'HH:mm:ss")` 输出的是**本地时区时间**（如北京时间 `2026-07-15T10:34:38`），不带时区后缀。而数据库中 `start_time` 列存的是 UTC（`Utc::now()`）。

**第二层：后端不做时区转换。** `query_spend_logs_filtered()`（`db.rs:2100-2125`）直接把前端传过来的日期字符串绑定到 SQL：

```sql
WHERE start_time >= ? AND start_time <= ?
-- bind: '2026-07-15T10:34:38'（前端本地时间）vs '2026-07-15T02:34:38Z'（数据库 UTC）
```

如果用户在 UTC+8 时区（如北京），15 分钟过滤器实际会查询 UTC 时间 02:34~10:34，范围跨度 8 小时而非 15 分钟。

**Litellm 代码库对比：** litellm 后端接受日期参数后会用 datetime 库解析，并且有一些时间转换逻辑处理这个偏移。但 UI 层面同样缺乏时区意识——前端不分青红皂白发送本地时间。

### 修复方案

**推荐方案：前端发送 UTC 时间戳（改动最小）**

`presetRange()` 改用 `toISOString()` 输出 UTC 时间：

```ts
function presetRange(p: TimePreset): { start: string; end: string } {
  const now = Date.now();
  switch (p) {
    case "15m":
      return { start: new Date(now - 15 * 60 * 1000).toISOString(),
               end: new Date(now).toISOString() };
    case "4h":
      return { start: new Date(now - 4 * 3600 * 1000).toISOString(),
               end: new Date(now).toISOString() };
    case "24h":
      return { start: new Date(now - 24 * 3600 * 1000).toISOString(),
               end: new Date(now).toISOString() };
    case "7d":
      return { start: new Date(now - 7 * 24 * 3600 * 1000).toISOString(),
               end: new Date(now).toISOString() };
    // ...
  }
}
```

这样前端发送 `2026-07-15T02:34:38.000Z`，后端 `start_time` 列存的也是 UTC 格式，字符串比较天然对齐。

**后端增强（可选，防御不同客户端格式）**：

```rust
// spend.rs 中接收日期参数后，尝试解析多种格式
fn normalize_date_for_query(date_str: &str, is_end: bool) -> String {
    // 已经是 RFC3339，直接返回
    if date_str.contains('Z') || date_str.contains('+') {
        return date_str.to_string();
    }
    // 纯日期 "yyyy-MM-dd"，补时间部分
    if date_str.len() == 10 {
        return if is_end {
            format!("{}T23:59:59.999Z", date_str)
        } else {
            format!("{}T00:00:00Z", date_str)
        };
    }
    // 有本地时间无时区后缀，补 Z
    format!("{}Z", date_str)
}
```

### TDD 测试用例

```rust
#[test]
fn test_spend_logs_date_filter_preserves_records_in_range() { ... }
#[test]  
fn test_spend_logs_date_filter_excludes_records_before_range() { ... }
```

### BDD 测试用例

```gherkin
Scenario: 15-minute filter shows only recent requests
  Given 有 5 条 spend log，时间分布在过去 1 小时内
  When 选择"15 min"预设
  Then 只显示过去 15 分钟内的记录

Scenario: 4-hour filter handles timezone correctly  
  Given 有 spend log 记录在过去 8 小时内
  When 选择"4 hours"预设  
  Then 只显示过去 4 小时内的记录
```

---

## 问题 2: Usage 页面不包括当天的数据

### 现象

Usage 页面选择 30 天预设时，发送参数：
```
/global/spend/activity?start_date=2026-06-15&end_date=2026-07-15
```
但 7 月 15 日当天的数据没有被统计出来。

### 根因

**两层问题：end_date 被截断 + 后端按 UTC 分组统计**

**第一层：end_date 纯日期被截断为零点**

`query_activity_metadata()`（`db.rs:2163`）和 `query_activity_daily()`（`db.rs:2206`）中：

```sql
WHERE start_time >= ? AND start_time <= ?
--    start_time >= '2026-06-15'
--    start_time <= '2026-07-15'
```

数据库 `start_time` 列是 `DateTime<Utc>`，存储值形如 `2026-07-15T10:30:00Z`（带时间和时区后缀）。

SQL 字符串比较时：
- `'2026-07-15'` 相当于 `'2026-07-15'`（前缀）
- `'2026-07-15T10:30:00Z'` > `'2026-07-15'`（更多字符，`T` > 空）
- 所以 `start_time <= '2026-07-15'` **无法匹配任何带时间部分的记录**，因为所有存储值都以 `2026-07-15T` 开头

**第二层：后端使用 UTC 日期分组**

`query_activity_daily()` 使用 SQL `DATE(start_time)` 做日聚合（`db.rs:2201`）。由于 `start_time` 存储的是 `DateTime<Utc>`，`DATE()` 函数返回的是 **UTC 日期**，而非用户的本地日期。

这意味着对于 UTC+8 用户（如北京）：
- 上午 6:00 的请求 → UTC 前一日 22:00 → 被归入**前一天**
- 只有 UTC 同日（即北京时间 08:00 之后）的请求才会归入当天

**总结：** 当日数据消失的真正原因是：
1. `end_date` 纯日期在 SQL 字符串比较中不匹配任何带时间的记录（**更严重**）
2. UTC 日期分组导致上午数据归入前一天（**次要**）

### 修复方案

**推荐方案：后端修复 end_date 截断（必须） + 前端改为 UTC 日期（建议）**

**后端（必须）** — `query_activity_metadata()` 和 `query_activity_daily()` 用 `date()` 做比较，解决字符串截断：

```sql
-- 旧：
WHERE start_time >= ? AND start_time <= ?
-- 新：
WHERE date(start_time) >= date(?) AND date(start_time) <= date(?)
```

这样 `date('2026-07-15T10:30:00Z') >= date('2026-06-15')` = `'2026-07-15' >= '2026-06-15'` 正确匹配。

或者用代码规范化 end_date 再 query：

```rust
/// 纯日期补 T23:59:59.999Z，有时间的补 Z
fn normalize_activity_end_date(end_date: &str) -> String {
    if end_date.len() == 10 {
        format!("{}T23:59:59.999Z", end_date)
    } else if !end_date.contains('Z') && !end_date.contains('+') {
        format!("{}Z", end_date)
    } else {
        end_date.to_string()
    }
}
```

**前端（建议）** — `usage/index.tsx` 的 `presetRange()` 用 UTC 日期：

```ts
function presetRange(p: DatePreset): { start: string; end: string } {
  const now = Date.now();
  const end = new Date(now).toISOString().split('T')[0]; // "2026-07-15" in UTC
  switch (p) {
    case "3d":
      return { start: new Date(now - 3 * 86400000).toISOString().split('T')[0], end };
    case "7d":
      return { start: new Date(now - 7 * 86400000).toISOString().split('T')[0], end };
    case "30d": default:
      return { start: new Date(now - 30 * 86400000).toISOString().split('T')[0], end };
  }
}
```

这样 `DATE(start_time)` 直接等于前端日期，UTC 分组偏移问题不治而愈。

> **两个 bug 的依赖关系：** 若只改前端 UTC 日期不改后端 `date()`，当天数据仍会丢失（`'2026-07-15'` < `'2026-07-15T08:00:00Z'`）。必须两个一起修。

### TDD 测试用例

```rust
#[tokio::test]
async fn test_activity_includes_today_when_end_date_is_today() { ... }
#[tokio::test]
async fn test_activity_daily_includes_last_day_rows() { ... }
```

### BDD 测试用例

```gherkin
Scenario: 30-day usage includes today's data
  Given 今天有 spend log 记录
  When 打开 Usage 页面，默认选择 30 天预设
  Then 总 spend 指标包含今天的数据
  And 每日图表最后一根柱子是今天的日期
```

---

## 问题 3: Request ID 格式不一致 & Claude Code 请求元数据未利用

### 现象

同一个 Claude Code 会话中（检查主机名称）：
- 第一次请求：`req_1db699c8-99cc-4cb1-a85b-5455ce105cd2` ✅（带 `req_` 前缀）
- 第二次请求：`3cfccdb0-0475-4ad2-b5df-920a38afc717` ❌（没有 `req_` 前缀）
- 异常请求：`aa51780a-1678-489e-b060-c5bd32035776` ❌（没有 `req_` 前缀）

### Litellm 源码分析：End User 的来源

#### 你看到的 JSON blob 是什么

UI 表格 "End User" 列显示：
```
{"device_id":"afc79055735e...","account_uuid":"","session_id":"786d39c1-..."}
```

**这是 Claude Code 通过 Anthropic 协议的 `metadata.user_id` 字段发送的。** Claude Code 把自己的 device_id、session_id 序列化为 JSON 字符串，塞进请求体的 `metadata.user_id` 里。litellm 透传了这个字符串。

#### 完整链路（6 级优先级）

Litellm 的 `get_end_user_id_from_request_body()`（`auth/auth_utils.py:1143`）按以下顺序查找 end_user：

```
Check 1: 标准 customer ID headers（x-customer-id, x-end-user-id 等）
Check 2: 管理员配置的 user_header_name / user_header_mappings
Check 3: request_body["user"]                        ← OpenAI 格式
Check 4: request_body["litellm_metadata"]["user"]
Check 5: request_body["metadata"]["user_id"]         ← ★ Claude Code 走这里
Check 6: request_body["safety_identifier"]
```

Claude Code 的请求体结构是：
```json
{
  "model": "claude-sonnet-5",
  "max_tokens": 4096,
  "messages": [...],
  "metadata": {
    "user_id": "{\"device_id\":\"afc79...\",\"account_uuid\":\"\",\"session_id\":\"786d39c1-...\"}"
  }
}
```

Litellm 的 Anthropic adapter（`adapters/transformation.py:891-904`）先把 `metadata.user_id` 映射到内部 `user` 字段，但**不影响** `get_end_user_id_from_request_body`——它会**直接用原始请求体查 `metadata.user_id`**（Check 5）。

#### `_coerce_user_id_to_str()` 的透传逻辑

`auth_utils.py:1109` 有个关键的过滤函数：

```python
def _coerce_user_id_to_str(value):
    if isinstance(value, str):
        # 默认（validate_end_user_id_in_db=False）:
        #   JSON 字符串原样通过！"{\"device_id\":...}" 不会被拆解
        # 如果开启 validate_end_user_id_in_db=True:
        #   以 { 或 [ 开头的字符串会被拒绝
        return stripped
```

默认没开验证，所以 JSON blob 直接存入 `end_user` 列。

#### 结论

**Litellm 的 "End User" = Anthropic 请求体 `metadata.user_id`（Check 5 命中时）。Claude Code 把 device_id + session_id 打包成 JSON 字符串放在这个字段里。** 不需要任何管理员配置；它是 Anthropic 协议的标准字段，litellm 自动提取。

### 根因 A: Request ID 格式不一致

aigw 中 `request_id` 的实际生成位置（**协议响应字段** vs **SpendLog 数据库字段**）：

| 位置 | 字段类型 | 格式 | 是否正确 |
|------|---------|------|:------:|
| `v1_messages.rs:35` 等 | Anthropic 错误响应 `request_id` | `req_{uuid}` | ✅ Anthropic 协议约定 |
| `v1_messages.rs:455-460`（流式） | **SpendLog.request_id** | `req_{uuid}` | ❌ **SpendLog 不应带前缀** |
| `v1_messages.rs:287,399,691`（非流式） | **SpendLog.request_id** | 纯 UUID | ✅ |
| `chat.rs:900,1209` | **SpendLog.request_id** | 纯 UUID | ✅ |

**总结：**

- `req_` 前缀是 Anthropic **协议层**的约定——仅用于 Anthropic 错误响应格式（`{"request_id": "req_xxx"}`）。
- Litellm 的 SpendLog `request_id` **不带任何前缀**——它优先复用上游 LLM 响应 ID（如 Anthropic 返回的 `msg_xxx`），其次用 `x-litellm-call-id` 头，兜底生成纯 UUID v4。
- aigw 流式路径（`v1_messages.rs:460`）错误地把 `req_` 前缀带进了 SpendLog——需改为纯 UUID。
- 非流式路径和 `chat.rs` 的纯 UUID 是正确的。
- **aigw 还缺少复用上游 LLM 响应 ID 的能力**——导致 SpendLog 的 request_id 和上游响应 ID 无法关联。

### 根因 B: End User 元数据未被利用

**Litellm 的实际做法：从 Anthropic 协议 `metadata.user_id` 自动提取。**

Claude Code 发到 aigw 的 `/v1/messages` 请求体中包含：
```json
{
  "metadata": {
    "user_id": "{\"device_id\":\"afc79...\",\"account_uuid\":\"\",\"session_id\":\"786d39c1-...\"}"
  }
}
```

Litellm 在 `get_end_user_id_from_request_body()` 的 Check 5 中直接从请求体 `metadata.user_id` 提取，不需要任何额外配置。

**aigw 代码现状**（所有 SpendLog 创建点：`v1_messages.rs:310,424,483,716` 和 `chat.rs:861,929,1181,1243`）：

```rust
end_user: None,           // ← 始终为 None，从未从请求体 metadata.user_id 提取
session_id: None,          // ← 始终为 None
requester_ip_address: None, // ← 始终为 None
agent_id: None,            // ← 始终为 None
```

### 修复方案

**1. Request ID（不改前缀，增加复用上游响应 ID 的能力）：**

```rust
// 非流式：从上游 HTTP 响应中提取 id 字段
let upstream_id = resp_body
    .get("id")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());
let request_id = upstream_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
```

**2. End User（从请求体 `metadata.user_id` 提取，对标 litellm Check 5）：**

```rust
// 从 Anthropic 协议 metadata.user_id 提取
// Claude Code 会把 device_id/session_id 打包成 JSON 字符串放在这里
let end_user = body_val
    .get("metadata")
    .and_then(|m| m.get("user_id"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

// 可选：如果值是 JSON 字符串，解析出 device_id/session_id 分别存储
let (end_user, session_id) = if let Some(ref eu) = end_user {
    if let Ok(parsed) = serde_json::from_str::<Value>(eu) {
        let sid = parsed.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        (Some(eu.clone()), sid)
    } else {
        (Some(eu.clone()), None)
    }
} else {
    (None, None)
};
```

**3. 从请求头提取真实 IP：**

```rust
let requester_ip_address = headers
    .get("x-forwarded-for")
    .and_then(|v| v.to_str().ok())
    .map(|s| s.split(',').next().unwrap_or("").trim().to_string());
```

### TDD 测试用例

```rust
#[test]
fn test_spend_log_request_id_reuses_upstream_response_id() { ... }

#[test]
fn test_spend_log_end_user_extracted_from_metadata_user_id() { ... }

#[test]
fn test_spend_log_session_id_extracted_from_metadata_json() { ... }

#[test]
fn test_spend_log_captures_requester_ip() { ... }
```

---

## 问题 4: 复制按钮无交互反馈

### 根因

`copyToClipboard()`（`spend-logs/index.tsx:148-150`）调用 `navigator.clipboard.writeText()` 后不做任何 UI 反馈：

```tsx
function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
```

同样的问题存在于 `keys/index.tsx` 和 `playground/index.tsx`。

### 修复方案

创建一个通用的 `useCopyToClipboard` hook，使用 `useState` 追踪复制状态并显示短暂 toast/图标变化：

```tsx
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  
  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* ignore */ }
  };
  
  return (
    <Button variant="ghost" size="icon" onClick={handleCopy}>
      {copied 
        ? <Check className="h-3 w-3 text-green-500" />
        : <Copy className="h-3 w-3" />
      }
    </Button>
  );
}
```

### BDD 测试用例

```gherkin
Scenario: Copy button shows success feedback
  Given spend logs 页面已加载
  When 点击某条记录的 Request ID 复制按钮
  Then 按钮图标从 Copy 变为 Check（绿色）
  And 2 秒后恢复为 Copy 图标
```

---

## 总结

| 问题 | 严重度 | 修复复杂度 | 需要改动的文件 |
|------|:------:|:----------:|---------------|
| 1. Spend Logs 时间过滤（前端本地时间 vs 后端 UTC） | **高** | 中 | 前端 `spend-logs/index.tsx`（推荐）或后端 `db.rs` |
| 2. Usage 不含当天数据（end_date 截断 + UTC 分组偏移） | **高** | 中 | `db.rs`（`query_activity_*` 两处）+ 前端 `usage/index.tsx` |
| 3. Request ID 格式纠正 + end_user 来源分析 | 中 | 低 | `v1_messages.rs`、`chat.rs`（提取 metadata.user_id 作 end_user） |
| 4. 复制按钮无交互反馈 | 低 | 低 | 新建 `useCopyToClipboard` hook + 3 个页面 |

### 关键发现

1. **Litellm UI "End User" 的来源是 Anthropic 协议的 `metadata.user_id` 字段**（无需管理员配置）。Claude Code 把 `device_id` + `session_id` 序列化成 JSON 字符串塞进这个字段，litellm 的 `get_end_user_id_from_request_body()` Check 5 自动提取并存入 DB。aigw 只需在 `messages_handler` 中从 `body_val["metadata"]["user_id"]` 读取即可。

2. **`req_` 前缀只属于 Anthropic 协议层，不应出现在 SpendLog 中。** Litellm 的 SpendLog `request_id` 优先复用上游 LLM 响应的 `id`（如 Anthropic 的 `msg_xxx`），其次用 `x-litellm-call-id` 头，兜底才生成纯 UUID v4——全都不带前缀。aigw 的 SpendLog 用纯 UUID 是正确的。

3. **Litellm UI 的 15 分钟过滤器也有同样的时区 bug**，其请求格式 `start_date=2026-07-15+03:22:00` 同样是本地时间无 UTC 标记。最佳修复方向：aigw 可以比 litellm 做得更好——前端统一发送 UTC。

4. **Usage 页面当天数据丢失的主因是 end_date 纯日期在 SQL 字符串比较中被截断**（`'2026-07-15'` < `'2026-07-15T10:30:00Z'`），其次是 UTC 分组偏移。推荐两端同修。
