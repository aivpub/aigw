# Stage 14: NaCl 加密/解密库 + aigw-migrate PostgreSQL 源支持

**创建日期**: 2026-07-06
**状态**: 规划中
**优先级**: P0
**前置条件**: Stage 13 完成
**预估**: 6-8h

---

## 1. 目标

在 Rust 中实现 litellm 兼容的加解密能力，使 `aigw-migrate` 能解密源数据库中加密存储的 `litellm_params` 和 `credential_values`。同时扩展迁移工具支持 PostgreSQL 源数据库。

---

## 2. 加密算法规格（来自 litellm `encrypt_decrypt_utils.py`）

### 2.1 模式 1：Legacy NaCl SecretBox（生产环境默认）

```
算法: NaCl SecretBox (XSalsa20-Poly1305)
密钥派生: signing_key = SHA256(master_key) → 32 bytes
加密: SecretBox(key).encrypt(plaintext_bytes)
      输出格式: nonce(24) || ciphertext || MAC
编码: base64url(encrypted_bytes) → DB 存储字符串
解密: base64url_decode → SecretBox(key).decrypt()
```

### 2.2 模式 2：AES-256-GCM（`v2:gcm:` 前缀）

```
前缀: ciphertext 以 "v2:gcm:" 开头 → GCM 模式
密钥派生: PBKDF2(master_key, salt="litellm", iterations=600_000, dklen=32)
IV: 去掉前缀后 ciphertext 前 16 bytes
Tag: ciphertext 后 16 bytes
解密: AES-256-GCM(key, iv, ciphertext[16:-16]).decrypt(tag)
```

### 2.3 Salt Key 解析

```
salt_key = os.environ.get("LITELLM_SALT_KEY") or master_key
```

用于 spend_logs 中 api_key 的掩码加解密。aigw-migrate 从源 DB 读取 master_key 后同时用作 salt_key。

### 2.4 master_key 提取路径

```
SELECT param_value FROM LiteLLM_Config WHERE param_name = 'general_settings'
→ JSON 解析 → .master_key 字段
```

---

## 3. 交付

### 3.1 `aigw-core/src/crypto.rs` 加解密模块

```rust
/// 解密 litellm 加密字段（自动检测模式）
pub fn decrypt_litellm_value(encrypted_b64: &str, master_key: &str) -> Result<String>;

/// 用 litellm 算法加密（用于回滚导出或 aigw 侧加密）
pub fn encrypt_litellm_value(plaintext: &str, master_key: &str) -> Result<String>;

/// 提取 salt key（用于 api_key 掩码）
pub fn derive_salt_key(master_key: &str) -> String;
```

实现路径：
1. base64url decode（兼容 base64 standard）
2. 检测 `v2:gcm:` 前缀 → AES-256-GCM 路径
3. 否则 → NaCl SecretBox 路径
4. `SHA256(master_key)` → 32-byte key
5. `SecretBox::decrypt(ciphertext, key)` → plaintext

### 3.2 依赖 crate

```toml
crypto_secretbox = "0.1"    # NaCl SecretBox (XSalsa20-Poly1305)
sha2 = "0.10"               # SHA256
pbkdf2 = "0.12"             # PBKDF2 for AES-256-GCM key
aes-gcm = "0.10"            # AES-256-GCM
base64 = "0.22"             # base64url encode/decode
```

### 3.3 `aigw-migrate` PostgreSQL 源支持

**CLI 参数变更**（向后兼容）：

```
# 旧格式（文件路径，保持兼容）
aigw-migrate import --source ./litellm.db --target ./aigw.db

# 新格式（DB URL，支持 PG）
aigw-migrate import \
  --source-url postgres://user:pass@litellm-prod:5432/litellm \
  --target-url postgres://aigw:pass@localhost/aigw
```

**`import.rs` 重构**：
- 当前硬编码 `SqliteConnectOptions`、`SqlitePoolOptions`、`sqlx::sqlite::SqliteRow`
- 改为抽象 DB 连接：`fn connect(db_url: &str) -> Result<AnyPool>`
- 使用 `sqlx::Any` 或 trait 抽象消除 SQLite 耦合
- 自动检测 DB 类型（`sqlite:` / `postgres:` / `mysql:` 前缀）

### 3.4 master_key 自动提取

```rust
/// 从源 litellm 数据库提取 master_key
pub async fn extract_master_key(db_url: &str) -> Result<String> {
    // 1. 连接源 DB
    // 2. SELECT param_value FROM "LiteLLM_Config"
    //    WHERE param_name = 'general_settings'
    // 3. JSON 解析 → .master_key
    // 4. 返回 master_key
}
```

不需要用户手动提供，迁移命令自动完成。

---

## 4. 门禁

| 测试 | 验证方式 |
|------|----------|
| 解密已知密文 | 用生产 DB 数据验证：密文 → 明文一致 |
| 往返测试 | `encrypt(x)` → `decrypt(encrypt(x))` == x |
| AES-256-GCM 检测 | `v2:gcm:` 前缀密文正确解密 |
| PG 源连接 | `aigw-migrate import --source-url postgres://...` 成功 |
| master_key 提取 | 从测试 DB 自动提取，无需 CLI 参数 |
