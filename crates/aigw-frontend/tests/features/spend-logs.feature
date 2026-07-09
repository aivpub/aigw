Feature: Spend Logs

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page

  Scenario: View spend logs list
    Then I should see the spend logs table or card list
    And I should see spend log entries with model names and costs

  Scenario: Filter by date range
    When I change the start date to "2026-07-01"
    And I change the end date to "2026-07-08"
    Then the spend logs list should update

  Scenario: Filter by model name
    When I type "gpt-4" into the model filter
    Then the spend logs list should update

  Scenario: Mobile spend logs uses card layout
    Given the viewport is mobile size 375x667
    When I visit "/dash/spend-logs"
    Then the spend log data should be displayed in a mobile-friendly format

  Scenario: Loading state shows skeleton
    Given API endpoints are slow to respond
    When I visit the Spend Logs page
    Then I should see loading indicators before spend data appears
