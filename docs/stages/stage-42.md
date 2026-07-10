# Stage 42: `/v1/messages` SpendLog 修复 + 错误码修正

**Phase**: 14 — `/v1/messages` 接口修复
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 0.5h
**依赖**: Stage 40

---

## 目标

1. 修复 SpendLog 中 `api_key` 硬编码为 `"claude-message"` 的问题
2. 修复 `user_id` 未传入 SpendLog 的问题
3. 修复未知 model 返回 404 而非 `invalid_request_error` 的问题

## 验收标准

- [ ] SpendLog.api_key 使用真实的 `token_hash`（从 auth 阶段保存的值），不再是 `"claude-message"`
- [ ] SpendLog.user 使用真实的 `user_id`
- [ ] DailySpendLog.entity_id 使用 `user_id`
- [ ] 不存在的 model 返回 HTTP 400 + `invalid_request_error`（对齐 Anthropic 规范）
- [ ] 单元测试：验证错误类型为 `invalid_request_error`

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改：3 处 SpendLog 构造 + 1 处错误码 |

## 技术方案

### 修复 1+2：SpendLog 字段

依赖 Stage 44 在 auth 阶段保存的 `auth_token_hash` 和 `auth_user_id`。

**非流式 SpendLog**（当前 ~line 414-458）：

```diff
- api_key: "claude-message".to_string(),
+ api_key: auth_token_hash.clone(),
...
- user: None,
+ user: auth_user_id.clone(),
```

**流式 SpendLog**（当前 ~line 319-356）：同上。

**DailySpendLog entity_id**：

```diff
- entity_id: spend_log.user.clone().unwrap_or_default(),
+ entity_id: auth_user_id.clone().unwrap_or_default(),
```

### 修复 3：model not found 错误码

```diff
  let pm = models.iter().find(|m| m.model_name == model).ok_or_else(|| {
      anthropic_error(
-         StatusCode::NOT_FOUND,
-         "not_found_error",
+         StatusCode::BAD_REQUEST,
+         "invalid_request_error",
          &format!("Model '{}' not found", model),
      )
  })?;
```

### 单元测试

```rust
#[tokio::test]
async fn test_model_not_found_error_format() {
    // ... setup app with empty proxy_models ...
    let body = json!({
        "model": "nonexistent-model",
        "messages": [{"role": "user", "content": "hi"}],
        "max_tokens": 100
    });
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    
    let val: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(val["error"]["type"].as_str(), Some("invalid_request_error"));
}
```
