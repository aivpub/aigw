Feature: Playground Chat

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Playground page

  Scenario: Page loads with all controls
    Then I should see the model selector dropdown
    And I should see the system prompt textarea
    And I should see the user message textarea
    And I should see the send button
    And I should see the streaming toggle

  Scenario: Empty state shows placeholder text
    Then the response area should show "Enter a message and click Send to test"

  Scenario: Select a model from dropdown
    When I select model "gpt-4" from the dropdown
    Then the model "gpt-4" should be selected

  Scenario: Send a message and see response
    When I type "Hello, how are you?" into the user message
    And I click the Send button
    Then I should see a response in the response area

  Scenario: Send with system prompt
    When I type "You are a helpful assistant" into the system prompt
    And I type "What is 2+2?" into the user message
    And I click the Send button
    Then I should see a response in the response area

  Scenario: Stream response toggle
    When I toggle streaming on
    And I type "Hello" into the user message
    And I click the Send button
    Then I should see a response in the response area

  Scenario: Mobile playground stacks layout
    Given the viewport is mobile size 375x667
    When I visit "/dash/playground"
    Then the playground should be displayed in a mobile-friendly format
