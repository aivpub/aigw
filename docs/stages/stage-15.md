# Stage 15: aigw-migrate 全量迁移（解密 → 重加密）+ 端到端验证

**创建日期**: 2026-07-06
**状态**: ✅ 完成
**优先级**: P0
**前置条件**: Stage 14 完成
**预估**: 6-8h

---

## 1. 目标

实现 `aigw-migrate remote-import` 命令，将 litellm 数据库全部数据导入 aigw。迁移过程中：解密 litellm 加密字段 → 用 aigw 密钥重新加密 → 写入 aigw DB。确保 aigw DB 中不出现明文 `api_key`。

---

## 2. 数据流

```
源 litellm DB (PG/SQLite)
  │
  ├─ LiteLLM_Config → 提取 master_key
  │
  ├─ LiteLLM_CredentialsTable
  │   credential_values (litellm 密文)
  │     → NaCl 解密 (litellm master_key)
  │       → 明文 JSON
  │         → NaCl 加密 (aigw master_key)
  │           → credentials.credential_values (aigw 密文)
  │
  ├─ LiteLLM_ProxyModelTable
  │   litellm_params (litellm 密文)
  │     → NaCl 解密 (litellm master_key)
  │       → 明文 JSON
  │         → NaCl 加密 (aigw master_key)
  │           → proxy_models.litellm_params (aigw 密文)
  │
  ├─ LiteLLM_VerificationToken → virtual_keys (直接复制)
  ├─ LiteLLM_SpendLogs → spend_logs (直接复制，分批)
  ├─ Multi-tenant tables → 直接复制
  │
  └─ 解密失败模型 → 跳过 + 输出警告
```

**关键**：aigw 侧使用与 litellm 相同的 NaCl SecretBox 算法，但用自己的 `AIGW_MASTER_KEY`。迁移完成后的 aigw DB 中 `api_key` 以密文存储，运行时按需解密。

---

## 3. 交付

### 3.1 `aigw-migrate remote-import` CLI

```
aigw-migrate remote-import \
  --source-url postgres://user:pass@litellm-prod:5432/litellm \
  --target-url postgres://aigw:pass@localhost/aigw \
  --target-master-key sk-aigw-master-key
```

（`--target-master-key` 默认从 `AIGW_MASTER_KEY` 环境变量读取）

### 3.2 迁移流程

```
Step 1: 连接源 DB，自动提取 litellm master_key
Step 2: 迁移多租户表（直接 SQL 复制，无需加解密）
Step 3: 迁移 credentials（解密 → 重加密 credential_values JSON）
Step 4: 迁移 proxy_models（解密 → 重加密 litellm_params JSON）
Step 5: 迁移 virtual_keys（直接复制 55+ 列）
Step 6: 分批迁移 spend_logs（每批 10,000 行，进度条）
Step 7: 验证行数一致性（对比源/目标每张表 row count）
```

### 3.3 解密感知列映射

`LiteLLM_CredentialsTable.credential_values`：
```
litellm 密文 → decrypt_litellm_value() → 明文 JSON → encrypt_litellm_value(aigw_key) → aigw 密文
```

`LiteLLM_ProxyModelTable.litellm_params`：
```
litellm 密文 → decrypt_litellm_value() → 明文 JSON → encrypt_litellm_value(aigw_key) → aigw 密文
```

### 3.4 解密失败处理

- NaCl 解密失败的模型自动跳过
- 输出警告：`[WARN] Skipped model {model_id}: decryption failed (may be old key)`
- 不影响其余数据迁移
- 最终报告跳过数量

### 3.5 大批量 spend_logs 优化

- 分批处理（每批 10,000 条，避免内存溢出）
- 使用 indicatif 进度条
- 预估生产：66.8 万条 spend_logs

### 3.6 BDD 迁移测试

```gherkin
Scenario: 从 litellm DB 迁移到 aigw 后模型代理正常
  Given litellm 数据库有加密存储的模型 "gpt-4o" 和凭证
  And litellm 已有 virtual key
  When 运行 aigw-migrate remote-import --target-master-key "aigw-key"
  And 启动 aigw 并设置 AIGW_MASTER_KEY="aigw-key"
  Then aigw /v1/models 返回与前 litellm 相同的模型列表
  And 使用迁移后的 key 调用 /v1/chat/completions 成功
  And aigw DB 中 api_key 以密文存储（直接读 DB 看不到明文）

Scenario: 迁移后 spend_logs 可查
  Given 迁移已完成
  When 查询 /spend/logs
  Then 日志数量与 litellm 源一致

Scenario: 解密失败的旧模型被跳过
  Given litellm 有无法解密的旧模型（历史密钥）
  When 运行 aigw-migrate remote-import
  Then 迁移完成并输出警告 "skipped N models"
  And 其余模型正常迁移
```

---

## 4. 门禁

- `remote-import` 从 PG 源完整迁移到 aigw PG/SQLite
- 迁移后 aigw DB 中 `credential_values` 和 `litellm_params` 为密文
- 设置 `AIGW_MASTER_KEY` 后 aigw 能正常代理请求
- 解密失败模型自动跳过，不影响其余
- BDD 迁移测试全部通过
