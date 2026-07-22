Feature: Key Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Keys page

  Scenario: View key list
    Then I should see 3 keys in the list
    And each key should show its alias, models, and max budget

  Scenario: Create a new key
    When I click the "New Key" button
    And I fill in the key creation form
    And I submit the key creation form
    Then a new key should appear in the list

  Scenario: Search keys by alias
    When I type "prod" into the search box
    Then only keys matching "prod" should be shown

  Scenario: Key token is shown after creation
    When I click the "New Key" button
    And I fill in the key creation form
    And I submit the key creation form
    Then I should see the generated API key token

  Scenario: Delete a key
    When I click the delete button for the first key
    And I confirm the deletion
    Then the key should be removed from the list

  Scenario: Copy key token to clipboard
    When I click the copy button for the first key's token
    Then I should see a "Copied to clipboard" notification

  Scenario: Copy key token via fallback when Clipboard API is unavailable
    Given Clipboard API is unavailable
    When I click the copy button for the first key's token
    Then I should see a "Copied to clipboard" notification

  Scenario: Copy failure shows error toast when all copy methods fail
    Given all copy methods are unavailable
    When I click the copy button for the first key's token
    Then I should see a "Copy failed" error notification
