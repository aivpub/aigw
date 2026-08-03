Feature: 密钥创建权限与归属验证
  作为 API 网关
  我需要验证管理员和普通用户的密钥创建权限
  以确保密钥归属正确且权限隔离有效

  Background:
    Given 管理员已认证

  Scenario: 普通用户创建密钥 — 自动归属自身
    Given 已存在用户 "u01" 邮箱 "u01@test.com" 角色 "internal_user"
    When 用户 "u01" 创建 key 请求 body:
      """
      {"key_alias": "my-key", "models": ["gpt-4"]}
      """
    Then 响应状态码为 200
    And 响应包含 key 字段
    And 响应 JSON "user_id" 字段值为 "u01"

  Scenario: 普通用户创建密钥 — 指定他人被拒绝
    Given 已存在用户 "u02" 邮箱 "u02@test.com" 角色 "internal_user"
    When 用户 "u02" 创建 key 请求 body:
      """
      {"key_alias": "for-other", "user_id": "someone-else", "models": ["gpt-4"]}
      """
    Then 响应状态码为 403

  Scenario: 管理员创建密钥 — 可指定任意用户
    When 管理员创建 key 请求 body:
      """
      {"key_alias": "admin-for-u99", "user_id": "u99", "models": ["gpt-4"]}
      """
    Then 响应状态码为 200
    And 响应包含 key 字段
    And 响应 JSON "user_id" 字段值为 "u99"

  Scenario: 管理员创建密钥 — 不指定 user_id 也可
    When 管理员创建 key 请求 body:
      """
      {"key_alias": "admin-no-user", "models": ["gpt-4"]}
      """
    Then 响应状态码为 200
    And 响应包含 key 字段

  Scenario: 普通用户列出密钥 — 只看到自己的
    Given 已存在用户 "uA" 邮箱 "ua@test.com" 角色 "internal_user"
    And 已存在用户 "uB" 邮箱 "ub@test.com" 角色 "internal_user"
    And 管理员已创建 key "uA-key" 归属用户 "uA"
    And 管理员已创建 key "uB-key" 归属用户 "uB"
    When 用户 "uA" 查询 key 列表
    Then 响应状态码为 200
    And 响应 key 列表仅包含 "1" 个 key
    And 响应 key 列表中的 user_id 均为 "uA"

  Scenario: 普通用户查询他人密钥 — 被拒绝
    Given 已存在用户 "uC" 邮箱 "uc@test.com" 角色 "internal_user"
    And 管理员已创建 key "uA-key" 归属用户 "uA"
    When 用户 "uC" 查询 key "uA-key"
    Then 响应状态码为 403
