@mock
Feature: MCP server 自定义出站认证头与额外请求头 — /admin/mcp/servers（Stage 146）

  Background:
    Given 管理员已认证
    And 数据库已初始化且已配置 master key

  Scenario: 自定义认证头以裸 token 到达上游且不发 Authorization
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "tapd" 并携带字段
      """
      {"auth_header_name": "X-Tapd-Access-Token", "auth_header_value": "tapd-secret"}
      """
    Then 响应状态码为 200
    When 通过 /mcp-rest 调用 "tapd" 的工具 "ping"
    Then 响应状态码为 200
    And 该 stub 的最后一个请求带 "X-Tapd-Access-Token" 为 "tapd-secret"
    And 该 stub 的最后一个请求不带 "Authorization"

  Scenario: 额外请求头与协议 Bearer 一起发出
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "wiki" 并携带字段
      """
      {"extra_headers": [{"name": "X-Project-Id", "value": "12345"}]}
      """
    Then 响应状态码为 200
    When 通过 API 为该 server 添加共享 token "sk-upstream-shared"
    Then 响应状态码为 200
    When 通过 /mcp-rest 调用 "wiki" 的工具 "ping"
    Then 响应状态码为 200
    And 该 stub 的最后一个请求带 "Authorization" 为 "Bearer sk-upstream-shared"
    And 该 stub 的最后一个请求带 "X-Project-Id" 为 "12345"

  Scenario: 未配新字段的 server 出站头与之前一致
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "plain" 并携带字段
      """
      {}
      """
    Then 响应状态码为 200
    When 通过 API 为该 server 添加共享 token "sk-upstream-shared"
    Then 响应状态码为 200
    When 通过 /mcp-rest 调用 "plain" 的工具 "ping"
    Then 响应状态码为 200
    And 该 stub 的最后一个请求只带协议认证头

  Scenario: 探测与真实调用发出同一套头
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "same-stack" 并携带字段
      """
      {"auth_header_name": "X-Tapd-Access-Token", "auth_header_value": "tapd-secret",
       "extra_headers": [{"name": "X-Project-Id", "value": "12345"}]}
      """
    Then 响应状态码为 200
    When 通过 API 探测该 server
    Then 响应状态码为 200
    When 通过 /mcp-rest 调用 "same-stack" 的工具 "ping"
    Then 响应状态码为 200
    And 探测与调用的出站认证头与额外头一致

  Scenario: 受保护头名被拒绝且上游零请求
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 尝试创建指向该 stub 的 server "reserved" 并携带字段
      """
      {"auth_header_name": "Mcp-Session-Id", "auth_header_value": "x"}
      """
    Then 响应状态码为 400
    And 响应错误 type 为 "invalid_header_name"
    And 该 stub 没有收到任何请求

  Scenario: 额外头与认证头重名被拒绝
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 尝试创建指向该 stub 的 server "dup" 并携带字段
      """
      {"auth_header_name": "X-Token", "auth_header_value": "a",
       "extra_headers": [{"name": "x-token", "value": "b"}]}
      """
    Then 响应状态码为 400
    And 响应错误 type 为 "duplicate_header_name"
    And 该 stub 没有收到任何请求

  Scenario: 配置了头名却没配值被拒绝
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 尝试创建指向该 stub 的 server "no-value" 并携带字段
      """
      {"auth_header_name": "X-Token"}
      """
    Then 响应状态码为 400
    And 响应错误 type 为 "auth_value_required"

  Scenario: 读接口把两个密文列都脱敏
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "redacted" 并携带字段
      """
      {"auth_header_name": "X-Tapd-Access-Token", "auth_header_value": "tapd-secret",
       "extra_headers": [{"name": "X-Project-Id", "value": "12345"}]}
      """
    Then 响应状态码为 200
    And 创建响应里认证头值脱敏且认证头名可见
    And 响应 JSON 不含明文 "tapd-secret"
    And 响应 JSON 不含明文 "12345"
    When 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 200
    And MCP server 列表第一行的认证头值脱敏
    And 响应 JSON 不含明文 "tapd-secret"
    And 响应 JSON 不含明文 "v2:gcm:"

  Scenario: 值缺省不改 空串清除
    Given 已启动记录出站请求头的 MCP stub
    When 通过 API 创建指向该 stub 的 server "edit-me" 并携带字段
      """
      {"auth_header_name": "X-Tapd-Access-Token", "auth_header_value": "tapd-secret",
       "extra_headers": [{"name": "X-Project-Id", "value": "12345"}]}
      """
    Then 响应状态码为 200
    When 通过 API 更新该 server 并携带字段
      """
      {"display_name": "renamed"}
      """
    Then 响应状态码为 200
    And 该 server 的认证头名仍为 "X-Tapd-Access-Token"
    When 通过 API 更新该 server 并携带字段
      """
      {"auth_header_name": "", "extra_headers": []}
      """
    Then 响应状态码为 200
    And 该 server 的认证头与额外头已清空
    When 通过 API 为该 server 添加共享 token "sk-upstream-shared"
    Then 响应状态码为 200
    When 通过 /mcp-rest 调用 "edit-me" 的工具 "ping"
    Then 响应状态码为 200
    And 该 stub 的最后一个请求带 "Authorization" 为 "Bearer sk-upstream-shared"
    And 该 stub 的最后一个请求不带 "X-Tapd-Access-Token"
    And 该 stub 的最后一个请求不带 "X-Project-Id"
