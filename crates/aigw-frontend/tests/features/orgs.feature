Feature: Organization Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin

  Scenario: View org list
    When I visit the Orgs page
    Then I should see an org named "Engineering" in the list
