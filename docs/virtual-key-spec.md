# Virtual Key 生成与 Hash 规范

> 对齐 litellm v1.90.0 `key_management_endpoints.py` / `_types.py` / `constants.py`

## 1. Virtual Key 生成

### 伪代码

```
LENGTH = 16  # bytes, 可由环境变量 LENGTH_OF_GENERATED_KEY 覆盖

random_bytes = CSPRNG(LENGTH)                    # 16 字节密码学安全随机数
b64_part    = base64url_encode(random_bytes)     # 22 字符, 无 padding
virtual_key = "sk-" + b64_part                   # 25 字符完整 key
```

### 字符集

- 前缀: `sk-` (固定)
- 主体: base64url 字母表 `[A-Za-z0-9_-]` (无 `=` padding)

### 长度关系

| 随机字节 | base64url 字符 | 完整 key |
|---------|---------------|---------|
| 16      | 22            | 25 (含 `sk-`) |

### 示例

```
random_bytes (hex) : 8a2c28637541668cf76c20a388e337xx  (16 字节)
b64_part           : iiwoY3VBZoz3bCCjiOM3lA            (22 字符)
virtual_key        : sk-iiwoY3VBZoz3bCCjiOM3lA          (25 字符)
```

## 2. Token Hash 计算

数据库存储 hash 而非明文,便于比对且避免明文泄露。

### 伪代码

```
token_hash = SHA256_hex(virtual_key)   # 对完整字符串(含 sk- 前缀)做 SHA256
```

### 关键细节

- **输入是完整 virtual_key 字符串**,包含 `sk-` 前缀
- **输入末尾无换行符** (`\n` 会让结果完全不同)
- 输出为 64 字符小写 hex

### 示例

```
virtual_key : sk-iiwoY3VBZoz3bCCjiOM3lA
token_hash  : 3d3d2718c23662aad7b9ecd8bd0c194335317c01531886d0e65e5be56693e4a2
```

### 命令行验证

```bash
# 正确 (无换行)
echo -n 'sk-iiwoY3VBZoz3bCCjiOM3lA' | shasum -a 256
# 或
printf '%s' 'sk-iiwoY3VBZoz3bCCjiOM3lA' | shasum -a 256

# 错误 (echo 默认带 \n, 结果不同)
echo 'sk-iiwoY3VBZoz3bCCjiOM3lA' | shasum -a 256
```

## 3. 字段映射

| 概念       | litellm 列名 | 说明                          |
|-----------|-------------|------------------------------|
| 明文 key   | (不存储)     | 仅返回给用户一次             |
| token hash | `token`     | SHA256(virtual_key), 用于比对 |
| key alias  | `key_alias` | 用户可读别名, 不参与认证     |

## 4. 认证流程

```
client 请求带 Authorization: Bearer sk-xxxx
  ↓
server: 提取 bearer token
  ↓
server: token_hash = SHA256(bearer_token)
  ↓
server: 查 LiteLLM_VerificationToken WHERE token = token_hash
  ↓
命中 → 认证通过 / 未命中 → 401
```

## 5. aigw 实现要求

- [ ] `generate_virtual_key()` → 生成 `sk-` + base64url(16 bytes)
- [ ] `hash_token(key)` → SHA256 hex
- [ ] 认证中间件用 hash 查 DB,明文 key 不落库
- [ ] 单元测试覆盖:生成 key 长度/字符集、hash 可重现、echo -n 对齐
