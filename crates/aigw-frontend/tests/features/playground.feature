Feature: Playground Chat

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Playground page

  Scenario: Page loads with chat layout
    Then I should see the message input area
    And I should see a send button

  Scenario: Empty state shows start conversation prompt
    Then the chat area should show "Start a conversation"

  Scenario: Select a model from settings
    When I select model "gpt-4" from the settings panel
    Then the model "gpt-4" should be shown as the active model

  Scenario: Send a streaming message and see response render
    When I select model "gpt-4" from the settings panel
    And I toggle streaming on
    And I type "Hello" into the chat input
    And I click the Send button
    Then I should see a chat response message

  Scenario: Send a non-streaming message and see response render
    When I select model "gpt-4" from the settings panel
    And I type "Hello" into the chat input
    And I click the Send button
    Then I should see a chat response message

  Scenario: New Chat clears conversation
    When I select model "gpt-4" from the settings panel
    And I type "Hello" into the chat input
    And I click the Send button
    And I click the New Chat button
    Then the chat messages should be cleared
