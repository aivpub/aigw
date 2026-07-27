Feature: AsyncTask 引擎框架

  # ━━━━ Stage 78: Engine claim + AsyncTask 调度/执行 ━━━━
  # 使用真实 SQLite（SKIP LOCKED 需要真实 DB）

  Background:
    Given async_jobs / async_job_steps / async_job_logs 三张表已创建
    And Engine 已注册一个 mock AsyncTask "test_task" (concurrency=1, tick_interval=60s)

  # ── claim 原子性 ──

  Scenario: claim_next_step 原子领取一个 pending step
    Given async_job_steps 中有 3 个 pending step，step_type = "test_task"
    When Engine exec loop 调用 claim_next_step("test_task")
    Then 返回 1 个 step，且 step.status 更新为 running
    And async_job_steps 中仍有 2 个 pending step

  Scenario: 全部 running 时 claim_next_step 返回 None
    Given async_job_steps 中有 2 个 step，状态均为 running，step_type = "test_task"
    When Engine exec loop 调用 claim_next_step("test_task")
    Then 返回 None

  Scenario: 不同 step_type 的 loop 隔离
    Given async_job_steps 中有 2 个 step_type="test_task" 的 pending step
    And async_job_steps 中有 3 个 step_type="body_archive" 的 pending step
    When exec loop A 调用 claim_next_step("test_task")
    And exec loop B 调用 claim_next_step("body_archive")
    Then loop A 拿到 step_type="test_task" 的 step
    And loop B 拿到 step_type="body_archive" 的 step

  Scenario: 多副本并发 — SKIP LOCKED 保证不重复
    Given async_job_steps 中有 5 个 pending step，step_type = "test_task"
    When 3 个 exec loop 同时调用 claim_next_step("test_task")
    Then 3 个 exec loop 分别拿到不同的 step
    And 剩余 2 个 step 仍为 pending

  Scenario: tick 调度去重 — UNIQUE(job_id, step_key) 防重复
    Given async_jobs 中已有 job "cron-test_task-2026072417"
    And async_job_steps 中已有该 job 的 step_key="hour=2026-07-24T14"
    When 两个 tick loop 同时 INSERT 相同 step_key 和 job_id
    Then 只有 1 条 INSERT 成功
    And 另 1 条因 UNIQUE 约束静默失败

  # ── tick ──

  Scenario: tick 有发现 → Engine INSERT Job + Steps
    Given mock AsyncTask tick 返回 3 个 NewStep
    When Engine tick loop 调用 task.tick()
    Then async_jobs 表中新增 1 条记录，trigger_type="cron"
    And async_job_steps 表中新增 3 条记录，status 均为 pending

  Scenario: tick 无发现 → 跳过本轮
    Given mock AsyncTask tick 返回 None
    When Engine tick loop 调用 task.tick()
    Then async_jobs 和 async_job_steps 表均无新增

  # ── complete ──

  Scenario: 最后一个 step 完成 → job completed + finalize
    Given async_jobs 中有一个 job，total_steps=3，completed_steps=2，failed_steps=0
    When exec loop 完成第 3 个 step
    Then async_jobs.completed_steps 更新为 3
    And async_jobs.status 更新为 "completed"
    And AsyncTask.finalize() 被调用 1 次

  Scenario: 部分 failed 的 job 也能正常完成
    Given async_jobs 中有一个 job，total_steps=3，completed_steps=1，failed_steps=1
    When exec loop 完成第 3 个 step
    Then completed_steps + failed_steps == total_steps
    And async_jobs.status 更新为 "partially_failed"

  # ── fail + retry ──

  Scenario: Step 失败且未超过 max_retries → reset 为 pending
    Given async_jobs 中 max_retries=3
    And async_job_steps 中有一个 retry_count=0 的 step
    When exec loop 执行该 step 失败
    Then step.retry_count 更新为 1
    And step.status 重置为 "pending"

  Scenario: Step 连续失败超过 max_retries → failed
    Given async_jobs 中 max_retries=3
    And async_job_steps 中有一个 retry_count=3 的 step
    When exec loop 执行该 step 再次失败
    Then step.status 更新为 "failed"
    And async_jobs.failed_steps 递增 1

  # ── cleanup ──

  Scenario: Cleanup 回收超时 running step
    Given async_job_steps 中有 1 个 step，status=running，started_at = 20 分钟前
    And Engine step_timeout = 10min
    When Engine cleanup loop 执行 cleanup_stale_steps
    Then 该 step.status 重置为 "pending"

  Scenario: Cleanup 不回收未超时的 running step
    Given async_job_steps 中有 1 个 step，status=running，started_at = 2 分钟前
    When Engine cleanup loop 执行 cleanup_stale_steps
    Then 该 step 仍保持 running 状态不变

  # ── 并发控制 ──

  Scenario: max_loops 全局上限生效
    Given Engine 配置 max_loops=4
    And 注册了 3 个 AsyncTask: body_archive(concurrency=2), budget_reset(concurrency=4), session_cleanup(concurrency=2)
    When Engine 分配 exec loop
    Then 每个 AsyncTask 至少 1 个 loop
    And 总 loop 数 ≤ 4

  Scenario: steps_from_payload 默认返回 unsupported error
    Given 注册了一个 AsyncTask，未 override steps_from_payload()
    When 调用 task.steps_from_payload(任意 payload)
    Then 返回错误 "manual trigger not supported"
