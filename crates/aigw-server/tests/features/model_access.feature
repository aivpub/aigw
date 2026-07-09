Feature: 模型访问控制 — Sentinel 哨兵值展开
  作为 AI Gateway 用户
  我需要通过 special model names 实现灵活的模型访问控制
  以便支持 all-team-models 和 all-proxy-models 哨兵

  Background:
    Given 管理员已认证

  Scenario: all-team-models 展开为团队模型列表
    Given 已存在团队 "sentinel-team" 允许模型 ["gpt-4", "gpt-3.5"]
    And 已存在密钥关联团队 "sentinel-key" 模型 ["all-team-models"] 团队 "sentinel-team"
    When 使用密钥 "sentinel-key" 请求 "/v1/chat/completions" 模型 "gpt-4"
    Then 模型检查通过

  Scenario: all-team-models 未关联团队返回 403
    Given 已存在独立密钥 "solo-sentinel-key" 模型 ["all-team-models"]
    When 使用密钥 "solo-sentinel-key" 请求 "/v1/chat/completions" 模型 "gpt-4"
    Then 响应状态码为 403

  Scenario: all-proxy-models 允许所有模型
    Given 已存在独立密钥 "proxy-sentinel-key" 模型 ["all-proxy-models"]
    When 使用密钥 "proxy-sentinel-key" 请求 "/v1/chat/completions" 模型 "any-model"
    Then 模型检查通过

  Scenario: 字面模型列表限制未授权模型
    Given 已存在独立密钥 "restricted-key" 模型 ["gpt-4"]
    When 使用密钥 "restricted-key" 请求 "/v1/chat/completions" 模型 "gpt-3.5"
    Then 响应状态码为 403

  Scenario: 团队 models 递归哨兵展开为全部模型
    Given 已存在团队 "recursive-team" 允许模型 ["all-team-models"]
    And 已存在密钥关联团队 "recursive-key" 模型 ["all-team-models"] 团队 "recursive-team"
    When 使用密钥 "recursive-key" 请求 "/v1/chat/completions" 模型 "any-model"
    Then 模型检查通过
