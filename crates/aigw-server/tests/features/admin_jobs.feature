Feature: Admin Jobs API

  # ━━━━ Stage 80: Admin API + Col Chunk Cache + 存量归档 ━━━━
  # 需要真实 SQLite DB + 已注册的 BodyArchiver

  Background:
    Given async_jobs / async_job_steps / async_job_logs 三张表已创建
    And Engine 已注册 AsyncTask "body_archive" 和 "test_task"
    And 使用 master-key 认证

  # ── POST /admin/jobs/trigger ──

  @skip
  Scenario: 手动触发 → 创建 Job 并返回 job_id
    Given AsyncTask "body_archive" 支持 steps_from_payload
    When 发送 POST /admin/jobs/trigger
      """
      {"step_type": "body_archive", "payload": {"start_date": "2026-07-22T00:00:00+08:00", "end_date": "2026-07-23T00:00:00+08:00"}}
      """
    Then 响应状态码为 201
    And 响应 body 包含 job_id、status="accepted"、total_steps=24
    And async_jobs 表中有 1 条新记录，trigger_type="manual"
    And async_job_steps 表中有 24 条新记录，status 均为 pending

  Scenario: 手动触发需要 admin 认证
    Given 使用普通用户 token（非 admin）
    When 发送 POST /admin/jobs/trigger
      """
      {"step_type": "body_archive", "payload": {}}
      """
    Then 响应状态码为 401

  Scenario: 触发未知 step_type → 404
    When 发送 POST /admin/jobs/trigger
      """
      {"step_type": "nonexistent", "payload": {}}
      """
    Then 响应状态码为 404

  @skip
  Scenario: 触发不支持手动触发的 AsyncTask → 400
    Given AsyncTask "test_task" 未 override steps_from_payload
    When 发送 POST /admin/jobs/trigger
      """
      {"step_type": "test_handler", "payload": {}}
      """
    Then 响应状态码为 400
    And 错误消息包含 "manual trigger not supported"

  # ── GET /admin/jobs ──

  Scenario: 列出所有 Job
    Given async_jobs 中有 3 条记录（2 条 body_archive + 1 条 test_handler）
    When 发送 GET /admin/jobs
    Then 响应 status_code 为 200
    And 响应 body 中 jobs 数组包含 3 条记录
    And total = 3

  Scenario: 按 step_type 过滤
    When 发送 GET /admin/jobs?step_type=body_archive
    Then 响应 jobs 数组中每条记录的 step_type 均为 "body_archive"

  Scenario: 按 status 过滤
    When 发送 GET /admin/jobs?status=running
    Then 响应 jobs 数组中每条记录的 status 均为 "running"

  Scenario: 需要 admin 认证
    Given 使用普通用户 token
    When 发送 GET /admin/jobs
    Then 响应状态码为 401

  # ── GET /admin/jobs/{id} ──

  Scenario: 查看 Job 详情 — 含 Steps 和 result
    Given async_jobs 中有一条 Job，包含 2 个 Steps（1 completed + 1 pending）
    When 发送 GET /admin/jobs/{job_id}
    Then 响应包含 step_type、status、total_steps、completed_steps、failed_steps
    And steps 数组包含 2 个 Step
    And completed Step 的 status 为 "completed"，result 不为 null
    And pending Step 的 status 为 "pending"，result 为 null
    And summary 中包含 total_rows_exported（聚合自 result）

  Scenario: Job 不存在 → 404
    When 发送 GET /admin/jobs/nonexistent-id
    Then 响应状态码为 404

  # ── GET /admin/jobs/{id}/logs ──

  Scenario: 查看 Job 执行日志
    Given async_job_logs 中有 5 条日志（3 info + 1 warn + 1 error），job_id 匹配
    When 发送 GET /admin/jobs/{job_id}/logs
    Then 响应包含 5 条日志
    And 每条日志包含 level、message、created_at

  Scenario: 按 level 过滤日志
    Given async_job_logs 中有 5 条日志
    When 发送 GET /admin/jobs/{job_id}/logs?level=error
    Then 只返回 level="error" 的日志

  Scenario: 日志分页
    Given async_job_logs 中有 50 条日志
    When 发送 GET /admin/jobs/{job_id}/logs?limit=20&offset=0
    Then 返回 20 条日志
    When 发送 GET /admin/jobs/{job_id}/logs?limit=20&offset=20
    Then 返回下一批 20 条日志

  # ── GET /admin/jobs/stats ──

  @skip
  Scenario: 查看引擎统计 — 所有 step_type 的 loop 数和 queue depth
    Given Engine 运行中，body_archive 有 2 个 exec loop，test_handler 有 1 个
    And async_job_steps 中 body_archive 有 3 pending + 2 running + 1 stale
    When 发送 GET /admin/jobs/stats
    Then 响应包含 step_types 对象
    And body_archive.loops.allocated = 2
    And body_archive.queue.pending = 3
    And body_archive.queue.running = 2
    And body_archive.queue.stale = 1

  Scenario: Stats 需要 admin 认证
    Given 使用普通用户 token
    When 发送 GET /admin/jobs/stats
    Then 响应状态码为 401

  # ── GET /admin/archive/stats ──

  @skip
  Scenario: 查看 archive 全局统计
    Given spend_logs 中有 1000 条 body_archived=TRUE 的记录，总 messages 体积 75GB
    And spend_logs 中有 800 条 body_archived=FALSE 的记录
    When 发送 GET /admin/archive/stats
    Then 响应包含 auto_archive=true
    And total_archived_rows = 1000
    And pending_rows = 800
    And db_body_freed_bytes > 0
    And last_archive_at 不为空

  # ── Col Chunk 缓存 ──

  @skip
  Scenario: 缓存命中 → 跳过存储请求
    Given ColChunkCache 已启用，mode=fs
    And 缓存中已有 key "test.parquet:0:messages" 的数据
    When 查询同一 col chunk 第二次
    Then 不发起存储 get_range 请求

  @skip
  Scenario: 缓存满 → LFU 驱逐最少访问的条目
    Given ColChunkCache max_size_mb=2，已缓存 3 个 chunk 共 1.9MB
    And chunk-1 access_count=10，chunk-2 access_count=3，chunk-3 access_count=1
    When 缓存一个新的 500KB chunk
    Then chunk-3 被驱逐
    And 新 chunk 写入成功

  @skip
  Scenario: 进程重启 → 从 meta.json 恢复
    Given ColChunkCache 上次运行时有 2 个缓存条目
    When 进程重启并调用 cache.restore()
    Then 2 个条目均恢复
    And 所有 entry.access_count 归零

  @skip
  Scenario: mode=none → ColChunkCache 不生效
    Given col_chunk_cache.mode = "none"
    When 查询 Parquet body
    Then 每次 col chunk 都从存储后端读取
