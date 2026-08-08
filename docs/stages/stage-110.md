# Stage 110: OpenAI Embeddings API Passthrough — 后端 handler + 四端点路由

**所属**: Phase 44（Embeddings API 代理支持）
**预估**: 10h（后端 + 测试）
**依赖**: 无（与在途 P1 收尾完全解耦，可独立交付）

---

## 1. 目标

落地 `POST /v1/embeddings` Passthrough 端点，复用 responses.rs 骨架 + 认证/计费链路，让 OpenAI SDK `embeddings.create()` 可直打 aigw。

**四端点**：
- `/v1/embeddings` — OpenAI 标准（主）
- `/embeddings` — 无版本别名
- `/engines/{model}/embeddings` — Azure legacy
- `/openai/deployments/{model}/embeddings` — Azure

## 2. 核心设计

### 2.1 Handler 流程（非流式）

```
ChatAuth 认证
  → 校验 model（string）+ input（string|array，非空）→ 400
  → 非 master：resolve_key_model_list + max_budget_f64 预算检查
  → ModelResolver::resolve(model) + Router::pick_deployment
  → 硬选 OpenAIPassthrough（⚠️ 不得走 select_adapter 的 OpenAI+AnthropicNative arm → OpenAIToAnthropic 会破坏 embedding body）
  → adapt_request（OpenAIPassthrough 只改 model）
  → upstream URL = {api_base}/embeddings（四端点都汇聚到上游同一路径）
  → Authorization: Bearer 透传
  → 非流式 passthrough 响应
  → SpendLog call_type="embedding" + calc_spend(prompt_only) + 实体 spend 增量 + daily_spend_queue
```

### 2.2 关键决策

| 决策 | 理由 |
|------|------|
| **硬选 `OpenAIPassthrough`** | `select_adapter(OpenAI, AnthropicNative)` → `OpenAIToAnthropic`（adapter.rs L77）会把 embedding body 当 chat 转换，产生垃圾。embedding 模型天然 OpenAI 兼容，AnthropicNative 部署直接拒绝。 |
| **不加 `ClientProtocol::Embeddings` 变体** | Responses 加变体是因为 URL 路径/校验/usage 全不同；embeddings 的 usage 解析已能复用 `extract_prompt_tokens`/`extract_total_tokens`（responses.rs L49/L69），上游路径固定 `{api_base}/embeddings`，OpenAIPassthrough 透传足够。加变体纯属对称性装饰，非必需。 |
| **四端点共用同一 handler** | 差异仅路径匹配；`/engines/{model}/embeddings` 和 `/openai/deployments/{model}/embeddings` 的 model 取自 path param，需从 path 提取后合并进 body。 |
| **`call_type="embedding"`** | 对齐 litellm SDK 直调 call_type（proxy 侧是 `"aembedding"`；aigw 全同步无 async 区分，用 `"embedding"` 语义最贴）。 |
| **calc_spend prompt-only** | embedding usage 无 completion_tokens，`calc_spend` 传 completion=0 → 零输出成本，`prompt_tokens × input_cost_per_token` 正确。 |
| **非流式** | embeddings 无 `stream` 字段，不需要 responses.rs 的流式两阶段 SpendLog 代码（L502-930）。 |

### 2.3 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-server/src/routes/embeddings.rs` | **新建** | 主 handler（responses.rs 非流式子集，~450 行） |
| `crates/aigw-server/src/routes/mod.rs` | 修改 | `pub mod embeddings;` |
| `crates/aigw-server/src/main.rs` | 修改 | 注册 4 端点 + `use routes::{... embeddings}` + 文件 doc comment |
| `crates/aigw-server/src/routes/responses.rs` | 修改 | `extract_prompt_tokens`/`extract_total_tokens` 提升 `pub(crate)` |
| `crates/aigw-server/src/openapi.rs` | 修改 | `embeddings_spec()` + expected_endpoints 18→19 |

## 3. TDD — 单元测试（6 UT）

| # | Test | 断言 |
|---|------|------|
| 1 | `test_embedding_passthrough_returns_list` | mock upstream 返回 `{object:"list", data:[...]}`，handler 透传 object=list + data 数组 |
| 2 | `test_embedding_input_string_vs_array` | 两种 input 形态都 200，透传 |
| 3 | `test_embedding_missing_input_400` | 缺 input → 400 invalid_request_error 含 "input" |
| 4 | `test_embedding_empty_input_400` | input=[] 或 "" → 400 |
| 5 | `test_embedding_calc_spend_prompt_only` | usage `{prompt_tokens:10, total_tokens:10}`，input_cost=1e-7 → spend = 10×1e-7，completion=0 |
| 6 | `test_embedding_select_adapter_openai_passthrough` | `select_adapter(OpenAI, OpenAICompatible)` → OpenAIPassthrough；`select_adapter(OpenAI, AnthropicNative)` 被 handler 拒绝 → 400 unsupported_provider |

## 4. TDD — BDD 核心场景（~11 scenarios, `@mock`）

```gherkin
Feature: OpenAI Embeddings API Passthrough — /v1/embeddings

  Scenario: 非流式 passthrough（object=list shape）
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-user" 已生成
    When 使用 key "emb-user" 发送 POST /v1/embeddings 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "list"
    And 响应 JSON 中 "data" 数组长度大于 0
    And mock 上游收到请求

  Scenario: input 为 string
    Given ... 已配置 model "text-embedding-3-small" ...
    When 使用 key 发送 input="hello" 的 /v1/embeddings 请求
    Then 响应状态码为 200

  Scenario: input 为 array（批量）
    Given ... 已配置 model "text-embedding-3-small" ...
    When 使用 key 发送 input=["a","b"] 的 /v1/embeddings 请求
    Then 响应状态码为 200

  Scenario: /embeddings 无版本别名
    When 使用 key 发送 POST /embeddings 请求
    Then 响应状态码为 200

  Scenario: /engines/{model}/embeddings Azure 别名
    When 使用 key 发送 POST /engines/text-embedding-3-small/embeddings 请求
    Then 响应状态码为 200

  Scenario: /openai/deployments/{model}/embeddings Azure 别名
    When 使用 key 发送 POST /openai/deployments/text-embedding-3-small/embeddings 请求
    Then 响应状态码为 200

  Scenario: 缺失 model 返回 400
    When 发送 /v1/embeddings 请求不带 model
    Then 响应状态码为 400
    And 响应 JSON "error.type" 为 "invalid_request_error"
    And 响应 JSON "error.message" 包含 "model"

  Scenario: 缺失 input 返回 400
    When 发送 /v1/embeddings 请求不带 input
    Then 响应状态码为 400
    And 响应 JSON "error.type" 为 "invalid_request_error"
    And 响应 JSON "error.message" 包含 "input"

  Scenario: 空 input 返回 400
    When 发送 input=[] 的 /v1/embeddings 请求
    Then 响应状态码为 400

  Scenario: SpendLog 记录（call_type=embedding + prompt_tokens>0）
    Given mock 上游已启动 ...
    When 使用 key 发送 /v1/embeddings 请求
    Then 响应状态码为 200
    And SpendLog 中最近一条记录的 call_type 为 "embedding"
    And SpendLog 中最近一条记录的 prompt_tokens 大于 0
    And SpendLog 中最近一条记录的 completion_tokens 为 0
    And SpendLog 中最近一条记录的 total_tokens 大于 0

  Scenario: model-not-allowed（key 无此模型权限）
    Given 一个限制模型列表的 key（不含 text-embedding-3-small）
    When 使用该 key 发送 /v1/embeddings 请求
    Then 响应状态码为 403
    And 响应 JSON "error.code" 为 "model_not_allowed"

  Scenario: 上游不可达返回 502
    Given model 指向不可达上游
    When 发送 /v1/embeddings 请求
    Then 响应状态码为 502
```

## 5. mock_upstream 变更

`mock_upstream.rs` 新增 `/v1/embeddings` handler（响应 `{object:"list", data:[{object:"embedding", embedding:[0.1,0.2], index:0}], model, usage:{prompt_tokens:10, total_tokens:10}}`）+ 请求录制。复用 `e2e_steps.rs` 的 `mock 上游已启动` / `已配置 model 指向 mock 上游` Given 步骤。

## 6. 验收标准

- [ ] `task check` 编译通过
- [ ] `task test` 全量 UT 通过（含新增 6 UT）
- [ ] `task bdd` mock BDD 全绿（含新增 embeddings.feature ~11 场景）
- [ ] 三端点别名（`/embeddings`、`/engines/{model}/embeddings`、`/openai/deployments/{model}/embeddings`）均已注册并 BDD 覆盖
- [ ] openapi.json 含 `/v1/embeddings` spec，expected_endpoints 19 项
