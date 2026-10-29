import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import {
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

Then("the MCP call log shows {int} calls", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-call-row")).toHaveCount(count, { timeout: 8000 });
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
