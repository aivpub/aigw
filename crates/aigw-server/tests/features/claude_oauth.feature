@mock
Feature: Claude OAuth 凭证交换 — /credential/oauth/exchange（Stage 126）

  Background:
    Given mock 上游已启动
    And mock 上游 OAuth 已配置
    And 数据库已初始化且已配置 master key（OAuth 场景）

  Scenario: cookie 换 token 成功并持久化 OAuth 凭证（敏感字段 redact）
    When 发送 POST /credential/oauth/exchange 请求
      """
      {"name": "oauth-cred-1", "session_key": "sk-ant-sid-test-123", "proxy_id": null}
      """
    Then 响应状态码为 200
    And 响应凭证 type 为 "anthropic_oauth"
    And 响应凭证敏感字段已 redact

  Scenario: 无效 cookie 返回结构化错误
    Given mock 上游 OAuth 组织接口返回 401
    When 发送 POST /credential/oauth/exchange 请求
      """
      {"name": "oauth-cred-bad", "session_key": "sk-ant-sid-bad", "proxy_id": null}
      """
    Then 响应状态码为 403
    And 响应错误 kind 为 "unauthorized"

  Scenario: 绑定不存在的代理返回 400
    When 发送 POST /credential/oauth/exchange 请求
      """
      {"name": "oauth-cred-noproxy", "session_key": "sk-ant-sid-1", "proxy_id": 99999}
      """
    Then 响应状态码为 400

  Scenario: 敏感字段响应 redact（access/refresh/session 掩码）
    Given 已存在 OAuth 凭证 "oauth-cred-redact"
    When 发送 GET /credential/info 请求查询该凭证
    Then 响应状态码为 200
    And 响应凭证敏感字段已 redact
