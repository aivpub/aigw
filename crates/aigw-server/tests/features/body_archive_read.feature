Feature: Body Archive 读链路

  # ━━━━ Stage 79: Query Router + Footer Cache ━━━━
  # 需要真实 SQLite DB + mock object_store（或 MinIO/FS 真实存储）

  Background:
    Given spend_logs 表已包含 body_archived 和 parquet_path 列
    And BodyArchiver 已启用，Footer 缓存已初始化
    And mock 存储后端中有 Parquet 文件包含测试数据

  # ── 查询路由 ──

  Scenario: 热数据 — DB 中有 body → 直接返回，不查存储
    Given spend_logs 中有 request_id="hot-001"，messages 不为 NULL
    When 调用 BodyArchiver.get_message_body("hot-001")
    Then 返回的 body 包含 messages 和 response
    And 存储后端未被访问

  Scenario: 冷数据 — DB 无 body + body_archived=TRUE → 查 Parquet 返回
    Given spend_logs 中有 request_id="cold-001"，messages=NULL，body_archived=TRUE，parquet_path 指向已有文件
    And 该 Parquet 文件包含该 request_id 的完整 body
    When 调用 BodyArchiver.get_message_body("cold-001")
    Then 存储后端被访问以读取 footer 和 col chunk
    And 返回的 body 包含 messages 和 response

  Scenario: 归档中 — DB 无 body + body_archived=FALSE → 返回 None
    Given spend_logs 中有 request_id="pending-001"，messages=NULL，body_archived=FALSE
    When 调用 BodyArchiver.get_message_body("pending-001")
    Then 返回 None
    And 存储后端未被访问

  Scenario: 记录不存在 → 返回 None
    When 调用 BodyArchiver.get_message_body("nonexistent-id")
    Then 返回 None

  # ── Footer 缓存 ──

  Scenario: 首次查询 → footer 缓存未命中 → 读取并缓存
    Given Footer 缓存为空
    And Parquet 文件 "test.parquet" 已存在
    When 第一次查询该文件中的 request_id
    Then 从存储后端读取了 footer（1 次 get_range）
    And Footer 已缓存到 moka Cache 中
    And query_parquet_with_cache 共发起了 2 次存储请求（footer + col chunk）

  Scenario: 缓存命中 → 跳过 footer 请求
    Given Footer 缓存中已有 "test.parquet" 的 ParquetMetaData
    When 第二次查询同一文件中的另一个 request_id
    Then 没有发起 footer 请求（0 次）
    And query_parquet_with_cache 只发起 1 次存储请求（仅 col chunk）

  Scenario: Footer 缓存 TTL 过期 → 重新下载
    Given Footer 缓存中 "test.parquet" 的 entry 已过期（TTL 超时）
    When 查询该文件中的 request_id
    Then 从存储后端重新读取 footer（缓存 miss）
    And 新的 ParquetMetaData 重新缓存

  # ── Row Group 定位 ──

  Scenario: 通过 column statistics 定位 request_id 所在 row group
    Given Parquet 文件有 3 个 row group
    And request_id "zzz-last" 在 row group 2 的 min/max 范围内
    When 执行 locate_row_group_by_request_id
    Then 跳过 row group 0 和 1（statistics min/max 不匹配）
    And 返回 row group 索引 2

  Scenario: 所有 row group 都不匹配 → 返回 RequestNotFound 错误
    Given Parquet 文件有 2 个 row group
    And request_id "not-in-file" 不在任何 row group 的 statistics 范围内
    When 执行 locate_row_group_by_request_id
    Then 返回 ArchiveError::RequestNotFound

  # ── 详情端点集成 ──

  Scenario: GET /global/spend/logs/{request_id} — 冷数据自动回源
    Given spend_logs 中有 request_id="detail-cold-001"，body_archived=TRUE，DB 中 body 为 NULL
    And 对应 Parquet 文件包含完整 body
    When 使用 master-key 发送 GET /global/spend/logs/detail-cold-001
    Then 响应状态码为 200
    And 响应 body 包含 messages 和 response 字段（从 Parquet 回源）

  Scenario: 存储后端不可达 → 详情端点返回 error 但不 crash
    Given spend_logs 中有 request_id="detail-err-001"，body_archived=TRUE，DB 中 body 为 NULL
    And 存储后端不可达
    When 使用 master-key 发送 GET /global/spend/logs/detail-err-001
    Then 响应状态码为 500
    And 服务正常运行不崩溃

  Scenario: 详情端点无认证 → 401
    When 发送 GET /global/spend/logs/some-id 请求（无认证）
    Then 响应状态码为 401

  Scenario: 不存在 request_id → 404
    Given 数据库中无 request_id="nonexistent"
    When 使用 master-key 发送 GET /global/spend/logs/nonexistent
    Then 响应状态码为 404
