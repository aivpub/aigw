Feature: Proxy Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Proxies page

  Scenario: View proxy list
    Then I should see 3 proxies in the list
    And each proxy should show its exit IP

  Scenario: Create a new proxy via dialog
    When I click "New Proxy" on the Proxies page
    And I fill proxy name with "my-proxy"
    And I fill proxy URL with "http://user:pass@1.2.3.4:8080"
    And I click the "Save" button in the proxy dialog
    Then the proxy dialog closes
    And the new proxy "my-proxy" appears in the list

  Scenario: Edit a proxy via dialog
    When I click the edit button on the first proxy row
    Then the proxy dialog opens with pre-filled data

  Scenario: Delete proxy shows confirmation and removes from list
    When I click the delete button on the first proxy row
    Then a proxy delete confirmation dialog appears

  Scenario: Test proxy writes exit probe snapshot
    When I click the Test button on the first proxy row
    Then each proxy should show its exit IP

  Scenario: Quality check shows score/grade/items dialog
    When I click the Quality button on the first proxy row
    Then I should see the quality check dialog with score and grade
    And I should see the quality items breakdown
