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

  # ── Stage 127: Token 生命周期 + 三层自愈 ──

  Scenario: 缓存命中返回 token 不刷新
    Given 已存在 OAuth 凭证 "oauth-token-cache" 用于 token 获取
    When 通过 TokenProvider 获取该凭证的 token
    Then token 获取结果为 "sk-ant-access-refreshed"

  Scenario: refresh 失效后 cookie 自愈拿到新 token
    Given 已存在 OAuth 凭证 "oauth-token-heal" 其 refresh_token 已失效
    When 通过 TokenProvider 获取该凭证的 token
    Then token 获取结果为 "sk-ant-access-refreshed"

  # ── Stage 128: 反代管线 ──

  Scenario: messages 走 OAuth 反代（Bearer + billing 块 + 代理出口）
    Given 已存在 OAuth 凭证 "oauth-pipeline-messages" 用于反代
    And 已配置 model "claude-oauth-mock" 引用 OAuth 凭证 "oauth-pipeline-messages"
    When 发送 POST /v1/messages 请求带认证 model="claude-oauth-mock" 走 OAuth 反代
    Then 响应状态码为 200
    And mock 上游收到 /v1/messages 请求且 Authorization 为 Bearer token
    And mock 上游收到的 body 首条 system 块为 billing 块

  Scenario: chat/completions → OAuth 反代（转换 + 同一管线）
    Given 已存在 OAuth 凭证 "oauth-pipeline-chat" 用于反代
    And 已配置 model "claude-oauth-chat" 引用 OAuth 凭证 "oauth-pipeline-chat"
    When 使用 master-key 发送 POST /chat/completions 请求用 model "claude-oauth-chat" 走 OAuth 反代
    Then 响应状态码为 200
    And mock 上游收到 /v1/messages 请求且 Authorization 为 Bearer token

  Scenario: embeddings 解析到 OAuth 凭证返回 400
    Given 已存在 OAuth 凭证 "oauth-pipeline-embed" 用于反代
    And 已配置 model "claude-oauth-embed" 引用 OAuth 凭证 "oauth-pipeline-embed"
    When 使用 master-key 发送 POST /v1/embeddings 请求用 model "claude-oauth-embed" 走 OAuth 反代
    Then 响应状态码为 400
    And 错误信息包含 "不支持 embeddings"

  Scenario: 401 → 刷新重试成功
    Given 已存在 OAuth 凭证 "oauth-pipeline-401" 用于反代
    And 已配置 model "claude-oauth-401" 引用 OAuth 凭证 "oauth-pipeline-401"
    And mock 上游 /v1/messages 首次返回 401
    When 发送 POST /v1/messages 请求带认证 model="claude-oauth-401" 走 OAuth 反代
    Then 响应状态码为 200
    And mock 上游 /v1/messages 请求次数为 2
