# Stage 7-12 调研中间结论

> 2026-07-04 整理，用于更新 stage 文档，避免上下文丢失

## Stage 8: credential 管理

### litellm Credentials 表（schema.prisma:37-41）

```prisma
model LiteLLM_CredentialsTable {
  credential_id    String  @id @default(uuid())
  credential_name  String  @unique
  credential_values Json
  credential_info  Json?
}
```

- **独立 credentials 表**：litellm 有专门的凭据存储，不只是嵌在 litellm_params 里
- **credential_name 唯一**：可通过 name 引用
- **credential_values**：JSON 存储（含 api_key 等）
- **credential_info**：JSON 元信息（可选）
- **proxy_models 关联**：schema.prisma:970 `litellm_credential_name String?` — 模型表有字段引用 credential name

### litellm_params 中的凭据引用方式

litellm 支持两种方式：
1. **直接嵌 api_key**：`litellm_params: {"model":"gpt-4","api_key":"sk-xxx"}`
2. **引用 credential**：`litellm_params: {"model":"gpt-4","litellm_credential_name":"my-aws-cred"}`
   - 网关启动时从 `LiteLLM_CredentialsTable` 查 credential_values 注入

### aigw 设计决策

- 字段名：aigw 用 `model_params`（不用 litellm_params），但保留兼容映射
- 新增 `credentials` 表（对齐 litellm）
- `proxy_models` 表增加 `credential_name` 字段（可选，引用 credentials 表）
- aigw-migrate 双向映射时处理 credential 引用

## Stage 9: litellm-rust 实现参考

### litellm-rust 真实存在

路径：`~/works/projects/github.com/BerriAI/litellm/litellm-rust`

3 crate workspace：
- `crates/core` — 纯转换层（types, provider transforms, router），无网络
- `crates/ai-gateway` — 路由 + 网络 I/O（axum server）
- `crates/python-bridge` — PyO3 cdylib

### provider 目录结构

```
crates/core/src/providers/<provider>/<route>/
  transformation.rs   # 请求/响应转换
  mod.rs              # 模块声明
```

现有 provider：azure_ai, mistral, openai, vertex_ai（每个有 ocr/realtime 等 route）

### 关键 trait（需进一步读源码确认）

litellm-rust 的 provider trait 设计在 `crates/core/src/providers/` 下，每个 provider 实现 transformation 模块。需要参考其：
- 请求转换接口
- 响应转换接口
- 流式 chunk 转换接口
- 错误处理

### Python adapter 模式（litellm 主仓库）

- `BaseConfig` 抽象基类（trait 等价）：transform_request / transform_response / map_openai_params / validate_environment / get_complete_url / get_error_class
- 双向 adapter：`litellm/llms/anthropic/experimental_pass_through/adapters/`
  - `AnthropicAdapter` — OpenAI↔Anthropic 转换
  - `LiteLLMAnthropicMessagesAdapter` — 实际转换逻辑
- 流式状态机：`AnthropicStreamWrapper`
  - 状态：sent_first_chunk, sent_content_block_start/finish, current_content_block_type
  - 事件序列：message_start → content_block_start → content_block_delta → content_block_stop → message_delta → message_stop
  - `_CombinedChunkSplitter` 处理 fake-stream
- 流式和非流式独立转换路径

## Stage 10: Claude API 规范

### 流式判断

- **`stream` 布尔字段** 在请求体中，默认 `false`
- **不是 Accept header**
- SDK 用 `.stream()` vs `.create()`，底层都设 stream 字段

### 请求必填

- Header: `Content-Type: application/json`, `anthropic-version: 2023-06-01`, `x-api-key`
- Body: `model`, `messages`, `max_tokens`

### messages.content

- 可以是**字符串**（简写，等价 `[{type:"text",text:"..."}]`）
- 也可以是 **content block 数组**（text/image/document/tool_use/tool_result/thinking 等）

### 非流式响应

- Content-Type: `application/json`
- 单个 Message 对象：`{id, type:"message", role:"assistant", content:[...], model, stop_reason, stop_sequence, usage}`
- stop_reason: `end_turn` / `max_tokens` / `stop_sequence` / `tool_use` / `pause_turn` / `refusal`
- usage: `{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens, ...}`

### 流式响应

- Content-Type: `text/event-stream`
- SSE 事件：`event: <type>\ndata: <json>\n\n`
- 事件类型：message_start, content_block_start, content_block_delta, content_block_stop, message_delta, message_stop, ping, error
- message_start: `message.content` 为空数组，stop_reason 为 null
- content_block_delta 类型：text_delta, input_json_delta（tool_use 部分 JSON）, thinking_delta, signature_delta
- message_delta: usage 是**累计值**（非增量），含 stop_reason
- tool_use 流式：input_json_delta 是部分 JSON 字符串，需累积后解析

### 错误格式

- 非流式：`{"type":"error","error":{"type":"...","message":"..."},"request_id":"..."}`
- 流式：作为 `event: error` SSE 事件（HTTP 200 后）
- HTTP 状态：400/401/402/403/404/413/429/500/504/529

### 兼容性测试策略

- BDD 分为 mock 场景（默认执行）和 real 场景（需真实 API key，标签隔离）
- 用 `@real_api` tag 标记 real 场景，默认跳过，配置环境变量后执行
- 用 Claude SDK / OpenAI SDK 做兼容性验证（客户端视角）

## Stage 11: 端点前缀

### 调研结论（基于现有代码和 litellm 惯例）

litellm proxy 同时支持两种前缀：
1. **`/key/*`** — 历史前缀，litellm 早期版本使用
2. **`/v1/key/*`** — 现代前缀，符合 OpenAI 风格的版本化路由

两者是**别名关系**，共享 handler。aigw 应同时支持以兼容不同客户端。

### aigw 设计决策

- 保留现有 `/key/*`（向后兼容）
- 新增 `/v1/key/*` 别名（litellm 现代兼容）
- 共享 handler，仅路由前缀不同
- BDD 验证两者行为一致

## Stage 12: mock vs real 场景

### 用户反馈

1. 全覆盖应分为 mock 和 real 两组场景
2. 检测特定配置/环境变量时同时运行 real 场景

### 设计决策

- BDD 分为两组：
  - `@mock` — 默认执行，使用 mock 上游
  - `@real_api` — 默认跳过，检测到 `AIGW_REAL_API=1` 或对应 API key 环境变量时执行
- CI 默认只跑 `@mock`
- 本地/手动验证时可通过 `task bdd-real` 跑 `@real_api`
- real 场景需要真实 OpenAI/Claude API key，验证端到端兼容性
- real 场景使用独立 feature 文件：`features/real/*.feature`

### Tag 设计

```gherkin
@mock
Feature: 端到端调用链路（mock）
  ...

@real_api
Feature: 真实 API 兼容性验证
  ...
```

### 执行命令

- `task bdd` — 只跑 @mock（默认）
- `task bdd-real` — 只跑 @real_api（需要 API key）
- `task bdd-all` — 全部执行
