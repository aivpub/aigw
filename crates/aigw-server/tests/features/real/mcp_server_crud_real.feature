@real_api
Feature: MCP server 管理 real BDD — 真库 DML / JSON 往返 / 级联（Stage 141）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 创建时 enabled=false 在真库往返
    Given 通过 API 创建 MCP server "real-mcp-off" 且 enabled 为 false
    Then real MCP 创建返回 200
    When 通过 API 按 search "real-mcp-off" 与 status disabled 与 page_size 10 查询 MCP server
    Then real MCP 列表 total_count 为 1
    And real MCP 列表的第一行 enabled 为 false

  Scenario: 探测 stub 后快照落库且 JSON 列往返无损
    Given 已启动 real MCP JSON-RPC stub，提供工具 "search"
    And 通过 API 创建指向该 stub 的 MCP server "real-mcp-probe"
    When 通过 API 探测该 MCP server
    Then real MCP 探测返回 200
    And real 库中该 server 的 tool catalog 行数为 1
    When 通过 API 读取该 MCP server 的 tools
    Then real MCP tools 响应工具数为 1
    And real MCP tools 的第一个工具 description 为 "search tool"
    And real MCP tools 的第一个工具 input_schema 与 stub 申报的一致
    And real MCP tools 的第一个工具 annotations 与 stub 申报的一致

  Scenario: 状态与搜索过滤同时压 count 与分页
    Given 通过 API 创建 MCP server "real-mcp-page-a" 且 enabled 为 true
    And 通过 API 创建 MCP server "real-mcp-page-b" 且 enabled 为 true
    And 通过 API 创建 MCP server "real-mcp-page-off" 且 enabled 为 false
    When 通过 API 按 search "real-mcp-page" 与 status enabled 与 page_size 2 查询 MCP server
    Then real MCP 列表 total_count 为 2
    And real MCP 列表返回行数为 2
    And real MCP 列表 total_pages 为 1
    And real MCP 列表的第一行 enabled 为 true

  Scenario: 删除 server 后 token 与 catalog 一并消失
    Given 已启动 real MCP JSON-RPC stub，提供工具 "search"
    And 通过 API 创建指向该 stub 的 MCP server "real-mcp-del"
    When 通过 API 探测该 MCP server
    Then real 库中该 server 的 tool catalog 行数为 1
    When 通过 API 为该 MCP server 添加 token
    Then real 库中该 server 的 token 行数为 1
    When 通过 API 删除该 MCP server
    Then real MCP 删除返回 200
    And real 库中该 server 的 mcp_servers 行数为 0
    And real 库中该 server 的 tool catalog 行数为 0
    And real 库中该 server 的 token 行数为 0

  Scenario: 设为默认 token 在三方言上真实生效
    Given 已启动 real MCP JSON-RPC stub，提供工具 "search"
    And 通过 API 创建指向该 stub 的 MCP server "real-mcp-default"
    When 通过 API 为该 MCP server 添加 token "real-first"
    And 通过 API 为该 MCP server 添加 token "real-second"
    And 通过 API 把 token "real-first" 设为默认
    Then real MCP 设为默认返回 200
    And real 库中该 server 的默认 token 行数为 1
    When 通过 API 把 token "real-second" 设为默认
    Then real MCP 设为默认返回 200
    And real 库中该 server 的默认 token 行数为 1
    And real 库中该 server 的 token 行数为 2

  Scenario: 直删 mcp_servers 行后 catalog 被外键级联清空
    Given 已启动 real MCP JSON-RPC stub，提供工具 "search"
    And 通过 API 创建指向该 stub 的 MCP server "real-mcp-cascade"
    When 通过 API 探测该 MCP server
    Then real 库中该 server 的 tool catalog 行数为 1
    When 真库直删 MCP server 行 "real-mcp-cascade"
    Then real 库中该 server 的 tool catalog 行数为 0
    And real 库中该 server 的 mcp_servers 行数为 0
