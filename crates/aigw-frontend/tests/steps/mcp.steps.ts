import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import {
  mockAllApis,
  MCP_TOKEN_PLAINTEXT,
  getLastMcpServerBody,
} from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am on the MCP page", async ({ page }) => {
  await mockAllApis(page);
  await page.goto("/dash/mcp");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

/** Server rows exist in a desktop table and a mobile card; both carry the same testids. */
function serverRow(page: Page, alias: string) {
  return page
    .locator('[data-server-id]:visible')
    .filter({ hasText: alias })
    .first();
}

function mcpDialog(page: Page) {
  return page.locator('[role="dialog"]');
}

Then("I should see {int} MCP servers in the list", async ({ page }, count: number) => {
  await expect(page.locator('[data-testid="mcp-server-alias"]')).toHaveCount(count * 2, {
    timeout: 8000,
  });
  await expect(page.locator("main")).toContainText("github", { timeout: 5000 });
  await expect(page.locator("main")).toContainText("hr-portal", { timeout: 5000 });
});

Then("the MCP page header has both action buttons", async ({ page }) => {
  await expect(page.getByTestId("mcp-new-server").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByTestId("mcp-reload-all").first()).toBeVisible({ timeout: 5000 });
});

When("I click {string} on the MCP page", async ({ page }, label: string) => {
  await page.getByTestId("mcp-new-server").click();
  void label;
  await page.waitForTimeout(400);
});

When("I fill MCP alias with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-alias-input").fill(value);
  await page.waitForTimeout(300);
});

When("I fill MCP display name with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-display-name-input").fill(value);
});

When("I fill MCP URL with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-url-input").fill(value);
});

When("I click the save button in the MCP server dialog", async ({ page }) => {
  await mcpDialog(page).getByTestId("mcp-server-save").click();
  await page.waitForTimeout(700);
});

Then("the MCP server dialog closes", async ({ page }) => {
  await expect(mcpDialog(page)).not.toBeVisible({ timeout: 5000 });
});

Then("the new MCP server {string} appears in the list", async ({ page }, alias: string) => {
  await expect(
    page.locator('[data-testid="mcp-server-alias"]:visible').filter({ hasText: alias }).first(),
  ).toBeVisible({ timeout: 8000 });
});

Then("the MCP page has no horizontal page scroll", async ({ page }) => {
  const overflow = await page.evaluate(() => {
    const doc = document.documentElement;
    return doc.scrollWidth - doc.clientWidth;
  });
  expect(overflow).toBeLessThanOrEqual(0);
});

Then("I see an MCP success toast", async ({ page }) => {
  await expect(page.locator("[data-sonner-toast]").first()).toBeVisible({ timeout: 5000 });
});

Then("the MCP alias error is shown", async ({ page }) => {
  await expect(page.getByTestId("mcp-alias-error")).toBeVisible({ timeout: 5000 });
});

Then("the MCP save button is disabled", async ({ page }) => {
  await expect(page.getByTestId("mcp-server-save")).toBeDisabled();
});

Then("the MCP alias error mentions a duplicate", async ({ page }) => {
  await expect(page.getByTestId("mcp-alias-error")).toContainText(
    /duplicate|already registered|已.*注册|重复/i,
    { timeout: 5000 },
  );
});

When("I select the MCP status filter {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-status-filter").selectOption(value);
  await page.waitForTimeout(700);
});

Then("the MCP server {string} is not listed", async ({ page }, alias: string) => {
  await expect(
    page.locator('[data-testid="mcp-server-alias"]:visible').filter({ hasText: alias }),
  ).toHaveCount(0, { timeout: 8000 });
});

When("I click the probe button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-probe").click();
  await page.waitForTimeout(900);
});

Then(
  "the MCP probe dialog shows the protocol version {string}",
  async ({ page }, version: string) => {
    await expect(page.getByTestId("mcp-probe-outcome")).toContainText(version, {
      timeout: 8000,
    });
  },
);

Then("the MCP probe dialog shows {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-probe-tool-count")).toHaveText(String(count), {
    timeout: 8000,
  });
});

Then("the MCP probe dialog shows a conflict block with {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-probe-conflicts")).toBeVisible({ timeout: 8000 });
  await expect(page.getByTestId("mcp-probe-conflict-row")).toHaveCount(count);
});

When(
  "I click the credentials button on the {string} MCP server",
  async ({ page }, alias: string) => {
    await serverRow(page, alias).getByTestId("mcp-server-credentials").click();
    await page.waitForTimeout(800);
  },
);

Then("every MCP token value is redacted", async ({ page }) => {
  const values = page.getByTestId("mcp-token-value");
  await expect(values.first()).toBeVisible({ timeout: 5000 });
  for (const value of await values.allTextContents()) {
    expect(value.trim()).toBe("***");
  }
});

Then("the token plaintext never appears in the page", async ({ page }) => {
  const html = await page.content();
  expect(html).not.toContain(MCP_TOKEN_PLAINTEXT);
});

When("I set the {string} shared token as default", async ({ page }, tokenName: string) => {
  const row = page
    .getByTestId("mcp-token-shared-section")
    .getByTestId("mcp-token-row")
    .filter({ hasText: tokenName })
    .first();
  await row.getByTestId("mcp-token-set-default").click();
  await page.waitForTimeout(800);
});

Then("only {string} carries the default badge", async ({ page }, tokenName: string) => {
  const badged = page
    .getByTestId("mcp-token-shared-section")
    .getByTestId("mcp-token-row")
    .filter({ has: page.getByTestId("mcp-token-default-badge") });
  await expect(badged).toHaveCount(1, { timeout: 5000 });
  await expect(badged.first()).toContainText(tokenName);
});

When("I click the tools button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-tools").click();
  await page.waitForTimeout(800);
});

Then("the MCP tools dialog lists {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-tool-row")).toHaveCount(count, { timeout: 5000 });
});

When("I click the toggle button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-toggle").click();
  await page.waitForTimeout(800);
});

Then("the MCP server {string} is shown as disabled", async ({ page }, alias: string) => {
  await expect(
    serverRow(page, alias).getByTestId("mcp-status-badge").first(),
  ).toContainText(/停用|Disabled/i, { timeout: 5000 });
});

When("I click the delete button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-delete").click();
  await page.waitForTimeout(800);
});

When("I click the edit button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-edit").click();
  await page.waitForTimeout(500);
});

When("I choose the custom auth header in the MCP server dialog", async ({ page }) => {
  await mcpDialog(page).getByTestId("mcp-auth-custom").click();
});

When("I fill MCP auth header name with {string}", async ({ page }, value: string) => {
  await mcpDialog(page).getByTestId("mcp-auth-name-input").fill(value);
});

When("I fill MCP auth header value with {string}", async ({ page }, value: string) => {
  await mcpDialog(page).getByTestId("mcp-auth-value-input").fill(value);
});

When(
  "I add an MCP extra header {string} with value {string}",
  async ({ page }, name: string, value: string) => {
    const dialog = mcpDialog(page);
    await dialog.getByTestId("mcp-extra-header-add").click();
    await dialog.getByTestId("mcp-extra-header-name-0").fill(name);
    await dialog.getByTestId("mcp-extra-header-value-0").fill(value);
  },
);

Then(
  "the last MCP server request names the auth header {string}",
  async ({}, name: string) => {
    expect(getLastMcpServerBody().auth_header_name).toBe(name);
  },
);

Then(
  "the last MCP server request carries the extra header {string}",
  async ({}, name: string) => {
    const headers = getLastMcpServerBody().extra_headers as { name: string }[];
    expect(headers.map((h) => h.name)).toContain(name);
  },
);

Then("the MCP auth name error mentions a reserved header", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-name-error")).toContainText(/gateway|网关/i, {
    timeout: 5000,
  });
});

Then("the MCP auth value is shown masked", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-value-masked")).toHaveText("***");
});

Then("the MCP auth value input is empty", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-value-input")).toHaveValue("");
});

Then(
  "the last MCP server request does not carry a masked auth value",
  async ({}) => {
    const value = getLastMcpServerBody().auth_header_value;
    expect(value ?? "").not.toBe("***");
  },
);

Then("the typed MCP auth value never appears in the page", async ({ page }) => {
  expect(await page.content()).not.toContain("tapd-secret");
});
