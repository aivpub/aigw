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

  Scenario: Create model — upstream model auto-fills from model name
    When I click "Add Model" on the Models page
    And I fill model_name with "my-gpt-4"
    Then the Upstream Model field is automatically set to "my-gpt-4"

  Scenario: Create a new model via dialog
    When I click "Add Model" on the Models page
    And I fill the model form with name "test-model" provider "openai" input price "15" output price "30"
    And I click the "Create Model" button in the dialog
    Then the dialog closes

  Scenario: Edit a model via dialog
    When I click the edit button on the first model row
    Then the model dialog opens with pre-filled data
    And the model name field is disabled

  Scenario: Delete model shows confirmation and removes from list
    When I click the delete button on the first model row
    Then a delete confirmation dialog appears
