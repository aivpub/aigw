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

  # ── Stage 104: image attachments ──

  Scenario: Upload a single image shows preview thumbnail
    When I upload an image to the playground
    Then I should see 1 image preview

  Scenario: Upload multiple images shows preview thumbnails
    When I upload 2 images to the playground
    Then I should see 2 image previews

  Scenario: Paste an image from clipboard shows preview thumbnail
    When I paste an image into the playground
    Then I should see 1 image preview

  Scenario: Preview thumbnails render the data URL image
    When I upload an image to the playground
    Then the preview thumbnail should have a data:image src

  Scenario: Remove an attachment clears its preview
    When I upload 2 images to the playground
    And I remove the first image attachment
    Then I should see 1 image preview

  Scenario: Send with image to chat endpoint carries image_url content parts
    When I select model "gpt-4" from the settings panel
    And I upload an image to the playground
    And I type "describe this" into the chat input
    And I click the Send button
    Then the chat request body should include an image_url content part

  Scenario: Send with image to messages endpoint carries Claude image block
    When I select model "gpt-4" from the settings panel
    And I switch the endpoint type to Claude Messages
    And I upload an image to the playground
    And I type "describe this" into the chat input
    And I click the Send button
    Then the messages request body should include a Claude image block

  Scenario: Image attachment survives reload and is cleared by New Chat
    When I upload an image to the playground
    And I reload the playground page
    Then I should see 1 image preview
    When I click the New Chat button
    Then the chat messages should be cleared

  # ── Stage 105: image bubble rendering ──

  Scenario: User message renders the sent image attachment as a thumbnail
    When I select model "gpt-4" from the settings panel
    And I upload an image to the playground
    And I type "describe this" into the chat input
    And I click the Send button
    Then the user message should render an image thumbnail

  # ── Stage 114 TD-009a/b: image compression + body-limit defense ──

  Scenario: Large photo upload is downscaled (compressed data URL is smaller)
    When I upload a large photo to the playground
    Then the pending image should be compressed to a smaller data URL

  Scenario: Small image upload is NOT compressed (keeps original)
    When I upload an image to the playground
    Then the pending image should be the original tiny PNG unchanged

  Scenario: Oversized attachment is rejected with a toast and no request is sent
    When I select model "gpt-4" from the settings panel
    And I upload an oversized image to the playground
    And I type "describe this" into the chat input
    And I click the Send button
    Then I should see the attachment-too-large toast
    And no chat request should have been sent

