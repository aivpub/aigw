Feature: 模型管理 CRUD
  作为管理员
  我需要管理 proxy models
  以便配置 AI 模型部署

  Scenario: 创建新模型
    Given 管理员已认证
    When 发送 POST /model/new 请求
      """
      {"model_name": "gpt-4-proxy", "litellm_params": {"model": "openai/gpt-4", "api_base": "https://api.openai.com"}}
      """
    Then 响应状态码为 200
    And 响应包含 model_id 字段
    And 响应包含 model_name 字段值为 gpt-4-proxy

  Scenario: 未认证创建模型被拒绝
    When 发送 POST /model/new 请求（无认证）
      """
      {"model_name": "test-model", "litellm_params": {"model": "openai/gpt-4"}}
      """
    Then 响应状态码为 401

  Scenario: 查询模型详情
    Given 已存在模型 "query-model"
    When 发送 GET /model/info 请求查询该模型
    Then 响应状态码为 200
    And 响应包含 model_name 字段值为 query-model

  Scenario: 查询不存在的模型
    Given 管理员已认证
    When 发送 GET /model/info?model_id=nonexistent-id
    Then 响应状态码为 404

  Scenario: 列出所有模型
    Given 已存在 3 个模型
    When 发送 GET /model/list
    Then 响应状态码为 200
    And 响应中的 data 包含 3 个模型

  Scenario: 更新模型
    Given 已存在模型 "update-model"
    When 发送 PUT /model/update 请求更新模型名称为 updated-model
    Then 响应状态码为 200
    And 响应包含 model_name 字段值为 updated-model

  Scenario: 删除模型
    Given 已存在模型 "delete-model"
    When 发送 DELETE /model/delete 请求删除该模型
    Then 响应状态码为 200
    And 该模型不再存在

  Scenario: /model/list 解密 litellm_params 嵌套加密字段
    Given 管理员已认证
    And 已存在一个模型其 litellm_params 包含加密的 api_base 和 api_key
    When 通过解密路由发送 GET /model/list
    Then 响应状态码为 200
    And 响应中首个模型的 api_base 已解密为 "https://decrypted-api.example.com"
    And 响应中首个模型的 api_key 已解密为 "sk-decrypted-secret"

  Scenario: /v1/models 暴露多模态模型的 model_info.mode
    Given 管理员已认证
    And 已存在多模态模型 "qwen3.5-vl" 其 model_info.mode 为 "image"
    When 发送 GET /v1/models 请求
    Then 响应状态码为 200
    And /v1/models 中模型 "qwen3.5-vl" 的 model_info.mode 为 "image"

  Scenario: /v1/models 非 master 权限省略 model_info
    Given 管理员已认证
    And 已存在多模态模型 "qwen3.5-vl" 其 model_info.mode 为 "image"
    And 一个普通 key "models-key-user" 已生成且绑定模型 "qwen3.5-vl"
    When 使用普通 key "models-key-user" 发送 GET /v1/models 请求
    Then 响应状态码为 200
    And /v1/models 不返回 model_info 字段

  # ── Stage 112: embedding 模型注册 + /v1/models 展示 + /v1/embeddings 全链路 ──

  Scenario: 注册 embedding 模型（mode=embed）并在 /v1/models 展示
    Given 管理员已认证
    When 发送 POST /model/new 请求
      """
      {"model_name": "text-embedding-3-small", "litellm_params": {"model": "openai/text-embedding-3-small", "api_base": "https://api.openai.com"}, "model_info": {"mode": "embed", "input_cost_per_token": 0.00000002}}
      """
    Then 响应状态码为 200
    When 发送 GET /v1/models 请求
    Then 响应状态码为 200
    And /v1/models 中模型 "text-embedding-3-small" 的 model_info.mode 为 "embed"

  Scenario: embedding 模型走 /v1/embeddings 生成 SpendLog call_type=embedding
    Given mock 上游已启动
    And 已配置 model "text-embedding-3-small" 指向 mock 上游
    And 一个普通 key "emb-model-full" 已生成
    When 使用 key "emb-model-full" 发送 POST /v1/embeddings 请求
    Then 响应状态码为 200
    And SpendLog 中最近一条记录的 call_type 为 "embedding"

  # ── Stage 116: config.yaml model_list seed（静态配置模型接入）──

  Scenario: config.yaml model_list seed 到 proxy_models 并在 /v1/models 展示
    Given 通过 config_loader 从 model_list seed 模型 "seed-from-config"
    When 发送 GET /v1/models 请求
    Then 响应状态码为 200
    And /v1/models 中模型 "seed-from-config" 的 model_info.mode 为 "chat"

  Scenario: config.yaml model_list seed 幂等且不覆盖已有模型
    Given 通过 config_loader 从 model_list seed 模型 "seed-idem-config"
    And 通过 config_loader 从 model_list seed 模型 "seed-idem-config"
    When 发送 GET /model/list
    Then 响应状态码为 200
    And 响应中的 data 包含 1 个模型

