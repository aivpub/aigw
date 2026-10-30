@real_api
Feature: MCP server 自定义请求头 real BDD — 密文落库与更新语义（Stage 146）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 自定义头与额外头在真库都是密文且可还原
    When 通过 API 创建带自定义头的 MCP server "real-hdr-sealed"
    Then real MCP 创建返回 200
    And real 库中该 server 的认证头密文以 v2:gcm: 开头且不含明文
    And real 库中该 server 的额外头密文以 v2:gcm: 开头且不含明文

  Scenario: 只改显示名不会清掉已存的两个密文列
    Given 通过 API 创建带自定义头的 MCP server "real-hdr-keep"
    When 通过 API 只更新该 MCP server 的显示名
    Then real MCP 更新返回 200
    And real 库中该 server 的认证头值解密后仍为 "tapd-secret"
    And real 库中该 server 的额外头仍含 "X-Project-Id"

  Scenario: 空串与空数组清除两列
    Given 通过 API 创建带自定义头的 MCP server "real-hdr-clear"
    When 通过 API 用空认证头名与空额外头更新该 MCP server
    Then real MCP 更新返回 200
    And real 库中该 server 的认证头与额外头均为 null

  Scenario: 读接口在真库路径上同样脱敏
    Given 通过 API 创建带自定义头的 MCP server "real-hdr-redacted"
    When 通过 API 按 id 读取该 MCP server
    Then real MCP 读取返回 200
    And real MCP 响应里认证头值为 "***" 且不含明文
