Feature: Web Search Providers

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Web Search page

  Scenario: View provider list with pricing and default badge
    Then I should see 2 search providers in the list
    And the default provider should be badged

  Scenario: Expand a provider to see its instances
    When I expand the first search provider
    Then I should see the instance "searx-hk"

  Scenario: Create a provider via dialog
    When I click "New Provider" on the Web Search page
    And I fill search provider name with "searxng-eu"
    And I fill search cost with "0.0002"
    And I click the "Save" button in the provider dialog
    Then the provider dialog closes
    And the new provider "searxng-eu" appears in the list

  Scenario: A placeholder list price raises a warning in the dialog
    When I click "New Provider" on the Web Search page
    And I fill search cost with "0.01"
    Then a cost placeholder warning is shown

  Scenario: Edit a provider via dialog
    When I click the edit button on the first search provider row
    Then the provider dialog opens with pre-filled pricing

  Scenario: Delete the default provider is refused
    When I click the delete button on the default search provider row
    Then a refusal is reported

  Scenario: Probe an instance shows latency and result count
    When I expand the first search provider
    And I click the probe button on the "searx-hk" instance
    Then the probe dialog shows a successful result

  Scenario: Probe an unparseable instance is flagged
    When I expand the first search provider
    And I click the probe button on the "searx-sg" instance
    Then the probe dialog flags an unparseable response
