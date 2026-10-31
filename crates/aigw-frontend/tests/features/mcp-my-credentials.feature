Feature: MCP My Credentials

  Background:
    Given API endpoints are mocked for a non-admin user
    And I am logged in as a non-admin user
    And I am on the MCP page

  Scenario: A non-admin only sees the my credentials tab
    Then the MCP tab "mine" is visible
    And the MCP tab "servers" is not visible
    And the my credentials table lists every visible server

  # ── Stage 150: refusals have no identity to scope by, so only an admin sees them ──

  Scenario: A non-admin never sees the rejection tab
    Then the MCP tab "rejections" is not visible

  Scenario: A non-admin deep link to an admin-only tab lands somewhere they can read
    When I open the URL "/dash/mcp/rejections"
    Then the browser is at "/dash/mcp/mine"
    And the MCP tab "rejections" is not visible

  Scenario: A user_required server without a token is flagged as blocking
    Then the my credentials row "hr-portal" is flagged as blocking
    And the my credentials row "hr-portal" explains the tools are unavailable
    And the my credentials row "hr-portal" offers a set button

  Scenario: A shared server offers no token action
    Then the my credentials row "internal-wiki" shows the token is not needed
    And the my credentials row "internal-wiki" offers no set button

  Scenario: An optional server shows the token action as optional
    Then the my credentials row "github" shows the token is optional

  Scenario: Setting a token normalizes the row and never leaks the plaintext
    When I open the my token dialog for "hr-portal"
    And I type a personal token into the my token form
    And I save the my token form
    Then the my credentials row "hr-portal" is no longer flagged as blocking
    And the my credentials row "hr-portal" shows the token is configured
    And the personal token plaintext never appears on the page
    And the personal token plaintext was sent to the API

  # ── Stage 147: a user can see their own calls ──

  Scenario: A non-admin sees the call log tab and only their own calls
    Then the MCP tab "calls" is visible
    When I open the MCP call log tab
    Then the MCP call log shows 1 calls
    And the MCP call log has no user column

  Scenario: The call log is scoped by key and cannot ask for another identity
    When I open the MCP call log tab
    Then the MCP call log only carries the signed-in user's rows
