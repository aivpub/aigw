Feature: Spend Logs

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page

  Scenario: View spend logs list
    Then I should see the spend logs table or card list
    And I should see spend log entries with model names and costs

  Scenario: Time presets change the date range
    When I click the "24 hours" time preset button
    Then the spend logs list should update
    And I should see a table with multiple columns including Time Type Model and Cost

  Scenario: Live Tail toggle enables auto-refresh
    When I toggle the Live Tail switch on
    Then I should see an auto-refresh banner indicating 15 second refresh

  Scenario: Page size selector changes rows per page
    When I change the page size to 50
    Then the spend logs query should include page_size=50

  Scenario: Call ID search filters logs
    When I type "req-001" into the call ID search
    Then the spend logs list should update

  Scenario: Click row opens detail drawer
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata

  Scenario: Mobile spend logs uses card layout
    Given the viewport is mobile size 375x667
    When I visit "/dash/spend-logs"
    Then the spend log data should be displayed in a mobile-friendly format

  Scenario: Loading state shows skeleton
    Given API endpoints are slow to respond
    When I visit the Spend Logs page
    Then I should see loading indicators before spend data appears

  Scenario: Click row opens detail drawer with body content
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata
    And the detail drawer should show prompt and response content

  Scenario: Detail drawer shows loading skeleton while fetching body
    Given API detail endpoints are slow to respond
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see skeleton loading inside the detail drawer

  Scenario: Detail drawer shows error and retry when fetch fails
    Given API detail endpoints return error
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see an error message inside the detail drawer
    And I should see a retry button inside the detail drawer

  Scenario: Mobile card click also fetches detail body
    Given the viewport is mobile size 375x667
    And API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata
    And the detail drawer should show prompt and response content

  Scenario: Call ID is the leftmost column in the table header
    Then the first column header of the spend logs table should be "Call ID"

  Scenario: Detail drawer shows both Call ID and Request ID badges
    When I click on the first spend log row
    Then I should see a "Call ID" badge in the detail drawer
    And I should see a "Request ID" badge in the detail drawer

  Scenario: Fuzzy search by call_id prefix filters logs
    When I type "req-00" into the call ID search
    Then the spend logs list should update
