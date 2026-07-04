# Stage 12: BDD 全量覆盖 + 集成测试体系（RGR 收尾）

**Status**: Complete
**Phase**: Phase 5 — 最小化后端补齐（RGR 驱动）
**预估工时**: 4-6h
**依赖**: Stage 7-11（全部完成）

## Goal

完成 BDD 全量用例覆盖，建立端到端集成测试体系（含 mock 上游服务器），CI 集成，输出 BDD 实践指南。这是 Phase 5 的收尾 stage，验证所有端点行为符合 .feature 描述。

## Mock vs Real 场景分组

### 设计原则

全覆盖测试分为两组：

| 组 | Tag | 默认执行 | 触发条件 | 上游 |
|----|-----|---------|---------|------|
| Mock | `@mock` | 是 | 始终 | 内存 mock 服务器 |
| Real | `@real_api` | 否 | `AIGW_REAL_API=1` 或对应 API key 环境变量 | 真实 OpenAI/Claude API |

### 触发机制

- **CI 默认只跑 `@mock`** — 不依赖外部 API key，确定性、无成本
- **本地手动验证** — 通过 `task bdd-real` 跑 `@real_api` 场景
- **自动检测** — 当检测到 `AIGW_REAL_API=1` 且 `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` 已配置时，自动启用 real 场景
- **`task bdd-all`** — 全部执行（mock + real）

### 文件组织

```
tests/bdd/features/
  *.feature                    # @mock 场景（默认执行）
  real/
    *.feature                  # @real_api 场景（默认跳过）
```

### Tag 用法

```gherkin
@mock
Feature: 端到端调用链路（mock）
  Scenario: OpenAI 客户端调用 OpenAI 上游
    Given mock OpenAI 上游已启动
    ...

@real_api
Feature: 真实 API 兼容性验证
  Scenario: Claude SDK 调用 /v1/messages
    Given AIGW_REAL_API=1 且 ANTHROPIC_API_KEY 已配置
    ...
```

### Taskfile 命令

```yaml
bdd:
  desc: 运行 BDD 测试（仅 @mock）
  cmds:
    - cargo test --test bdd -- --tags @mock

bdd-real:
  desc: 运行 BDD 测试（仅 @real_api，需 API key）
  cmds:
    - AIGW_REAL_API=1 cargo test --test bdd -- --tags @real_api

bdd-all:
  desc: 运行全部 BDD 测试（mock + real）
  cmds:
    - AIGW_REAL_API=1 cargo test --test bdd
```

## Mock 上游服务器

```rust
pub struct MockUpstream {
    pub openai_server: axum::Server,
    pub claude_server: axum::Server,
    pub recorded_requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

impl MockUpstream {
    pub async fn start() -> Self;
    pub fn openai_url(&self) -> String;
    pub fn claude_url(&self) -> String;
    pub fn set_response(&self, path: &str, status: u16, body: Value);
    pub fn recorded_requests(&self) -> Vec<RecordedRequest>;
}
```

支持：
- `/v1/chat/completions`（OpenAI 协议）
- `/v1/messages`（Claude 协议）
- 流式 SSE 响应
- 可配置响应状态码和 body
- 记录所有收到的请求用于断言

## 关键交付件

1. `tests/bdd/features/end_to_end.feature` — 端到端场景（@mock）
2. `tests/bdd/features/error_handling.feature` — 错误处理场景（@mock）
3. `tests/bdd/features/auth.feature` — 鉴权场景全集（@mock）
4. `tests/bdd/features/real/end_to_end_real.feature` — 真实 API 端到端（@real_api）
5. `tests/bdd/features/real/compatibility_real.feature` — SDK 兼容性（@real_api）
6. `tests/bdd/support/mock_upstream.rs` — Mock OpenAI/Claude 上游服务器
7. `tests/bdd/support/world.rs` — 增强 World（支持 mock 上游 + real API 切换）
8. `.github/workflows/bdd.yml` — CI 集成（仅 @mock）
9. `docs/15-bdd-guide.md` — BDD 实践指南
10. `Taskfile.yml` — `task bdd` / `task bdd-real` / `task bdd-all` 命令

## BDD 场景

### end_to_end.feature（@mock）

```gherkin
@mock
Feature: 端到端调用链路（mock）

  Scenario: OpenAI 客户端调用 OpenAI 上游
    Given mock OpenAI 上游已启动
    And 已配置 model "gpt-4" 指向 mock 上游
    When 客户端发送 POST /v1/chat/completions
      """
      {"model":"gpt-4","messages":[{"role":"user","content":"hi"}]}
      """
    Then 响应状态码为 200
    And mock 上游收到 1 次请求
    And 响应来自 mock 上游

  Scenario: OpenAI 客户端调用 Claude 上游
    Given mock Claude 上游已启动
    And 已配置 model "claude-via-openai" 指向 Claude mock
    When 客户端发送 POST /v1/chat/completions
    Then mock Claude 上游收到 Claude 协议请求
    And 客户端收到 OpenAI 协议响应

  Scenario: Claude 客户端调用 OpenAI 上游
    Given mock OpenAI 上游已启动
    And 已配置 model "gpt-4-via-claude" 指向 OpenAI mock
    When 客户端发送 POST /v1/messages
    Then mock OpenAI 上游收到 OpenAI 协议请求
    And 客户端收到 Claude 协议响应

  Scenario: 端到端流式调用
    Given mock 上游返回流式响应
    When 客户端发送 stream=true 请求
    Then 客户端收到完整 SSE 流
    And 流包含多个 chunk

  Scenario: 用量记录写入
    Given 客户端发起一次成功调用
    When 查询 /spend/logs
    Then 包含本次调用的记录
    And 记录包含 model 和 tokens

  Scenario: 预算限制触发
    Given key "budget-key" 的预算已耗尽
    When 使用该 key 发起调用
    Then 响应状态码为 429
    And 错误信息为 budget exceeded
```

### error_handling.feature（@mock）

```gherkin
@mock
Feature: 错误处理

  Scenario: 上游 500 错误传递
    Given 上游返回 500
    When 客户端调用
    Then 响应状态码为 502
    And 错误体包含上游错误信息

  Scenario: 上游超时
    Given 上游响应延迟 60s
    When 客户端调用（timeout=5s）
    Then 响应状态码为 504

  Scenario: 上游 429 限流
    Given 上游返回 429
    When 客户端调用
    Then 响应状态码为 429
    And 错误信息为 rate limited

  Scenario: 无效请求体
    When 发送无效 JSON 的 POST 请求
    Then 响应状态码为 400

  Scenario: model 字段缺失
    When 发送不含 model 的请求
    Then 响应状态码为 400
```

### auth.feature（@mock）

```gherkin
@mock
Feature: 鉴权全集

  Scenario: 无 Bearer token 被拒绝
    When 发送未带 Authorization 的请求
    Then 响应状态码为 401

  Scenario: 无效 token 被拒绝
    When 发送 Authorization: Bearer invalid-token
    Then 响应状态码为 401

  Scenario: 已删除 key 失效
    Given key "deleted" 已被删除
    When 使用该 key 发起请求
    Then 响应状态码为 401

  Scenario: master key 全权限
    Given 使用 master key
    When 访问 /key/list
    Then 响应状态码为 200

  Scenario: 普通 key 不能访问管理端点
    Given 使用普通 key
    When 访问 /key/list
    Then 响应状态码为 403
```

### end_to_end_real.feature（@real_api）

```gherkin
@real_api
Feature: 真实 API 端到端

  Scenario: OpenAI SDK 调用 aigw /v1/chat/completions
    Given AIGW_REAL_API=1 且 OPENAI_API_KEY 已配置
    When 使用 OpenAI SDK 调用 aigw
    Then SDK 成功解析响应
    And 响应包含 choices[0].message.content

  Scenario: Claude SDK 调用 aigw /v1/messages
    Given AIGW_REAL_API=1 且 ANTHROPIC_API_KEY 已配置
    When 使用 Claude SDK 调用 aigw
    Then SDK 成功解析响应
    And 响应包含 content[0].text

  Scenario: OpenAI SDK 流式调用
    Given AIGW_REAL_API=1 且 OPENAI_API_KEY 已配置
    When 使用 OpenAI SDK stream=true 调用 aigw
    Then SDK 成功解析完整流
    And 流包含 delta.content

  Scenario: Claude SDK 流式调用
    Given AIGW_REAL_API=1 且 ANTHROPIC_API_KEY 已配置
    When 使用 Claude SDK stream=true 调用 aigw
    Then SDK 成功解析完整事件流
    And 事件序列为 message_start → content_block_* → message_stop

  Scenario: 跨协议调用
    Given AIGW_REAL_API=1 且 OPENAI_API_KEY 已配置
    When 使用 Claude SDK 调用 aigw model="gpt-4"
    Then SDK 成功解析（虽然上游是 OpenAI）

  Scenario: 用量记录写入
    Given AIGW_REAL_API=1
    When 客户端发起一次真实调用
    Then /spend/logs 包含本次调用记录
    And 记录的 tokens > 0
```

### compatibility_real.feature（@real_api）

```gherkin
@real_api
Feature: SDK 兼容性验证

  Scenario: OpenAI SDK 错误格式兼容
    Given AIGW_REAL_API=1
    When 发送无效请求经 aigw 到 OpenAI
    Then 错误格式与 OpenAI 官方一致

  Scenario: Claude SDK 错误格式兼容
    Given AIGW_REAL_API=1
    When 发送缺少 max_tokens 的请求经 aigw
    Then 错误格式与 Anthropic 官方一致
    And 错误 type 为 "invalid_request_error"

  Scenario: Claude SDK 流式错误兼容
    Given AIGW_REAL_API=1
    When 流式调用中途上游返回错误
    Then 客户端收到 event: error SSE 事件
```

## CI 集成（.github/workflows/bdd.yml）

```yaml
name: BDD Tests
on: [push, pull_request]
jobs:
  bdd-mock:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cucumber-rust
      - run: task bdd              # 仅 @mock
      - run: task bdd-coverage

  bdd-real:
    runs-on: ubuntu-latest
    if: false  # 默认禁用，手动触发
    env:
      AIGW_REAL_API: 1
      ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
      OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cucumber-rust
      - run: task bdd-real
```

## BDD 实践指南（docs/15-bdd-guide.md）

内容大纲：
1. RGR 循环工作流
2. .feature 文件组织规范（@mock vs @real_api）
3. Step binding 编写指南
4. World 上下文使用
5. Mock 上游集成
6. Real API 测试策略（环境变量、API key 管理、成本控制）
7. CI 集成配置
8. 常见模式与反模式

## RGR 循环

1. **Red**: 写端到端 .feature（@mock + @real_api）→ 失败（mock 上游未集成）
2. **Green**: 实现 mock 上游 + 增强 World → @mock 场景通过；@real_api 场景需手动验证
3. **Refactor**: 提取公共 step 到 `common.rs`，整理 .feature 分类

## 验收标准

- [x] `end_to_end.feature`（@mock）≥ 6 个 Scenario 全部通过
- [x] `error_handling.feature`（@mock）≥ 5 个 Scenario 全部通过
- [x] `auth.feature`（@mock）≥ 5 个 Scenario 全部通过
- [x] `end_to_end_real.feature`（@real_api）≥ 6 个 Scenario（需 API key 时跳过，不阻断 CI）
- [x] `compatibility_real.feature`（@real_api）≥ 3 个 Scenario
- [x] Mock 上游服务器支持 OpenAI/Claude 协议 + 流式
- [x] CI workflow 自动运行 `task bdd`（仅 @mock）
- [x] `task bdd-real` 命令可用（需 API key）
- [x] `task bdd-all` 命令可用
- [x] `docs/15-bdd-guide.md` 完成
- [x] 全部 Stage 7-12 的 .feature 在 CI 中通过
- [ ] BDD 覆盖率报告生成（端点覆盖率 ≥ 90%）— 后续优化
- [x] `@real_api` 场景默认跳过，`AIGW_REAL_API=1` 时执行

## 风险

| 风险 | 缓解 |
|------|------|
| Mock 上游端口冲突 | 使用 ephemeral port，CI 中隔离 |
| 流式 mock 复杂 | 复用 Stage 10 的 SSE 工具，mock 端复用解析器 |
| CI 执行时间长 | 并行化 .feature 文件执行，缓存 cargo build |
| 端到端场景不稳定 | Mock 上游响应确定性配置，避免随机 |
| Real API 测试成本 | 默认跳过，CI 不依赖；本地手动 `task bdd-real`；设置每月预算上限 |
| Real API 非确定性 | Real 场景只校验结构不校验具体内容；重试机制处理瞬态失败 |
| API key 泄露 | 通过 GitHub Secrets 注入，不写入代码；`.env` 加入 `.gitignore` |
