Feature: Spend Logs

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page

  Scenario: View spend logs list
    Then I should see the spend logs table or card list
    And I should see spend log entries with model names and costs

  Scenario: Time presets change the date range
    When I click the "24 hours" time preset button
    Then the spend logs list should update
    And I should see a table with multiple columns including Time Type Model and Cost

  Scenario: Live Tail toggle enables auto-refresh
    When I toggle the Live Tail switch on
    Then I should see an auto-refresh banner indicating 15 second refresh

  Scenario: Page size selector changes rows per page
    When I change the page size to 50
    Then the spend logs query should include page_size=50

  Scenario: Call ID search filters logs
    When I type "req-001" into the call ID search
    Then the spend logs list should update

  Scenario: Click row opens detail drawer
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata

  Scenario: Mobile spend logs uses card layout
    Given the viewport is mobile size 375x667
    When I visit "/dash/spend-logs"
    Then the spend log data should be displayed in a mobile-friendly format

  Scenario: Loading state shows skeleton
    Given API endpoints are slow to respond
    When I visit the Spend Logs page
    Then I should see loading indicators before spend data appears

  Scenario: Click row opens detail drawer with body content
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata
    And the detail drawer should show prompt and response content

  Scenario: Detail drawer shows loading skeleton while fetching body
    Given API detail endpoints are slow to respond
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see skeleton loading inside the detail drawer

  Scenario: Detail drawer shows error and retry when fetch fails
    Given API detail endpoints return error
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see an error message inside the detail drawer
    And I should see a retry button inside the detail drawer

  Scenario: Mobile card click also fetches detail body
    Given the viewport is mobile size 375x667
    And API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the first spend log row
    Then I should see a detail drawer with request metadata
    And the detail drawer should show prompt and response content

  Scenario: Call ID is the leftmost column in the table header
    Then the first column header of the spend logs table should be "Call ID"

  Scenario: Detail drawer shows both Call ID and Request ID badges
    When I click on the first spend log row
    Then I should see a "Call ID" badge in the detail drawer
    And I should see a "Request ID" badge in the detail drawer

  Scenario: Fuzzy search by call_id prefix filters logs
    When I type "req-00" into the call ID search
    Then the spend logs list should update

  # ── Stage 105: multimodal body rendering ──

  Scenario: Detail drawer shows image thumbnail for image_url prompt
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-img-001"
    Then I should see an image thumbnail in the detail drawer

  Scenario: Detail drawer renders output_text response content
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-img-001"
    Then the detail drawer should show the output_text text

  Scenario: Detail drawer raw tab preserves original image_url JSON
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-img-001"
    And I switch to the raw tab in the detail drawer
    Then the raw tab should show the image_url JSON

  # ── Stage 108: image_tokens display ──

  Scenario: Detail drawer shows image tokens with upstream source badge
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-img-001"
    Then I should see "Image Tokens" in the detail drawer
    And I should see the "upstream" image token source badge

  Scenario: Spend log list marks multimodal rows with the image emoji
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-img-001" should show the multimodal marker

  # ── Stage 111: embedding response rendering ──

  Scenario: Detail drawer shows embedding response vectors instead of empty state
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-emb-001"
    Then the detail drawer should show embedding vector dimensions
    And the detail drawer should show the prompt_tokens usage

  Scenario: Spend log list type badge shows embedding call type
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-emb-001" should show the "embedding" type badge


  # ── Stage 137: web search call rendering ──
  #
  # Search calls are a new row kind (Stage 136): no tokens, per-query spend,
  # and a `metadata.parent_call_id` pointing at the LLM request that triggered
  # them. These scenarios lock the rendering decisions of design §3.1-§3.6.

  Scenario: Spend log list type badge shows search call type
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-001" should show the "search" type badge

  Scenario: Search row renders em dash instead of zero tokens
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-001" should show no token value

  Scenario: Search row token cell explains why it has no token value
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the token cell of the spend log row with call id "req-search-001" should explain itself

  Scenario: Search row shows provider and spend in the list
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-001" should show the provider and spend

  Scenario: Zero spend search row renders a real zero amount
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-002" should show a zero amount and no tokens

  Scenario: Zero spend search row carries a not-priced affordance
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-002" should show the not priced badge
    And the spend log row with call id "req-search-001" should not show the not priced badge

  Scenario: Search row provider label is not hardcoded
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-003" should show the provider "stubsearch"

  Scenario: Search detail drawer shows query text and result count
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-001"
    Then the detail drawer should show the search query and result count

  Scenario: Search detail drawer shows the instance that served the call
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-001"
    Then the detail drawer should show the search instance "http://searxng-a:9099"

  Scenario: Zero spend drawer explains why the amount is zero
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-002"
    Then the detail drawer should show the search instance "http://searxng-b:9099"
    And the detail drawer should explain the zero search spend

  Scenario: Search detail drawer shows parent call link
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-001"
    Then the detail drawer should show the parent call link

  Scenario: Clicking parent call link switches the drawer to the LLM row
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-001"
    And I click the parent call link in the detail drawer
    Then the detail drawer should show the LLM request "req-001"

  Scenario: LLM detail drawer shows a search badge linking to its search rows
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-001"
    Then the detail drawer should show the search calls button

  Scenario: Clicking the search badge filters the list by parent call
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-001"
    And I click the search calls button in the detail drawer
    Then the spend logs query should include parent_call_id and show only search rows

  Scenario: Call type filter narrows the list to search calls
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I select "search" in the call type filter
    Then the spend logs query should include call_type and show only search rows

  Scenario: Call type filter set to all shows both LLM and search rows
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend logs list should show both LLM and search rows

  Scenario: Search row with empty metadata still renders without crashing
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Then the spend log row with call id "req-search-004" should be visible without errors

  Scenario: Search detail drawer hides cache and TTFT blocks
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    When I click on the spend log row with call id "req-search-001"
    Then the detail drawer should not show cache or ttft

  Scenario: Mobile search card renders em dash tokens and provider
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the Spend Logs page
    Given the viewport is mobile size 375x667
    When I visit "/dash/spend-logs"
    Then the mobile search card should show no tokens and a provider
