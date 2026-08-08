@mock
Feature: OpenAI Embeddings API Passthrough — /v1/embeddings (四端点)

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
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-str" 已生成
    When 使用 key "emb-str" 发送 input="hello" 的 /v1/embeddings 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "list"

  Scenario: input 为 array（批量）
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-arr" 已生成
    When 使用 key "emb-arr" 发送数组 input 的 /v1/embeddings 请求
    Then 响应状态码为 200

  Scenario: /embeddings 无版本别名
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-alias" 已生成
    When 使用 key "emb-alias" 发送 POST /embeddings 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "list"

  Scenario: /engines/{model}/embeddings Azure 别名
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-engine" 已生成
    When 使用 key "emb-engine" 发送 POST /engines/text-embedding-3-small/embeddings 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "list"

  Scenario: /openai/deployments/{model}/embeddings Azure 别名
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-deploy" 已生成
    When 使用 key "emb-deploy" 发送 POST /openai/deployments/text-embedding-3-small/embeddings 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "list"

  Scenario: 缺失 model 返回 400
    Given 一个普通 key "emb-no-model" 已生成
    When 使用 key "emb-no-model" 发送 POST /v1/embeddings 请求不带 model
    Then 响应状态码为 400
    And 错误 type 为 "invalid_request_error"
    And 错误信息包含 "model"

  Scenario: 缺失 input 返回 400
    Given 一个普通 key "emb-no-input" 已生成
    When 使用 key "emb-no-input" 发送 POST /v1/embeddings 请求不带 input
    Then 响应状态码为 400
    And 错误 type 为 "invalid_request_error"
    And 错误信息包含 "input"

  Scenario: 空 input 返回 400
    Given 一个普通 key "emb-empty" 已生成
    When 使用 key "emb-empty" 发送 POST /v1/embeddings 请求带空 input
    Then 响应状态码为 400
    And 错误信息包含 "empty"

  Scenario: SpendLog 记录（call_type=embedding + prompt-only）
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-spend" 已生成
    When 使用 key "emb-spend" 发送 POST /v1/embeddings 请求
    Then 响应状态码为 200
    And SpendLog 中最近一条记录的 call_type 为 "embedding"
    And SpendLog 中最近一条记录的 prompt_tokens 大于 0
    And SpendLog 中最近一条记录的 completion_tokens 为 0
    And SpendLog 中最近一条记录的 total_tokens 大于 0

  Scenario: model-not-allowed（key 无此模型权限）
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-deny" 已生成且绑定模型 "gpt-4"
    When 使用 key "emb-deny" 发送 POST /v1/embeddings 请求
    Then 响应状态码为 403
    And 错误信息包含 "not allowed"
