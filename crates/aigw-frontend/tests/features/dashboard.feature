Feature: Usage Overview

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Usage page

  Scenario: View spend overview cards
    Then I should see total spend information
    And I should see spend by model chart or data

  Scenario: Loading state shows skeleton
    Given API endpoints are slow to respond
    When I visit the Usage page
    Then I should see loading indicators before data appears

  # ━━━━ Stage 71: Usage page charts & rankings ━━━━

  Scenario: Daily Trend tokens shows prompt and completion stacked bars
    When I click the "📊 Tokens" tab in the Daily Trend card
    Then the Daily Trend chart should show stacked bars for prompt and completion tokens
    And the chart legend should show "Input" and "Output"

  Scenario: Daily Trend requests shows success and failed stacked bars
    When I click the "📋 Requests" tab in the Daily Trend card
    Then the Daily Trend chart should show stacked bars for successful and failed requests
    And the chart legend should show "Success" and "Failed"

  Scenario: Top Virtual Keys ranking list displayed with spend
    Then the Top Virtual Keys card should show a ranked list of keys
    And the ranking should be sorted by spend in descending order
    And the first ranked key should show "#1" and its spend

  Scenario: Top Virtual Keys tab switches to tokens view
    When I click the "📊 Tokens" tab in the Top Virtual Keys card
    Then the ranking values should switch from spend to token values

  Scenario: Top Virtual Keys tab switches to requests view
    When I click the "📋 Requests" tab in the Top Virtual Keys card
    Then the ranking values should switch from spend to request counts

  Scenario: Spend by Model ranking shows sorted model list
    When I click the ranking toggle in the Spend by Model card
    Then the model ranking list should be displayed with progress bars
    And models should be sorted by spend in descending order

  Scenario: Date presets update chart data
    When I click the "7 days" preset button
    Then the activity query should include a 7-day date range

  # ━━━━ Token card compact formatting ━━━━

  Scenario: Token card shows B-tier compact value with exact comma-separated tooltip
    Then the Tokens card should show "1.5B" with exact value "1,500,000,000" on hover

  Scenario: Today preset local-day range with offset
    When I capture activity requests and click the "Today" preset button
    Then the captured activity query should use today's local date with offset_minutes
