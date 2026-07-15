# Stage 54: end_user 提取 + 复制按钮反馈

**Phase**: 18 — Spend Logs & Usage 质量修复（P0）
**状态**: ⏳ 待开始
**预估**: 5h
**依赖**: Phase 17（代理转发架构重构）完成

---

## 目标

1. **end_user 提取** — 从 Anthropic 协议请求体 `metadata.user_id` 提取终端用户身份，对标 litellm `get_end_user_id_from_request_body()` Check 5
2. **SpendLog 元数据填充** — `session_id`（从 JSON blob 解析）、`requester_ip_address`（从 `X-Forwarded-For` 头）
3. **Request ID 格式修正** — 流式路径 `v1_messages.rs:455-460` 误将 `req_` 前缀写入 SpendLog，改为纯 UUID
4. **复制按钮反馈** — 新建 `useCopyToClipboard` hook，替换 3 个页面的静默复制

## 根因摘要

- **end_user 始终为 None**: `messages_handler` 解析了请求体的 `model`/`messages`/`max_tokens`/`stream`，但从未读 `metadata.user_id`
- **Claude Code 行为**: 将 `{"device_id":"...","account_uuid":"","session_id":"..."}` JSON 字符串写入 `metadata.user_id`，litellm 自动提取并以原始字符串存入 DB
- **流式 request_id 异常**: `v1_messages.rs:455` 的 `streaming_request_id` 用 `format!("req_{}", uuid)` 写入 SpendLog，而 litellm 的 SpendLog 全用纯 UUID
- **复制无反馈**: `navigator.clipboard.writeText()` 调用后不更新 UI

详见 `docs/14-spend-logs-usage-bugs.md` 问题 3 和问题 4。

## 验收标准

- [ ] `/v1/messages` 请求带 `metadata.user_id` 时，SpendLog.end_user 正确记录
- [ ] `metadata.user_id` 为 JSON 字符串时，`session_id` 解析并单独存储
- [ ] 无 `metadata.user_id` 时 end_user 为 None（不崩溃）
- [ ] `requester_ip_address` 从 `X-Forwarded-For` 正确提取（含代理链场景）
- [ ] 所有 SpendLog.request_id 不带 `req_` 前缀（流式路径已修正）
- [ ] 复制按钮点击后图标从 Copy 变为 Check（绿色），2 秒后恢复
- [ ] **门禁**: 全量 UT + BDD + 前端 Playwright 全部通过

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/v1_messages.rs` | **修改** — 提取 metadata.user_id；修正流式 request_id；提取 X-Forwarded-For |
| `crates/aigw-server/src/routes/chat.rs` | **修改** — 提取 X-Forwarded-For（对齐 v1_messages） |
| `crates/aigw-frontend/src/hooks/useCopyToClipboard.ts` | **新建** — 通用复制 hook |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | **修改** — 使用 useCopyToClipboard |
| `crates/aigw-frontend/src/pages/keys/index.tsx` | **修改** — 使用 useCopyToClipboard |
| `crates/aigw-frontend/src/pages/playground/index.tsx` | **修改** — 使用 useCopyToClipboard |

## 技术方案

### 1. end_user 提取

在 `messages_handler()` 中 `body_val` 解析后在获取 `model`/`messages`/`max_tokens`/`stream` 之后，新增：

```rust
// 从 Anthropic 协议 metadata.user_id 提取 end_user
// Claude Code 会把 device_id/session_id 打包成 JSON 字符串放在这里
let end_user = body_val
    .get("metadata")
    .and_then(|m| m.get("user_id"))
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

// 可选：如果值是 JSON 字符串，解析出 device_id/session_id 分别存储
let session_id = end_user.as_ref().and_then(|eu| {
    serde_json::from_str::<Value>(eu)
        .ok()
        .and_then(|v| v.get("session_id")
            .and_then(|id| id.as_str())
            .map(|s| s.to_string()))
});
```

然后将 `end_user` 和 `session_id` 传入所有 `SpendLog` 构造点（流式/非流式/错误路径共 4 处），替换现有的 `end_user: None` / `session_id: None`。

### 2. requester_ip_address 提取

```rust
let requester_ip_address = headers
    .get("x-forwarded-for")
    .and_then(|v| v.to_str().ok())
    .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
    .filter(|s| !s.is_empty());
```

同样传入所有 SpendLog 构造点。

### 3. 流式 Request ID 修正

`v1_messages.rs:455`：

```rust
// 旧（错误地将 req_ 前缀写入 SpendLog）:
let streaming_request_id = format!("req_{}", uuid::Uuid::new_v4());

// 新（对齐 litellm，纯 UUID）:
let streaming_request_id = uuid::Uuid::new_v4().to_string();
```

### 4. useCopyToClipboard hook

`crates/aigw-frontend/src/hooks/useCopyToClipboard.ts`：

```ts
import { useState, useCallback } from "react";

export function useCopyToClipboard(resetMs = 2000) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), resetMs);
    } catch {
      // clipboard API unavailable — silently ignore
    }
  }, [resetMs]);

  return { copied, copy };
}
```

三个页面替换：

```tsx
// 旧:
function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).catch(() => {});
}
<Button onClick={() => copyToClipboard(log.request_id)}>
  <Copy className="h-3 w-3" />
</Button>

// 新:
const { copied, copy } = useCopyToClipboard();
<Button onClick={() => copy(log.request_id)}>
  {copied ? <Check className="h-3 w-3 text-green-500" /> : <Copy className="h-3 w-3" />}
</Button>
```

## TDD 测试用例

### UT (Rust)

```rust
#[test]
fn test_end_user_extracted_from_metadata_user_id() {
    // body_val = {"metadata": {"user_id": "alice"}}
    // assert: end_user = Some("alice")
}

#[test]
fn test_end_user_json_parses_session_id() {
    // body_val = {"metadata": {"user_id": "{\"session_id\":\"s1\",\"device_id\":\"d1\"}"}}
    // assert: end_user = Some("{\"session_id\":...}")
    // assert: session_id = Some("s1")
}

#[test]
fn test_end_user_empty_when_no_metadata() {
    // body_val = {"model": "claude-3", "messages": [...]}  // no metadata
    // assert: end_user = None
}

#[test]
fn test_requester_ip_from_x_forwarded_for() {
    // headers = {"x-forwarded-for": "10.0.0.1, 10.0.0.2"}
    // assert: requester_ip_address = Some("10.0.0.1")
}

#[test]
fn test_streaming_spend_log_no_req_prefix() {
    // verify streaming_request_id does NOT contain "req_"
    // assert: !streaming_request_id.starts_with("req_")
}
```

### BDD (Gherkin)

```gherkin
Scenario: Request with metadata.user_id records end_user
  Given 发送 /v1/messages 请求，body 包含 metadata.user_id = "test-user-123"
  When 请求成功完成
  Then SpendLog 中 end_user 为 "test-user-123"

Scenario: Copy button shows success feedback
  Given spend logs 页面已加载
  When 点击某条记录的 Request ID 复制按钮
  Then 按钮图标从 Copy 变为 Check（绿色）
  And 2 秒后恢复为 Copy 图标
```

## 风险与回滚

| 风险 | 应对 |
|------|------|
| `metadata.user_id` JSON 解析失败 | `serde_json::from_str()` 返回 `Err` → fallback 为 `None`，不影响 end_user 原始值 |
| `X-Forwarded-For` 被 CDN 注入伪造 | 仅取最左 IP（客户端源 IP），配合 trusted-proxy 配置可追加校验 |
| 旧代码不再 import `format`/`subMinutes` | 检查 `spend-logs/index.tsx` 是否还有其他位置使用 date-fns（如 `todayStr()`） |

回滚方式：`git revert` 该 commit。
