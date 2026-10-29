@mock
Feature: MCP server 管理 CRUD — /admin/mcp/*（Stage 141）

  Background:
    Given 管理员已认证

  Scenario: 创建 server 后列表可见且尚未探测
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/mcp/servers 请求创建 MCP server "docs" 且 enabled 为 true
    Then 响应状态码为 200
    And 响应包含 alias 字段值为 "docs"
    And 响应 enabled 为 true
    When 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 200
    And MCP server 总数为 1
    And MCP server 列表工具数为 0

  Scenario: 创建时关闭开关得到停用的 server
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/mcp/servers 请求创建 MCP server "off" 且 enabled 为 false
    Then 响应状态码为 200
    And 响应 enabled 为 false
    When 发送 GET /admin/mcp/servers 请求
    Then MCP server 列表 enabled 为 false

  Scenario: alias 含双下划线被拒绝
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/mcp/servers 请求创建 MCP server "bad__alias" 且 enabled 为 true
    Then 响应状态码为 400
    And 响应错误 type 为 "invalid_alias"

  Scenario: 探测 stub 上游后工具数出现在列表里
    Given 数据库已初始化且已配置 master key
    And 已启动提供 2 个工具的 MCP JSON-RPC stub
    And 已通过 API 创建指向该 stub 的 MCP server "probed"
    When 发送 POST /admin/mcp/servers/1/probe 请求
    Then 响应状态码为 200
    And 探测协议版本为 "2026-07-28"
    And 探测工具数为 2
    When 发送 GET /admin/mcp/servers/1/tools 请求
    Then 响应状态码为 200
    And tools 响应工具数为 2
    And tools 响应 fetched_at 非空
    When 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 200
    And MCP server 列表工具数为 2

  Scenario: 添加 token 响应脱敏且落库为密文
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建 MCP server "with-token"
    When 发送 POST /admin/mcp/servers/1/tokens 请求添加 token
      """
      {"token_name": "shared", "token_value": "sk-upstream-secret"}
      """
    Then 响应状态码为 200
    And token 响应 token_redacted 为 "***"
    And 响应 JSON 不含明文 "sk-upstream-secret"
    And 数据库存储的 token 带 v2:gcm: 前缀

  Scenario: 置第二条为默认后只有一条默认
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建 MCP server "two-tokens"
    When 发送 POST /admin/mcp/servers/1/tokens 请求添加 token
      """
      {"token_name": "one", "token_value": "sk-one", "is_default": true}
      """
    Then 响应状态码为 200
    When 发送 POST /admin/mcp/servers/1/tokens 请求添加 token
      """
      {"token_name": "two", "token_value": "sk-two"}
      """
    Then 响应状态码为 200
    When 发送 POST /admin/mcp/servers/1/tokens/1/default 请求
    Then 响应状态码为 200
    And token 列表中仅有一条 is_default

  Scenario: 停用后按状态过滤只剩停用的 server
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建 MCP server "switch-me"
    When 发送 POST /admin/mcp/servers/1/toggle 请求
    Then 响应状态码为 200
    And 响应 enabled 为 false
    When 发送 GET /admin/mcp/servers 且 status=disabled 请求
    Then 响应状态码为 200
    And MCP server 总数为 1
    When 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 200
    And MCP server 总数为 1

  Scenario: 删除 server 后列表为空
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建 MCP server "to-delete"
    When 发送 DELETE /admin/mcp/servers/1 请求
    Then 响应状态码为 200
    When 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 200
    And MCP server 总数为 0

  Scenario: 非 admin key 访问 MCP 管理接口返回 403
    Given 数据库已初始化且已配置 master key
    And 已生成普通 key "mcp-nonadmin"（MCP 场景）
    When 使用普通 key "mcp-nonadmin" 发送 GET /admin/mcp/servers 请求
    Then 响应状态码为 403
    And 响应错误 type 为 "forbidden"
