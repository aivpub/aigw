# Stage 16: aigw 运行时解密 + 凭证引用解析

**创建日期**: 2026-07-06
**状态**: ✅ 完成
**优先级**: P0
**前置条件**: Stage 15 完成
**预估**: 4-6h

---

## 1. 目标

aigw 启动时读取 `AIGW_MASTER_KEY`，运行时解密 `proxy_models.litellm_params` 和 `credentials.credential_values` 中的密文字段。同时实现凭证引用解析：当模型通过 `litellm_credential_name` 引用凭证时，自动查 `credentials` 表注入上游 `api_key`。

---

## 2. 背景

Stage 15 迁移完成后，aigw DB 中的数据是加密存储的：
- `proxy_models.litellm_params` — NaCl 密文（含 `api_key`、`api_base` 等）
- `credentials.credential_values` — NaCl 密文（含 `api_key`、`api_base` 等）

aigw 需要在请求到达时解密这些字段，才能正常代理上游请求。

---

## 3. 交付

### 3.1 aigw 启动时加密上下文初始化

```rust
// 启动时
let master_key = std::env::var("AIGW_MASTER_KEY")
    .expect("AIGW_MASTER_KEY must be set");

let crypto_ctx = CryptoContext::new(&master_key);
// 注入到 app state
```

`CryptoContext` 持有 SHA256 派生后的 32-byte key，避免每次解密重复派生。

### 3.2 `proxy_models.litellm_params` 运行时解密

路由层请求到达时：

```
1. 从 DB 读取 proxy_models.litellm_params（密文 JSON 字符串）
2. decrypt_litellm_value(litellm_params, master_key) → 明文 JSON
3. 解析 JSON 获得 {model, api_base, api_key, ...}
4. 用于构造 upstream 请求
```

### 3.3 凭证引用解析器

litellm 使用两种凭证模式，aigw 需同时兼容：

**引用模式**（9/15 个模型使用）：
```
proxy_models.litellm_params = {
  "model": "openai/gpt-4o",
  "litellm_credential_name": "my-openai-key",
  // 没有 api_key
}

→ 查 credentials 表 WHERE credential_name = "my-openai-key"
→ 解密 credential_values → {api_key: "sk-xxx", api_base: "https://...", ...}
→ 合并到 upstream 请求参数
```

**内联模式**（3/15 个模型使用）：
```
proxy_models.litellm_params = {
  "model": "openai/gpt-4o",
  "api_key": "sk-xxx",     // 内联（已解密）
  "api_base": "https://..."
}
→ 直接使用
```

**优先级**：`litellm_credential_name` 存在时走引用模式，否则走内联模式。

### 3.4 实现位置

在路由层选择 upstream 时执行：

```rust
// aigw-server/src/router.rs（或等效位置）
async fn resolve_upstream_params(
    model: &ProxyModel,
    credentials_store: &CredentialsStore,
    crypto: &CryptoContext,
) -> Result<UpstreamParams> {
    // 1. 解密 litellm_params
    let params: LitellmParams = crypto.decrypt_json(&model.litellm_params)?;

    // 2. 检查是否有 credential_name 引用
    if let Some(cred_name) = &params.litellm_credential_name {
        let cred = credentials_store.get_credential_by_name(cred_name).await?;
        let cred_values: CredentialValues = crypto.decrypt_json(&cred.credential_values)?;
        // 合并凭证值（api_key, api_base, api_version 等）
        params.merge_credential(cred_values);
    }

    Ok(params.into_upstream())
}
```

---

## 4. 门禁

| 测试 | 验证 |
|------|------|
| 引用凭证模型代理成功 | `litellm_credential_name` 引用的模型能正常请求上游 |
| 内联 api_key 模型代理成功 | 不带 `credential_name` 的模型直接使用 `litellm_params.api_key` |
| 凭证不存在时明确报错 | 引用不存在的 credential_name → 返回明确错误信息 |
| 运行时解密正确 | 日志中不出现明文的 api_key（安全审计） |
