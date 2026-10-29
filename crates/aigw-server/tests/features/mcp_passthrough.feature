@mock
Feature: MCP 透传端点 — /mcp/{alias} 与 /mcp-rest/*（Stage 143）

  Background:
    Given 管理员已认证
    And 数据库已初始化且已配置 master key

  # ── 调试口 ───────────────────────────────────────────────

  Scenario: tools/list 走缓存目录且不触发上游协商
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "rest-demo"
    And 已为该 MCP server 缓存一个工具目录
    When 发送 /mcp-rest/tools/list 请求且 alias 为 "rest-demo"
    Then 响应状态码为 200
    And 响应 JSON 路径 "tools.0.tool_name" 为字符串 "ping"
    And 响应 JSON 路径 "servers.0.tool_count" 为数字 1
    And 上游没有收到任何 MCP 请求
    And 该 server 的 mcp_call_log 行数为 1
    And 最近一条 mcp_call_log 的 method 为 "tools/list"
    And MCP 响应头 x-call-id 与最近一条 mcp_call_log 的 call_id 一致

  Scenario: tools/call 把解密后的 token 注入出站 Authorization
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "rest-call"
    When 发送 /mcp-rest/tools/call 请求且 alias 为 "rest-call" 工具为 "ping"
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.content.0.text" 为字符串 "pong"
    And 上游依次收到 MCP 方法 "tools/call"
    And 上游收到的 Authorization 均为 "sk-upstream-shared"
    And 该 server 的 mcp_call_log 行数为 1
    And 最近一条 mcp_call_log 的 token_owner 为 "server"
    And 最近一条 mcp_call_log 的 request_bytes 大于 0

  Scenario: 普通 key 调用 tools/call 不会把自己的 key 发给上游
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "rest-key"
    And 已生成用于 MCP 透传的普通 key "mcp-pass-user"
    When 用普通 key 对 "rest-key" 发送 MCP 透传请求 "tools/call"
    Then 响应状态码为 200
    And 上游收到的 Authorization 均为 "sk-upstream-shared"
    And 最近一条 mcp_call_log 的 status 为 "success"

  # ── 单 server 透传 ────────────────────────────────────────

  Scenario: 现代客户端握手由网关本地作答
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "modern-demo"
    When 用 master key 对 "modern-demo" 发送 MCP 透传请求 "server/discover"
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.supportedVersions" 声明支持的协议版本
    And MCP 响应头不包含 Mcp-Session-Id
    And 上游没有收到任何 MCP 请求

  Scenario: 旧版客户端握手拿到会话头且同样不触达上游
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "legacy-client"
    When 对 "legacy-client" 发送旧版 initialize 请求
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.protocolVersion" 为字符串 "2025-06-18"
    And MCP 响应头包含 Mcp-Session-Id
    And 上游没有收到任何 MCP 请求

  Scenario: 旧版上游自己 initialize 且客户端会话头被丢弃
    Given 已启动旧版协议的 MCP 透传 stub 并注册为未探测的 server "legacy-upstream"
    When 客户端带会话头对 "legacy-upstream" 发送 MCP 透传请求 "tools/call"
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.content.0.text" 为字符串 "pong"
    And 上游依次收到 MCP 方法 "initialize, notifications/initialized, tools/call"
    And 上游收到的 Mcp-Session-Id 只可能是它自己签发的 "legacy-session-1"

  Scenario: 现代上游看到的只有目标方法
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "modern-upstream"
    When 用 master key 对 "modern-upstream" 发送 MCP 透传请求 "tools/call"
    Then 响应状态码为 200
    And 上游依次收到 MCP 方法 "tools/call"
    And 上游收到的 Authorization 均为 "sk-upstream-shared"
    And MCP 响应头 x-call-id 与最近一条 mcp_call_log 的 call_id 一致

  Scenario: 现代协议下 GET 返回 405
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "modern-get"
    When 对 "modern-get" 发送 MCP GET 请求
    Then 响应状态码为 405

  Scenario: Mcp-Name 与 body 工具名不一致是 400
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "mismatch"
    When 对 "mismatch" 发送 MCP 请求且 Mcp-Name 头为 "read" 而 body 工具名为 "ping"
    Then 响应状态码为 400
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32020
    And 最近一条 mcp_call_log 的 status 为 "failed"

  # ── ACL ──────────────────────────────────────────────────
  # 聚合端点的 ACL 由 mcp_aggregate.feature 覆盖；这里钉的是另外两个入口，
  # 它们同样能调到同一个上游，绕过去等于 allowlist 形同虚设。

  Scenario: ACL 拒绝在单 server 路径上同样生效
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "acl-alias"
    And 已生成用于 MCP 透传的普通 key "mcp-pass-user"
    And 管理员把透传 server "acl-alias" 的工具 "ping" 对普通 key 设为禁止
    When 用普通 key 对 "acl-alias" 发送 MCP 透传请求 "tools/call"
    Then 响应状态码为 403
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32003
    And 上游没有收到任何 MCP 请求
    And 该 server 的 mcp_call_log 行数为 1
    And 最近一条 mcp_call_log 的 status 为 "failed"

  Scenario: ACL 拒绝在调试口上同样生效
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "acl-rest"
    And 已生成用于 MCP 透传的普通 key "mcp-pass-user"
    And 管理员把透传 server "acl-rest" 的工具 "ping" 对普通 key 设为禁止
    When 用普通 key 发送 /mcp-rest/tools/call 请求且 alias 为 "acl-rest" 工具为 "ping"
    Then 响应状态码为 403
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32003
    And 上游没有收到任何 MCP 请求
    And 该 server 的 mcp_call_log 行数为 1
    And 最近一条 mcp_call_log 的 status 为 "failed"

  # 正对照：allowlist 非空但没提这个工具 —— 仍是放行，ACL 不是默认拒绝。

  Scenario: 未被禁的工具在单 server 路径上仍然放行
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "acl-ok"
    And 已生成用于 MCP 透传的普通 key "mcp-pass-user"
    And 管理员把透传 server "acl-ok" 的工具 "write" 对普通 key 设为禁止
    When 用普通 key 对 "acl-ok" 发送 MCP 透传请求 "tools/call"
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.content.0.text" 为字符串 "pong"
    And 上游依次收到 MCP 方法 "tools/call"
    And 最近一条 mcp_call_log 的 status 为 "success"

  Scenario: 未被禁的工具在调试口上仍然放行
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "acl-rest-ok"
    And 已生成用于 MCP 透传的普通 key "mcp-pass-user"
    And 管理员把透传 server "acl-rest-ok" 的工具 "write" 对普通 key 设为禁止
    When 用普通 key 发送 /mcp-rest/tools/call 请求且 alias 为 "acl-rest-ok" 工具为 "ping"
    Then 响应状态码为 200
    And 响应 JSON 路径 "result.content.0.text" 为字符串 "pong"
    And 上游依次收到 MCP 方法 "tools/call"
    And 最近一条 mcp_call_log 的 status 为 "success"

  # ── 未认证 / 未知 alias ───────────────────────────────────

  Scenario: 未认证请求是 401
    Given 已启动现代协议的 MCP 透传 stub 并注册为 server "no-auth"
    When 发送未认证的 MCP 请求到 "no-auth"
    Then 响应状态码为 401

  Scenario: 未知 alias 是 404 且仍然留痕
    Given MCP 透传场景以未注册的 alias "ghost" 为准
    When 用 master key 对 "ghost" 发送 MCP 透传请求 "server/discover"
    Then 响应状态码为 404
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32601
    And 该 server 的 mcp_call_log 行数为 1
    And 最近一条 mcp_call_log 的 status 为 "failed"
