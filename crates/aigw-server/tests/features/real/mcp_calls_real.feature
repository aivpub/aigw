@real_api
Feature: MCP 调用日志按用户隔离 real BDD — /v1/mcp/calls 与 034 索引（Stage 147）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 两个用户的调用各查各的，admin 可按用户过滤
    Given 通过 API 创建指向 real 透传 stub 的 MCP server "real-calls-scope" 并配置共享 token
    And 真库调用日志场景以用户 "real-calls-a" 的 key "real-calls-key-a" 为准
    And 真库调用日志场景以用户 "real-calls-b" 的 key "real-calls-key-b" 为准
    When 用真实 key "real-calls-key-a" 对 "real-calls-scope" 发送 tools/list
    And 用真实 key "real-calls-key-b" 对 "real-calls-scope" 发送 tools/list
    Then 用真实 key "real-calls-key-a" 查询 /v1/mcp/calls 该 alias 的 total_count 为 1
    And 用真实 key "real-calls-key-b" 查询 /v1/mcp/calls 该 alias 的 total_count 为 1
    And 用真实 key "real-calls-key-a" 带别人的 user_id 查询 /v1/mcp/calls 该 alias 的 total_count 仍为 1
    And 用 master key 查询 /admin/mcp/calls 按 user_id "real-calls-a" 得到的 total_count 为 1
    And 用 master key 查询 /admin/mcp/calls 按该 alias 得到的 total_count 为 2

  Scenario: 未绑定用户的 key 直接访问用户接口是 400
    When 用 master key 查询 /v1/mcp/calls
    Then 响应状态码为 400
    And 响应错误 type 为 "key_without_user"

  Scenario: 034 的用户索引按 (user_id, created_at) 落库
    Then 真库 mcp_call_log 上存在以 user_id 为前导列、created_at 为次列的索引 "idx_mcp_call_log_user_created"
