Feature: MCP Servers

  Background:
    Given API endpoints are mocked
    And I am logged in as admin
    And I am on the MCP page

  Scenario: View the server list
    Then I should see 3 MCP servers in the list
    And the MCP page header has both action buttons
    And the MCP page has no horizontal page scroll

  Scenario: Create a server with a valid alias
    When I click "New Server" on the MCP page
    And I fill MCP alias with "notion"
    And I fill MCP display name with "Notion"
    And I fill MCP URL with "https://mcp.notion.example.com/mcp"
    And I click the save button in the MCP server dialog
    Then the MCP server dialog closes
    And I see an MCP success toast
    And the new MCP server "notion" appears in the list

  Scenario: An alias containing a double underscore is rejected inline
    When I click "New Server" on the MCP page
    And I fill MCP alias with "no__tion"
    Then the MCP alias error is shown
    And the MCP save button is disabled
    And no MCP create request was sent
    And the MCP server "no__tion" is not listed

  Scenario: A duplicate alias is rejected inline
    When I click "New Server" on the MCP page
    And I fill MCP alias with "github"
    Then the MCP alias error mentions a duplicate

  Scenario: Filtering by enabled hides disabled servers
    When I select the MCP status filter "enabled"
    Then the MCP server "internal-wiki" is not listed

  Scenario: Probe a server shows protocol version and tool count
    When I click the probe button on the "github" MCP server
    Then the MCP probe dialog shows the protocol version "2026-07-28"
    And the MCP probe dialog shows 3 tools

  Scenario: Probe reports conflicting tools
    When I click the probe button on the "hr-portal" MCP server
    Then the MCP probe dialog shows a conflict block with 2 tools

  Scenario: The credentials dialog never shows token plaintext
    When I click the credentials button on the "github" MCP server
    Then every MCP token value is redacted
    And the token plaintext never appears in the page

  Scenario: Setting the second token as default clears the first
    When I click the credentials button on the "github" MCP server
    And I set the "svc-account-2025" shared token as default
    Then only "svc-account-2025" carries the default badge

  Scenario: Tools dialog lists the probed catalog
    When I click the tools button on the "github" MCP server
    Then the MCP tools dialog lists 3 tools

  Scenario: Disabling then deleting a server removes the row
    When I click the toggle button on the "hr-portal" MCP server
    Then the MCP server "hr-portal" is shown as disabled
    When I click the delete button on the "hr-portal" MCP server
    And I confirm the MCP deletion
    Then I see an MCP success toast
    And the MCP server "hr-portal" is not listed

  # ── Stage 145: my credentials, call log, ACL ──

  Scenario: Admin sees both the server management and my credentials tabs
    Then the MCP tab "servers" is visible
    And the MCP tab "mine" is visible

  Scenario: The credentials dialog form resets after it is closed
    When I click the credentials button on the "github" MCP server
    And I type a shared token value into the credentials form
    And I close the MCP credentials dialog
    When I click the credentials button on the "github" MCP server
    Then the reopened credentials form carries no typed value
    And the typed shared token value never appears in the page

  Scenario: The call log renders every call and highlights failures
    When I click the call log button on the "github" MCP server
    Then the MCP call log shows 2 calls
    And the failed MCP call row is highlighted
    And the MCP call log alias filter carries "github"

  Scenario: The call log filters by status, tool name and time
    When I open the MCP call log tab
    And I choose the MCP call status "failed"
    Then the MCP call log request carries status "failed"
    When I choose the MCP call status "success"
    And I search MCP calls for tool "get_issue"
    Then the MCP call log request carries tool_name "get_issue"
    When I choose the MCP call time preset "7d"
    Then the MCP call log request carries a time window

  Scenario: An empty call log says so instead of showing a blank table
    When I open the MCP call log tab
    And I search MCP calls for tool "nothing-matches"
    Then the MCP call log shows an empty state

  Scenario: A call row opens its full detail
    When I open the MCP call log tab
    And I open the first MCP call row
    Then the MCP call detail lists the call id and byte counts

  Scenario: A tool without a rule is unrestricted and can be restricted
    When I click the tools button on the "github" MCP server
    Then the MCP tool "search" shows no restriction
    When I click the restrict button on the MCP tool "search"
    And I restrict it to team "team-9" as denied
    Then the MCP tool "search" shows the restriction "team:team-9"

  Scenario: Adding a rule for a second scope keeps the first
    When I click the tools button on the "github" MCP server
    When I click the restrict button on the MCP tool "search"
    And I restrict it to team "team-9" as denied
    Then the MCP tool "search" shows the restriction "team:team-9"
    When I click the restrict button on the MCP tool "search"
    And I restrict it to org "org-1" as allowed
    Then the MCP tool "search" shows both restrictions "team:team-9" and "org:org-1"

  Scenario: Deletion asks for confirmation before removing the server
    When I click the delete button on the "hr-portal" MCP server
    Then the MCP delete confirmation is shown
    And the MCP server "hr-portal" is still listed
    When I confirm the MCP deletion
    Then the MCP server "hr-portal" is not listed

  Scenario: A custom auth header is sent with the credential and its value is never echoed
    When I click "New Server" on the MCP page
    And I fill MCP alias with "tapd"
    And I fill MCP display name with "TAPD"
    And I fill MCP URL with "https://mcpgw.example.com/tapd/"
    And I choose the custom auth header in the MCP server dialog
    And I fill MCP auth header name with "X-Tapd-Access-Token"
    And I fill MCP auth header value with "tapd-secret"
    And I add an MCP extra header "X-Project-Id" with value "12345"
    And I click the save button in the MCP server dialog
    Then the MCP server dialog closes
    And the last MCP server request names the auth header "X-Tapd-Access-Token"
    And the last MCP server request carries the extra header "X-Project-Id"
    And the typed MCP auth value never appears in the page

  Scenario: A header the gateway writes itself is rejected inline
    When I click "New Server" on the MCP page
    And I fill MCP alias with "reserved-header"
    And I fill MCP display name with "Reserved"
    And I fill MCP URL with "https://mcp.example.com/mcp"
    And I choose the custom auth header in the MCP server dialog
    And I fill MCP auth header name with "Mcp-Session-Id"
    Then the MCP auth name error mentions a reserved header
    And no MCP create request was sent

  Scenario: Editing an existing server keeps the stored secret and never sends the mask back
    When I click the edit button on the "internal-wiki" MCP server
    Then the MCP auth value is shown masked
    And the MCP auth value input is empty
    When I click the save button in the MCP server dialog
    Then the last MCP server request does not carry a masked auth value

  # ── Stage 147: the call log is a page, not a dialog ──

  Scenario: The admin call log shows the user column
    When I open the MCP call log tab
    Then the MCP call log shows 4 calls
    And the MCP call log shows the user column
    And the unbound-user call row shows "Not bound" in the user column
    And the MCP page has no horizontal page scroll

  Scenario: The call log's three tabs fit a phone
    When I open the MCP call log tab
    And I choose the MCP call time preset "custom"
    Then the MCP page has no horizontal page scroll

  Scenario: The call log tabs and filters fit a 320px screen
    When the MCP page is viewed on a 320px wide screen
    And I open the MCP call log tab
    Then the MCP tabs and call log filters fit inside the screen

  # ── Stage 148: re-probe all shows progress instead of a frozen button ──

  Scenario: Re-probing every server shows the sweep advancing
    When I click re-probe all on the MCP page
    Then the MCP progress dialog appears
    And the MCP progress bar reports it is not yet complete
    And the MCP progress dialog shows 3 servers
    And the MCP progress dialog fits inside the screen

  Scenario: The progress dialog fits a 320px screen
    When the MCP page is viewed on a 320px wide screen
    And I click re-probe all on the MCP page
    Then the MCP progress dialog fits inside the screen

  Scenario: One failing server is red while the others finish
    When I click re-probe all on the MCP page
    Then the MCP progress dialog shows 3 servers
    And the MCP progress row for "hr-portal" is marked "failed"
    And the MCP progress row for "hr-portal" carries an error summary
    And the MCP progress row for "github" is marked "ok"
    And the MCP progress dialog finishes with a partial summary

  Scenario: Clicking again while a sweep runs attaches to it
    When I click re-probe all on the MCP page
    And the MCP progress dialog appears
    And I close the MCP progress dialog
    And I click re-probe all again
    Then the MCP page shows the attach-to-running toast
    And no MCP error toast is shown

  Scenario: A 409 from another operator's start attaches instead of erroring
    When I click re-probe all on the MCP page
    And the MCP progress dialog appears
    And I close the MCP progress dialog
    And the latest reload run is hidden from this tab
    And I click re-probe all again
    Then the MCP page shows the attach-to-running toast
    And no MCP error toast is shown

  Scenario: Closing and reopening the dialog resumes the same run
    When I click re-probe all on the MCP page
    And the MCP progress bar reports it is not yet complete
    And I close the MCP progress dialog
    And I click re-probe all again
    Then the MCP progress bar reports it is not yet complete
    And the MCP progress dialog shows 3 servers

  Scenario: A single probe disables its own button while it runs
    When I start a probe on the "github" MCP server
    Then the probe button on the "github" MCP server is disabled
