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
    When 发送 POST /key/generate 请求（无认证）
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

  Scenario: 认证使用 hash 而非明文
    Given 已存在 1 个 key
    When 直接查询 virtual_keys 表
    Then token 列存储的是 SHA256 hash
    And token 列不等于明文 key
