@real_api
Feature: MCP 聚合 real BDD — mcp_tool_allowlist / mcp_call_log 三方言（Stage 144）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 整表替换写入的 ACL 在真库往返
    Given 通过 API 创建聚合场景的 MCP server "real-agg-acl"
    When 通过 API 把该 server 的 allowlist 整表替换为一条禁止 key 规则
    Then 响应状态码为 200
    And 真库该 server 的 allowlist 行数为 1
    And 真库该 server 的 allowlist 首行 allowed 为 0
    When 通过 API 读取该 server 的 allowlist
    Then 响应状态码为 200
    And allowlist 响应 count 为 1
    And allowlist 响应首行 scope_type 为 "key"
    And allowlist 响应首行 allowed 为 false

  Scenario: 第二次整表替换删掉旧规则而不是追加
    Given 通过 API 创建聚合场景的 MCP server "real-agg-acl-replace"
    When 通过 API 把该 server 的 allowlist 整表替换为一条禁止 key 规则
    Then 响应状态码为 200
    When 通过 API 把该 server 的 allowlist 整表替换为一条团队允许规则
    Then 响应状态码为 200
    And 真库该 server 的 allowlist 行数为 1
    And 真库该 server 的 allowlist 首行 allowed 为 1
    And allowlist 响应首行 scope_type 为 "team"

  Scenario: 经聚合端点的调用把日志写进真库并能按 alias 查回
    Given 已启动 real 聚合 stub，提供工具 "search"
    And 通过 API 创建指向该 stub 的聚合 server "real-agg-call"
    When 通过 API 经聚合端点调用 "real-agg-call" 的工具 "search"
    Then 响应状态码为 200
    And 真库该 alias 的 mcp_call_log 行数为 1
    And 真库该 alias 最近一行 method 为 "tools/call"
    When 按该 alias 查询 real 调用日志
    Then 响应状态码为 200
    And real 调用日志 total_count 为 1
    And real 调用日志首行 server_alias 为 "real-agg-call"
