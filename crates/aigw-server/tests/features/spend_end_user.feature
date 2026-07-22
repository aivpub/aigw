Feature: SpendLog end_user 提取与留存

  验证 end_user / session_id / requester_ip_address 字段
  持久化到 spend_logs 表后可通过查询 API 正确返回，
  不含 metadata 的场景也不会崩溃。

  Scenario: end_user/session_id/requester_ip 写入后可通过查询读取
    Given 一个普通 key "e2u-test-key" 已生成
    And 已插入一条带 end_user 和 session_id 和 requester_ip 的 SpendLog
      """
      {"end_user":"dev-user-001","session_id":"sess-abc","requester_ip":"10.0.0.1"}
      """
    When master-key 查询 global spend logs 获取 end_user 相关 SpendLog
    Then 响应状态码为 200
    And 响应 data 第一条记录 end_user 字段存在

  Scenario: 不含 metadata 时 end_user 为空但不崩溃
    Given 一个普通 key "e2u-no-meta-key" 已生成
    And 已插入一条 SpendLog 不含 end_user
    When master-key 查询 global spend logs 获取 end_user 相关 SpendLog
    Then 响应状态码为 200
    And 响应 data 第一条记录 end_user 为空或不存在

  Scenario: spend logs 返回 requester_ip_address 字段
    Given 一个普通 key "e2u-ip-test-key" 已生成
    And 已插入一条带 end_user 和 session_id 和 requester_ip 的 SpendLog
      """
      {"end_user":"ip-test-user","session_id":"sess-xyz","requester_ip":"192.168.1.1"}
      """
    When master-key 查询 global spend logs 获取 end_user 相关 SpendLog
    Then 响应状态码为 200
    And 第一条日志的 requester_ip_address 为 "192.168.1.1"
