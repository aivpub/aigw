@real_api
Feature: MCP 透传 real BDD — mcp_call_log 三方言插入与查询（Stage 143）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  # 每个场景用自己的 alias，行数断言才不受前一个场景影响。

  Scenario: tools/call 的真库账单行带完整计数
    Given 通过 API 创建指向 real 透传 stub 的 MCP server "real-pass-call" 并配置共享 token
    And 真库 mcp_call_log 中该 alias 的行为 0
    When 通过 API 对 "real-pass-call" 调用 MCP tools/call 且工具为 "search"
    Then 响应状态码为 200
    And 真库 mcp_call_log 中该 alias 的行数为 1
    And 真库最近一行 mcp_call_log 的 method 为 "tools/call"
    And 真库最近一行 mcp_call_log 的 status 为 "success"
    And 真库最近一行 mcp_call_log 的 request_bytes 为正整数
    And 真库最近一行 mcp_call_log 的 call_id 与 x-call-id 响应头一致

  Scenario: 现代握手只写一行且不触达上游
    Given 通过 API 创建指向 real 透传 stub 的 MCP server "real-pass-discover" 并配置共享 token
    And 真库 mcp_call_log 中该 alias 的行为 0
    When 通过 API 对 "real-pass-discover" 发送现代握手
    Then 响应状态码为 200
    And 真库 mcp_call_log 中该 alias 的行数为 1
    And 真库最近一行 mcp_call_log 的 method 为 "server/discover"
    And 真库最近一行 mcp_call_log 的 status 为 "success"
    And 真库最近一行 mcp_call_log 的 call_id 与 x-call-id 响应头一致

  Scenario: 未知 alias 的失败调用同样落库
    Given 真库 MCP 透传场景以未注册的 alias "real-pass-ghost-missing" 为准
    When 通过 API 对未注册的 alias "real-pass-ghost-missing" 调用 MCP tools/call
    Then 响应状态码为 404
    And 真库 mcp_call_log 中该 alias 的行数为 1
    And 真库最近一行 mcp_call_log 的 status 为 "failed"
