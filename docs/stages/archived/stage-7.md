# Stage 7: BDD 框架搭建 + 既有功能 .feature

**Status**: Planning
**Phase**: Phase 5 -- 最小化后端补齐（RGR 驱动）
**预估工时**: 4-6h
**依赖**: Stage 0-6（全部完成）

## Goal

搭建 Gherkin BDD 基础设施，为既有 `/key/*` `/spend/*` `/global/*` `/health/*` 写 .feature 固化行为，验证 RGR（Red-Green-Refactor）循环可用。后续 Stage 8-12 都将基于此框架用 BDD 驱动开发。

## 技术选型

- **BDD 框架**: `cucumber-rust`（Rust 生态最成熟的 Gherkin 实现）
- **HTTP 测试客户端**: `axum::test` + `tower::ServiceExt`
- **测试 DB**: 内存 SQLite（`sqlite::memory:`）

## 目录结构

```
tests/bdd/
  Cargo.toml              # bdd test crate（[[test]] 多 target）
  features/
    keys.feature          # 既有 /key/* 行为
    spend.feature         # 既有 /spend/* 行为
    global.feature        # 既有 /global/* 行为
    health.feature        # 既有 /health/* 行为
  steps/
    common.rs             # 公共 step（Given/Then 等）
    keys_steps.rs         # key 相关 step bindings
    spend_steps.rs        # spend 相关 step bindings
    global_steps.rs       # global 相关 step bindings
  support/
    world.rs              # cucumber World（测试共享状态）
```

## World 是什么

在 cucumber-rust 中，**World** 是一个由测试作者定义的结构体，实现 `cucumber::World` trait。它是**测试会话期间所有 step 共享的状态容器**：

- 每个 Scenario 开始时新建一个 World 实例
- 同一 Scenario 内的 Given/When/Then step 共享同一个 World
- 不同 Scenario 之间 World 互不影响（隔离）

World 持有跨 step 传递所需的状态，例如：

```rust
pub struct TestWorld {
    pub db: Database,                  // 内存 SQLite 连接
    pub app: axum::Router,             // 待测 axum 应用
    pub master_key: String,            // 管理员 token
    pub last_response: Option<Response>, // 上一次 HTTP 响应
    pub last_response_body: Option<serde_json::Value>,
    pub created_keys: HashMap<String, String>, // alias → 明文 key
}

#[async_trait::async_trait]
impl cucumber::World for TestWorld {
    async fn new() -> Self { ... }
}
```

这样 `Given 已存在 key "test-key"` 可以把生成的 key 存入 `created_keys`，后续 `When 发送请求 using key "test-key"` 就能从 World 取出使用，无需在每个 step 里重复创建。

## Virtual Key 实现规范

**严格遵循** `docs/virtual-key-spec.md`（对齐 litellm v1.90.0）：

### Key 生成
```
LENGTH = 16  # bytes
random_bytes = CSPRNG(16)
b64_part    = base64url_encode(random_bytes)   # 22 字符, 无 padding
virtual_key = "sk-" + b64_part                 # 25 字符
```

### Token Hash
```
token_hash = SHA256_hex(virtual_key)   # 输入含 "sk-" 前缀，末尾无换行
```

### 数据库字段映射（关键：litellm ↔ aigw 字段名差异）

| 概念 | litellm 列名 | aigw 列名 | 说明 |
|------|-------------|----------|------|
| token hash | `token` | `token` | SHA256(virtual_key)，用于认证比对 |
| key alias | `key_alias` | `key_alias` | 用户可读别名 |
| 明文 key | （不存储） | （不存储） | 仅返回用户一次 |

> ⚠️ aigw 表名是 `virtual_keys`（非 litellm 的 `LiteLLM_VerificationToken`），但 token 列名保持 `token` 以兼容迁移工具。

### 认证流程
```
client 请求带 Authorization: Bearer sk-xxxx
  ↓
server: 提取 bearer token
  ↓
server: token_hash = SHA256(bearer_token)
  ↓
server: 查 virtual_keys WHERE token = token_hash
  ↓
命中 → 认证通过 / 未命中 → 401
```

### 实现要求（已在 Stage 0-6 完成，本 Stage 通过 .feature 固化）

- `generate_virtual_key()` → 生成 `sk-` + base64url(16 bytes)
- `hash_token(key)` → SHA256 hex
- 认证中间件用 hash 查 DB，明文 key 不落库
- BDD 场景验证：生成 key 长度/字符集、hash 可重现、`echo -n` 对齐

## 关键交付件

1. `tests/bdd/Cargo.toml` — BDD 测试 crate
2. `tests/bdd/features/keys.feature` — 覆盖 key generate/info/list/update/delete/regenerate
3. `tests/bdd/features/spend.feature` — 覆盖 /spend/logs, /spend/keys
4. `tests/bdd/features/global.feature` — 覆盖 /global/spend, /global/spend/keys（如存在）
5. `tests/bdd/features/health.feature` — 覆盖 /health, /health/liveness, /health/readiness
6. `tests/bdd/steps/*.rs` — Step bindings
7. `tests/bdd/support/world.rs` — TestWorld（内存 DB + axum test client + 共享状态）
8. `Taskfile.yml` 新增 `task bdd` 命令

## BDD 场景示例

### keys.feature

```gherkin
Feature: Virtual Key 管理
  作为管理员
  我需要管理 virtual key
  以便控制 API 访问

  Scenario: 生成新 key
    Given 管理员已认证
    When 发送 POST /key/generate 请求
      """
      {"key_alias": "my-test-key", "models": ["gpt-4"]}
      """
    Then 响应状态码为 200
    And 响应包含 key 字段
    And key 以 "sk-" 开头
    And key 长度为 25 字符
    And key 主体字符集为 base64url

  Scenario: 未认证请求被拒绝
    When 发送 POST /key/generate 请求
      """
      {"key_alias": "test"}
      """
    Then 响应状态码为 401

  Scenario: 查询 key 信息
    Given 已存在 key "test-key"
    When 发送 GET /key/info?key=test-key
    Then 响应包含 key_alias 字段

  Scenario: 列出所有 key
    Given 已存在 3 个 key
    When 发送 GET /key/list
    Then 响应包含 3 个 key

  Scenario: 删除 key
    Given 已存在 key "delete-me"
    When 发送 DELETE /key/delete?key=delete-me
    Then 该 key 不再存在

  Scenario: 重新生成 key
    Given 已存在 key "old-key"
    When 发送 POST /key/regenerate {"key": "old-key"}
    Then 返回新 key
    And 旧 key 不再有效

  Scenario: 认证使用 hash 而非明文
    Given 已存在 key "auth-test"
    When 直接查询 virtual_keys 表
    Then token 列存储的是 SHA256 hash
    And token 列不等于明文 key
```

### spend.feature

```gherkin
Feature: Spend 查询
  Scenario: 查询 spend logs
    Given 已存在 5 条 spend_logs 记录
    When 发送 GET /spend/logs
    Then 响应包含 5 条记录

  Scenario: 按 key 过滤查询
    Given 已存在 key "test-key" 的 3 条记录
    When 发送 GET /spend/logs?key=test-key
    Then 响应仅包含 "test-key" 的记录

  Scenario: 查询 key 维度用量
    Given 已存在多个 key 的用量记录
    When 发送 GET /spend/keys
    Then 响应包含按 key 聚合的用量
```

### global.feature

```gherkin
Feature: Global 用量查询
  Scenario: 查询全局用量
    Given 已存在用量记录
    When 发送 GET /global/spend
    Then 响应包含全局聚合用量

  Scenario: 全局用量不受 key 隔离
    Given 已存在 3 个 key 各自的用量记录
    When 发送 GET /global/spend
    Then 响应包含所有 key 的合计用量
```

### health.feature

```gherkin
Feature: 健康检查
  Scenario: 健康总览
    When 发送 GET /health
    Then 响应状态码为 200
    And 响应包含 status 字段

  Scenario: liveness 探针
    When 发送 GET /health/liveness
    Then 响应状态码为 200

  Scenario: readiness 探针
    When 发送 GET /health/readiness
    Then 响应状态码为 200
```

## RGR 循环

- **Red**: 写 .feature（既有行为），运行 → 应全部通过（既有功能已实现）
- **Green**: 验证既有功能符合 .feature 描述，修复偏差
- **Refactor**: 提取公共 step 到 common.rs

## 验收标准

- [ ] `task bdd` 命令可运行
- [ ] keys.feature ≥ 7 个 Scenario 全部通过（含 hash 验证场景）
- [ ] spend.feature ≥ 3 个 Scenario 全部通过
- [ ] global.feature ≥ 2 个 Scenario 全部通过
- [ ] health.feature ≥ 3 个 Scenario 全部通过
- [ ] TestWorld 使用内存 SQLite，无外部依赖
- [ ] 公共 step 提取到 common.rs，可复用
- [ ] key 生成符合 `docs/virtual-key-spec.md`（25 字符、sk- 前缀、base64url）
- [ ] hash 存储符合规范（SHA256、含前缀、无换行）

## 风险

| 风险 | 缓解 |
|------|------|
| cucumber-rust 学习曲线 | 先用最简 .feature 验证框架，复杂场景逐步添加 |
| 既有功能行为与预期不符 | Red 阶段发现的偏差记录到 .feature，Green 阶段修复 |
| /global/* 端点行为不确定 | 先 grep 现有路由确认存在的端点，再写 .feature |
