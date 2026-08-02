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
    And I should see "Auto Archive"
    And I should see total archived rows formatted as "450K"
    And I should see pending rows count "800"
    And I should see Queue stats: "3 pending · 2 running · 0 stale"

  Scenario: 统计卡片数据每 30 秒自动刷新
    Given the archive stats will change on next fetch
    When I wait 30 seconds
    Then the stats card numbers update to reflect new data

  # ── Body Archive — 手动触发 ──

  Scenario: 手动触发存量归档
    When I click the "Body Archive" Sub-Tab
    And I click "Trigger Archive"
    And I fill Start Date with "2026-07-22"
    And I fill End Date with "2026-07-24"
    And I click "Trigger Job"
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
    Then GET /admin/jobs/job-id is called again
    And the step progress updates

  Scenario: completed Job 不自动刷新
    Given the expanded Job Detail has status "completed"
    When I wait 10 seconds
    Then GET /admin/jobs/job-id is NOT called again

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
    Then I should see a placeholder message "No jobs found"
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

  # ━━━━ Stage 84: 前端 Jobs 页面生产化重构 ━━━━
  # TDD Red 阶段 — 11 个新场景，当前实现失败（验证 Red）

  # ── Q8: 路由直达 ──

  Scenario: 路由直达 — 访问 /dash/jobs/{id} 直达详情页
    Given API mock returns job "job-test-001" with status "running"
    When I navigate to "/dash/jobs/job-test-001"
    Then I should see the Job Detail panel for "job-test-001"
    And I should NOT see the outer Sub-Tab bar

  # ── Q8: 详情页刷新 ──

  Scenario: 详情页刷新后仍显示同一 job
    Given I am viewing Job Detail for "job-test-002"
    When I refresh the page
    Then I should still see the Job Detail panel for "job-test-002"
    And the URL is still "/dash/jobs/job-test-002"

  # ── Q8: 浏览器后退 ──

  Scenario: 浏览器后退从详情回到列表
    Given I am viewing Job Detail for "job-test-003"
    When I press the browser back button
    Then I should see the job list
    And the URL is "/dash/jobs"

  # ── Q6: 列表分页 ──

  Scenario: 列表超过一页显示分页控件并翻页
    Given there are 120 jobs in total
    When I am on the Jobs page with default page=1
    Then I should see pagination controls with Page 1 of 3
    When I click page 2
    Then GET /admin/jobs is called with page=2
    And job list shows the next 50 jobs

  # ── Q4: Tab 标签不含下划线 ──

  Scenario: Tab 标签显示人类可读文本不含下划线
    When I am on the Jobs page
    Then I should see a tab labeled "Body Archive"
    And I should NOT see a tab labeled "body_archive"

  # ── Q4: Storage 未配置时 Trigger 按钮禁用 ──

  Scenario: Storage 未配置时 Trigger 按钮被禁用
    Given archive stats return storage_configured=false
    When I click the "Body Archive" Sub-Tab
    Then the Trigger button is disabled
    And hovering over the button shows a tooltip "Storage not configured"

  # ── Q3: 矛盾检测 — completed + rows_archived=0 → no-op ──

  Scenario: completed step 但 rows_archived=0 显示为灰色 no-op
    Given Job Detail has a step with status="completed" and result.rows_archived=0
    When I view the Steps table
    Then that step status shows as "completed (no-op)" in gray
    And it does NOT show the green checkmark

  # ── Q2: Logs 按 step_key 分组 ──

  Scenario: Logs 表显示 Step Key 列并按 step 折叠
    Given Job Detail has logs with step_keys "hour=..T14" and "hour=..T15"
    When I view the Logs section
    Then Logs table has a "Step Key" column
    And I can expand logs for a specific step_key

  # ── Q5: Manual Trigger 与 Sub-Tab 同行 ──

  Scenario: Manual Trigger 按钮在 Sub-Tab 同一行
    When I am on the Body Archive Sub-Tab
    Then the Trigger button is in the same row as the Sub-Tab bar
    And there is no separate Manual Trigger card

  # ── Q7: 详情页去冗余 ──

  Scenario: 详情页不显示外层 Sub-Tab 栏
    Given I am viewing Job Detail for "job-test-007"
    Then I do NOT see the "Body Archive" and "Budget Reset" Sub-Tab bar
    And the title shows "Body Archive · manual"
    And Steps table shows Payload, Result, and Duration columns
    And Steps table is paginated with pageSize=20

  # ── a11y: 键盘导航 ──

  Scenario: Job 行支持键盘 Enter/Space 触发详情
    Given I am on the job list
    When I focus on a job row and press Enter
    Then the Job Detail panel opens for that job
    When I go back and focus on another job row and press Space
    Then the Job Detail panel opens for that job
