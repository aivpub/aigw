Feature: User Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin

  Scenario: View user list
    When I visit the Users page
    Then I should see a user named "Alice" in the list

  Scenario: Create a new user
    When I visit the Users page
    And I click the "New User" button
    And I fill in the user creation form
    And I submit the user creation form
    Then I should see a success toast

  Scenario: Delete a user
    When I visit the Users page
    And I click the delete button for the first user
    And I confirm the deletion in the dialog
    Then I should see a deletion success toast
