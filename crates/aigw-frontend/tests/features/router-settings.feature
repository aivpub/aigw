Feature: Router Settings

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Router Settings page

  Scenario: Usage-based and latency-based strategies are selectable (Stage 118)
    When I open the routing strategy dropdown
    Then I should see "Usage-Based (least busy)" as an enabled option
    And I should see "Latency-Based (lowest EWMA)" as an enabled option
    And I should see "Simple Shuffle (random)" as an enabled option

  Scenario: Select and save a latency-based strategy
    When I open the routing strategy dropdown
    And I select "Latency-Based (lowest EWMA)" as the routing strategy
    And I click the save button
    Then a success toast should appear for the global router settings
