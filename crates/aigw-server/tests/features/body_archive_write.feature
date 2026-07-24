Feature: Body Archive 写链路

  # ━━━━ Stage 78: BodyArchiver impl AsyncTask ━━━━
  # 需要真实 SQLite DB + mock object_store

  Background:
    Given spend_logs 表已包含 body_archived 和 parquet_path 列
    And async_jobs / async_job_steps / async_job_logs 已创建
    And BodyArchiver 已注册到 Engine，存储后端为 mock
    And body_archive.archive_after_hours = 1
    And body_archive.null_body_after_days = 7

  Scenario: enabled=false 时 tick 返回 None
    Given body_archive.enabled = false
    When Engine 调用 BodyArchiver.tick()
    Then 返回 None

  Scenario: 无待归档数据时 tick 返回 None
    Given spend_logs 中最近 2 小时的数据 body_archived 均为 TRUE
    When Engine 调用 BodyArchiver.tick()
    Then 返回 None

  Scenario: 发现未归档小时 → tick 返回 Steps → Engine 创建 Job + Steps
    Given spend_logs 中有 2 小时前的数据，body_archived = FALSE，共 3 个不同小时
    When Engine tick loop 调用 BodyArchiver.tick()
    Then 返回 Some(steps)，steps 数量为 3
    And async_jobs 表中新增 1 条，step_type="body_archive"，trigger_type="cron"
    And async_job_steps 表中新增 3 条，status 均为 pending

  Scenario: Exec loop 执行一个 step — 归档指定小时
    Given async_job_steps 中有一个 pending step，payload = {"hour": "2026-07-24T14:00:00+08:00"}
    And spend_logs 中该小时有 2 条 body_archived=FALSE 的记录
    When Engine exec loop 调用 BodyArchiver.execute(step)
    Then 向存储后端上传了 1 个 Parquet 文件
    And 路径为 "year=2026/month=07/day=24/hour=14/data.parquet"
    And spend_logs 中该 2 条 body_archived 更新为 TRUE
    And step.status 更新为 completed
    And result 包含 {rows_archived: 2, size_bytes: >0, storage_path, duration_ms}

  Scenario: Exec loop 执行 step — 该小时无待归档数据
    Given async_job_steps 中有一个 pending step，payload = {"hour": "2026-07-24T15:00:00+08:00"}
    And spend_logs 中该小时所有记录 body_archived 均为 TRUE
    When Engine exec loop 调用 BodyArchiver.execute(step)
    Then step.status 更新为 completed
    And result.rows_archived = 0
    And 不上传任何文件

  Scenario: 存储后端不可达 → step 失败，retry_count 递增
    Given 存储后端不可达
    And async_job_steps 中有一个 pending step
    When Engine exec loop 调用 BodyArchiver.execute(step)
    Then step 失败，step.status 重置为 pending

  Scenario: finalize 清理超过 null_body_after_days 的 body
    Given spend_logs 中有 8 天前 body_archived=TRUE 的记录，body 不为空
    And spend_logs 中有 3 天前 body_archived=TRUE 的记录，body 不为空
    When Engine 调用 BodyArchiver.finalize(job)
    Then 8 天前记录 body 清空为 NULL
    And 3 天前记录 body 不变

  Scenario: null_body_after_archive=false 时 finalize 不清理
    Given body_archive.null_body_after_archive = false
    And spend_logs 中有 8 天前已归档记录
    When Engine 调用 BodyArchiver.finalize(job)
    Then 记录 body 不变

  Scenario: WHERE body_archived=FALSE 保证业务幂等
    Given spend_logs 中某小时 2 条记录 body_archived 已为 TRUE
    When exec loop 再次执行相同小时 step
    Then WHERE body_archived=FALSE 返回 0 行
    And step 完成，rows_archived = 0

  Scenario: Parquet 写入参数正确
    Given spend_logs 中有 50 条待归档记录
    When BodyArchiver 向存储后端写入 Parquet
    Then 使用 ZSTD 压缩
    And 包含 request_id 和 session_id 的 Bloom filter
    And 文件内按 request_id 升序排列

  Scenario: StorageBackend 解析 — S3
    Given config 中 type = "s3"，含 bucket, region, access_key_id, secret_access_key
    When 反序列化为 StorageBackend
    Then 为 StorageBackend::S3 变体

  Scenario: StorageBackend 解析 — FileSystem
    Given config 中 type = "fs"，path = "/data/aigw/archive"
    When 反序列化为 StorageBackend
    Then 为 StorageBackend::FileSystem 变体
