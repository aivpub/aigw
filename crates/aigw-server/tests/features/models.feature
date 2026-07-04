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
