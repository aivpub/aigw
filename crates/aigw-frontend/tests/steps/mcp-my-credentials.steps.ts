import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import {
  getLastMcpCallsPath,
  getLastMcpCallsQuery,
  getLastMcpMyTokenPutBody,
  getMcpCreateRequestCount,
  MCP_MY_TOKEN_PLAINTEXT_VALUE,
} from "./api-mocks";

const { When, Then } = createBdd();

function myCredRow(page: Page, alias: string) {
  return page
    .locator('[data-testid="mcp-my-cred-row"]:visible, [data-testid="mcp-my-cred-card"]:visible')
    .filter({ hasText: alias })
    .first();
}

function toolRow(page: Page, tool: string) {
  return page.getByTestId("mcp-tool-row").filter({ hasText: tool }).first();
}

/** The call log swaps a table for cards below `md`; assertions must not care which. */
function callRows(page: Page) {
  return page.locator('[data-testid="mcp-call-row"]:visible, [data-testid="mcp-call-card"]:visible');
}

Then("the MCP tab {string} is visible", async ({ page }, tab: string) => {
  await expect(page.getByTestId(`mcp-tab-${tab}`).first()).toBeVisible({ timeout: 8000 });
});

Then("the MCP tab {string} is not visible", async ({ page }, tab: string) => {
  await expect(page.getByTestId(`mcp-tab-${tab}`)).toHaveCount(0);
});

Then("the my credentials table lists every visible server", async ({ page }) => {
  const rows = page.locator(
    '[data-testid="mcp-my-cred-row"]:visible, [data-testid="mcp-my-cred-card"]:visible',
  );
  await expect(rows).toHaveCount(3, { timeout: 8000 });
});

Then("the my credentials row {string} is flagged as blocking", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias)).toHaveAttribute("data-blocked", "true", {
    timeout: 5000,
  });
});

Then(
  "the my credentials row {string} explains the tools are unavailable",
  async ({ page }, alias: string) => {
    await expect(myCredRow(page, alias)).toContainText(
      /not available to you|对你不开放/i,
      { timeout: 5000 },
    );
  },
);

Then("the my credentials row {string} offers a set button", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias).getByTestId("mcp-my-token-configure")).toBeVisible();
});

Then("the my credentials row {string} shows the token is not needed", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias).getByTestId("mcp-my-token-not-needed")).toBeVisible();
});

Then("the my credentials row {string} offers no set button", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias).getByTestId("mcp-my-token-configure")).toHaveCount(0);
});

Then("the my credentials row {string} shows the token is optional", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias).getByTestId("mcp-my-token-optional")).toBeVisible();
});

When("I open the my token dialog for {string}", async ({ page }, alias: string) => {
  await myCredRow(page, alias).getByTestId("mcp-my-token-configure").click();
  await expect(page.getByTestId("mcp-my-token-set")).toBeVisible({ timeout: 8000 });
  await page.getByTestId("mcp-my-token-set").click();
  await expect(page.getByTestId("mcp-my-token-form")).toBeVisible({ timeout: 5000 });
});

When("I type a personal token into the my token form", async ({ page }) => {
  await page.getByTestId("mcp-my-token-value-input").fill(MCP_MY_TOKEN_PLAINTEXT_VALUE);
});

When("I save the my token form", async ({ page }) => {
  await Promise.all([
    page.waitForResponse(
      (res) => res.url().includes("/my-token") && res.request().method() === "PUT",
      { timeout: 8000 },
    ),
    page.getByTestId("mcp-my-token-save").click(),
  ]);
  expect(getLastMcpMyTokenPutBody().token_value).toBe(MCP_MY_TOKEN_PLAINTEXT_VALUE);
  await page.waitForTimeout(500);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(600);
});

Then("the my credentials row {string} is no longer flagged as blocking", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias)).toHaveAttribute("data-blocked", "false", {
    timeout: 8000,
  });
});

Then("the my credentials row {string} shows the token is configured", async ({ page }, alias: string) => {
  await expect(myCredRow(page, alias).getByTestId("mcp-my-token-configured")).toBeVisible({
    timeout: 5000,
  });
});

Then("the personal token plaintext never appears on the page", async ({ page }) => {
  const html = await page.content();
  expect(html).not.toContain(MCP_MY_TOKEN_PLAINTEXT_VALUE);
});

Then("the personal token plaintext was sent to the API", async ({ page }) => {
  expect(getLastMcpMyTokenPutBody().token_value).toBe(MCP_MY_TOKEN_PLAINTEXT_VALUE);
  const html = await page.content();
  expect(html).not.toContain(MCP_MY_TOKEN_PLAINTEXT_VALUE);
});

Then("no MCP create request was sent", async ({ page }) => {
  void page;
  expect(getMcpCreateRequestCount()).toBe(0);
});

When("I type a shared token value into the credentials form", async ({ page }) => {
  await page.getByTestId("mcp-token-new").click();
  await page.getByTestId("mcp-token-value-input").fill("leaky-shared-token-value");
  await page.waitForTimeout(300);
});

When("I close the MCP credentials dialog", async ({ page }) => {
  await page.keyboard.press("Escape");
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

Then("the reopened credentials form carries no typed value", async ({ page }) => {
  await page.getByTestId("mcp-token-new").click();
  await expect(page.getByTestId("mcp-token-value-input")).toHaveValue("");
});

Then("the typed shared token value never appears in the page", async ({ page }) => {
  expect(await page.content()).not.toContain("leaky-shared-token-value");
});

When("I click the call log button on the {string} MCP server", async ({ page }, alias: string) => {
  await page
    .locator("[data-server-id]:visible")
    .filter({ hasText: alias })
    .first()
    .getByTestId("mcp-server-logs")
    .click();
  await page.waitForTimeout(800);
});

When("I open the MCP call log tab", async ({ page }) => {
  await page.getByTestId("mcp-tab-calls").first().click();
  await page.waitForTimeout(800);
});

When("I choose the MCP call status {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-calls-status-filter").selectOption(value);
  await page.waitForTimeout(600);
});

When("I search MCP calls for tool {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-calls-tool-filter").fill(value);
  // The input is debounced by 300 ms; anything faster asserts the previous request.
  await page.waitForTimeout(800);
});

When("I choose the MCP call time preset {string}", async ({ page }, value: string) => {
  await page.getByTestId(`mcp-calls-preset-${value}`).click();
  await page.waitForTimeout(600);
});

Then("the MCP call log shows {int} calls", async ({ page }, count: number) => {
  await expect(callRows(page)).toHaveCount(count, { timeout: 8000 });
});

Then("the MCP call log alias filter carries {string}", async ({ page }, alias: string) => {
  await expect(page.getByTestId("mcp-calls-alias-filter")).toHaveValue(alias);
  expect(getLastMcpCallsQuery()?.get("server_alias")).toBe(alias);
});

Then("the MCP call log request carries status {string}", async ({ page }, value: string) => {
  void page;
  await expect
    .poll(() => getLastMcpCallsQuery()?.get("status"), { timeout: 8000 })
    .toBe(value);
});

Then("the MCP call log request carries tool_name {string}", async ({ page }, value: string) => {
  void page;
  await expect
    .poll(() => getLastMcpCallsQuery()?.get("tool_name"), { timeout: 8000 })
    .toBe(value);
});

Then("the MCP call log request carries a time window", async ({ page }) => {
  void page;
  await expect
    .poll(() => getLastMcpCallsQuery()?.get("from"), { timeout: 8000 })
    .not.toBeNull();
  expect(Date.parse(getLastMcpCallsQuery()?.get("to") ?? "")).toBeGreaterThan(0);
});

Then("the MCP call log shows an empty state", async ({ page }) => {
  await expect(page.getByTestId("mcp-calls-empty")).toBeVisible({ timeout: 8000 });
  await expect(callRows(page)).toHaveCount(0);
});

When("I open the first MCP call row", async ({ page }) => {
  await callRows(page).first().click();
  await expect(page.getByTestId("mcp-call-detail")).toBeVisible({ timeout: 5000 });
});

Then("the MCP call detail lists the call id and byte counts", async ({ page }) => {
  const detail = page.getByTestId("mcp-call-detail");
  // Call ids are unreadable 36-char UUIDs; byte counts are the only row values a
  // human would scan. Both must be in the drawer, and neither is in the table.
  await expect(detail).toContainText("0192f0c4-0000-7000-8000-000000000001");
  await expect(detail).toContainText("1536");
});

/** The desktop table and the mobile card both render the user cell; at any one
 *  viewport only one of them is visible, so assertions must not count the other. */
function userCells(page: Page) {
  return page.locator('[data-testid="mcp-call-user"]:visible');
}

Then("the MCP call log shows the user column", async ({ page }) => {
  await expect(userCells(page).first()).toBeVisible({ timeout: 8000 });
});

Then("the anonymous call row shows {string} in the user column", async ({ page }, text: string) => {
  const anonymous = callRows(page).filter({ hasText: "get_issue" }).first();
  await expect(anonymous.locator('[data-testid="mcp-call-user"]')).toHaveText(text);
});

Then("the MCP call log has no user column", async ({ page }) => {
  await expect(page.locator('[data-testid="mcp-call-user"]')).toHaveCount(0);
});

Then("the MCP call log only carries the signed-in user's rows", async ({ page }) => {
  await expect(callRows(page)).toHaveCount(1, { timeout: 8000 });
  // `user_id` is not part of this endpoint's vocabulary: the server scopes by
  // key, so a client cannot widen the result by asking for another identity.
  expect(getLastMcpCallsPath()).toBe("/v1/mcp/calls");
  expect(getLastMcpCallsQuery()?.get("user_id")).toBeNull();
});

Then("the failed MCP call row is highlighted", async ({ page }) => {
  const failed = page.locator('[data-testid="mcp-call-row"][data-failed="true"]');
  await expect(failed).toHaveCount(1, { timeout: 5000 });
  await expect(failed.first()).toContainText(/-32603/);
});

Then("the MCP tool {string} shows no restriction", async ({ page }, tool: string) => {
  await expect(toolRow(page, tool).getByTestId("mcp-tool-acl-rule")).toHaveCount(0, {
    timeout: 8000,
  });
});

When("I click the restrict button on the MCP tool {string}", async ({ page }, tool: string) => {
  await toolRow(page, tool).getByTestId("mcp-tool-acl-edit").click();
  await expect(page.getByTestId("mcp-acl-form")).toBeVisible({ timeout: 5000 });
});

When(
  "I restrict it to {word} {string} as {word}",
  async ({ page }, scopeType: string, scopeId: string, decision: string) => {
    await page.getByTestId("mcp-acl-scope-type").selectOption(scopeType);
    await page.getByTestId("mcp-acl-scope-id").fill(scopeId);
    const allowedSwitch = page.getByTestId("mcp-acl-allowed");
    const wantAllowed = decision === "allowed";
    const isAllowed = (await allowedSwitch.getAttribute("aria-checked")) === "true";
    if (isAllowed !== wantAllowed) await allowedSwitch.click();
    await page.getByTestId("mcp-acl-save").click();
    await page.waitForTimeout(800);
  },
);

Then(
  "the MCP tool {string} shows the restriction {string}",
  async ({ page }, tool: string, restriction: string) => {
    await expect(toolRow(page, tool).getByTestId("mcp-tool-acl-rule").first()).toContainText(
      restriction,
      { timeout: 5000 },
    );
  },
);

Then(
  "the MCP tool {string} shows both restrictions {string} and {string}",
  async ({ page }, tool: string, first: string, second: string) => {
    const badges = toolRow(page, tool).getByTestId("mcp-tool-acl-rule");
    await expect(badges).toHaveCount(2, { timeout: 5000 });
    await expect(badges.filter({ hasText: first })).toHaveCount(1);
    await expect(badges.filter({ hasText: second })).toHaveCount(1);
  },
);

Then("the MCP delete confirmation is shown", async ({ page }) => {
  await expect(page.getByTestId("mcp-delete-confirm")).toBeVisible({ timeout: 5000 });
});
When("I confirm the MCP deletion", async ({ page }) => {
  await page.getByTestId("mcp-delete-confirm").click();
  await page.waitForTimeout(900);
});

Then("the MCP server {string} is still listed", async ({ page }, alias: string) => {
  await expect(
    page.locator('[data-testid="mcp-server-alias"]:visible').filter({ hasText: alias }).first(),
  ).toBeVisible({ timeout: 5000 });
});
