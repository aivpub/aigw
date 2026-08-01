# Stage 40: `/v1/messages` 复用 `resolve_upstream_params` + Key 校验对齐

**Phase**: 14 — `/v1/messages` 接口修复
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 1.5h
**依赖**: 无

---

## 目标

将 `/v1/messages` handler 的手动 upstream 查找和 key 校验逻辑替换为 chat.rs 中已验证的 `resolve_upstream_params()` + key 预算校验。

## 验收标准

- [ ] `resolve_upstream_params` 暴露为 `pub(crate)`，可被 v1_messages.rs 调用
- [ ] 替换 handler 中手动 `list_models()` + env var 查找为 `chat::resolve_upstream_params()`
- [ ] 上游请求使用 `resolved.api_base` 和 `resolved.api_key`，而非环境变量
- [ ] Key 预算校验：`spend >= max_budget` 时返回 Anthropic 格式 429 错误
- [ ] 现有单元测试通过（验证 handler 仍正确工作）
- [ ] Token hash + user_id 在 auth 阶段保存（为 Stage 46 准备）

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs:64` | 修改：`async fn resolve_upstream_params` → `pub(crate) async fn resolve_upstream_params` |
| `crates/aigw-server/src/routes/v1_messages.rs:195-226` | 替换：model lookup + env var 逻辑 → 一行 `chat::resolve_upstream_params()` |
| `crates/aigw-server/src/routes/v1_messages.rs:89-124` | 新增：auth 成功后保存 `token_hash`/`user_id` + budget check |

## 技术方案

### 改动 1：chat.rs 可见性

```rust
// Before:
async fn resolve_upstream_params(...)

// After:
pub(crate) async fn resolve_upstream_params(...)
```

### 改动 2：替换 model lookup

删掉 lines 195-226（~30 行），替换为：

```rust
// 4. Resolve upstream routing + pricing (reuses chat.rs verified logic:
//    proxy_models → decrypt → credential refs → env var fallback)
let resolved = chat::resolve_upstream_params(&state, &model).await?;
let input_cost = resolved.input_cost_per_token;
let output_cost = resolved.output_cost_per_token;
```

### 改动 3：Key 预算校验

在 auth 成功后（line 122 附近），对非 master key 增加：

```rust
if !is_master {
    if let Some(ref key) = key {
        if let Some(max_budget) = key.max_budget_f64() {
            if key.spend >= max_budget {
                return Err(anthropic_error(
                    StatusCode::TOO_MANY_REQUESTS,
                    "budget_exceeded",
                    "Budget exceeded for this API key",
                ));
            }
        }
    }
}
```

### 改动 4：保存 auth 信息

```rust
// 在 auth 成功后保存，供后续 SpendLog 使用（Stage 46 替换硬编码）
let auth_token_hash = if is_master {
    "master_key".to_string()
} else {
    hash_token(token)
};
let auth_user_id = key.as_ref().and_then(|k| k.user_id.clone());
```

## 风险

- `resolve_upstream_params` 变为 `pub(crate)` 不影响 chat.rs 的封装（仅 crate 内可见）
- 该函数在 chat.rs 中依赖 `state.db.get_model_by_name()`（按 `model_name` 精确查找），需确认 v1_messages 的 model 参数能正确匹配
