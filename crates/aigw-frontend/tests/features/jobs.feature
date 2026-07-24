Feature: Jobs 管理页面

  # ━━━━ Stage 81: 前端 Jobs 管理页面 ━━━━
  # Mock API，3 viewports

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Settings page
    And I click the "Jobs" tab

  # ── 页面框架 ──

  Scenario: Jobs 页面按 step_type 分 Sub-Tab，默认选中 Body Archive
    Then I should see Sub-Tabs: "Body Archive", "Budget Reset"
    And the "Body Archive" tab is selected by default
    And Sub-Tab list is loaded from GET /admin/jobs step_type dedup

  # ── Body Archive — 统计卡片 ──

  Scenario: 统计卡片展示归档信息和引擎统计
    When I click the "Body Archive" Sub-Tab
    Then I should see a stats card with "● Enabled" indicator
    And I should see "Last Archive: 2026-07-24 17:00"
    And I should see total archived rows formatted as "450K rows"
    And I should see total archived bytes formatted as "75 GB"
    And I should see DB space freed formatted as "120 GB"
    And I should see pending rows count "800 rows pending"
    And I should see Engine stats: "6 loops (3 replicas × 2)"
    And I should see Queue stats: "3 pending · 2 running · 0 stale"
    And I should see Today stats: "48 completed · 1 failed"

  Scenario: 统计卡片数据每 30 秒自动刷新
    Given the archive stats will change on next fetch
    When I wait 30 seconds
    Then the stats card numbers update to reflect new data

  # ── Body Archive — 手动触发 ──

  Scenario: 手动触发存量归档
    When I fill Start Date with "2026-07-22"
    And I fill End Date with "2026-07-24"
    And I click "Estimate"
    Then I should see estimated 48 steps
    When I click "Trigger Archive"
    Then POST /admin/jobs/trigger is called with step_type="body_archive"
    And a success notification appears with the job_id
    And the Job Detail panel opens for the new job

  Scenario: 非 admin 用户看不到手动触发
    Given I am logged in as a non-admin user
    When I am on the Jobs page
    Then the Manual Trigger card is not visible

  # ── Job 历史（通用组件） ──

  Scenario: Job 列表展示所有历史任务
    When I click the "Body Archive" Sub-Tab
    Then I should see a Job History list
    And each row shows Job ID (truncated), trigger_type, status, progress, and created_at
    And status "running" is shown with a blue animated indicator
    And status "completed" is shown with a green checkmark
    And status "failed" is shown with a red icon
    And jobs are ordered by created_at descending

  Scenario: 按 status 过滤 Job 列表
    Given there are 5 jobs: 2 running, 2 completed, 1 failed
    When I select status filter "running"
    Then only 2 jobs are shown, both with status "running"

  # ── Job 详情（通用组件） ──

  Scenario: 点击 Job 行展开详情面板
    When I click on a job row with status "running" in the Job History list
    Then a Job Detail panel appears below the list
    And the Summary section shows total_steps, completed_steps, failed_steps
    And the Steps table shows each step with step_key, status, payload, result
    And completed steps show ✅ icon
    And running steps show 🔄 icon
    And pending steps show ⏳ icon
    And failed steps show ❌ icon

  Scenario: running Job 每 10 秒自动刷新详情
    Given the expanded Job Detail has status "running"
    When I wait 10 seconds
    Then GET /admin/jobs/{job_id} is called again
    And the step progress updates

  Scenario: completed Job 不自动刷新
    Given the expanded Job Detail has status "completed"
    When I wait 10 seconds
    Then GET /admin/jobs/{job_id} is NOT called again

  Scenario: Logs 按 level 过滤
    Given the Job Detail panel is open
    And the Logs section shows the latest 50 log entries
    When I select level filter "error"
    Then only log entries with level "error" are shown
    And entries with level "warn" and "info" are hidden

  # ── result 格式化 ──

  Scenario: completed step 的 result 字段按 body_archive 格式化
    Given the Job Detail panel is open with completed body_archive steps
    Then size_bytes is formatted as "35 MB"
    And duration_ms is formatted as "3.1s"
    And rows_exported is shown as "200"
    And storage_path is shown with truncated path

  # ── 其他 step_type Sub-Tab ──

  Scenario: Budget Reset Sub-Tab 显示占位和统计
    When I click the "Budget Reset" Sub-Tab
    Then I should see a placeholder message "No jobs yet"
    And GET /admin/jobs?step_type=budget_reset is called
    And the stats card shows loop and queue stats for budget_reset

  Scenario: 通用 JobDetail 可用于任何 step_type
    Given the Budget Reset Sub-Tab has 1 job with steps
    When I expand the Job Detail
    Then Steps table shows step_key, status, payload, result
    And result fields without special formatting are shown as raw JSON

  # ── 移动端 ──

  Scenario: Mobile — Sub-Tabs 水平可滚动
    Given viewport is mobile (375x812)
    When I visit the Jobs page
    Then Sub-Tabs are horizontally scrollable
    And the "Body Archive" tab is still visible

  Scenario: Mobile — Steps 表格横向可滚动
    Given viewport is mobile (375x812)
    And the Job Detail panel is open
    Then the Steps table is horizontally scrollable
