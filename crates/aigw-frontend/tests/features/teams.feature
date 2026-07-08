Feature: Team Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin

  Scenario: View team list
    When I visit the Teams page
    Then I should see a team named "AI Team" in the list
