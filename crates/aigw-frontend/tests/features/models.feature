Feature: Model Management

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Models page

  Scenario: View model list
    Then I should see 3 models in the list
    And each model should show its model name

  Scenario: View model details
    When I click on the first model row
    Then I should see the model's litellm params and model info

  Scenario: Search models by name
    When I type "gpt" into the search box
    Then only models matching "gpt" should be shown
