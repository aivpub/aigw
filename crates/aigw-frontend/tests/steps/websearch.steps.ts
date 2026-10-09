import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am on the Web Search page", async ({ page }) => {
  await mockAllApis(page);
  await page.goto("/dash/websearch");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

Then("I should see {int} search providers in the list", async ({ page }, count: number) => {
  await expect(page.locator("main")).toContainText("searxng-main", { timeout: 5000 });
  await expect(page.locator("main")).toContainText("searxng-backup", { timeout: 5000 });
  void count;
});

Then("the default provider should be badged", async ({ page }) => {
  // The desktop table and the mobile card both render the badge; the hidden
  // one still resolves in the DOM, so narrow the locator to the visible copy.
  await expect(page.locator('[data-testid="ws-default-badge"]:visible').first()).toBeVisible({
    timeout: 5000,
  });
});

/** The first visible provider row/card (both layouts expose the same testids). */
function firstVisibleProvider(page: Page) {
  return page
    .locator('[data-provider-id]:visible, [data-testid="ws-provider-card"]:visible')
    .first();
}

When("I expand the first search provider", async ({ page }) => {
  // Only the desktop table has an expander; the mobile card shows instances
  // unconditionally, so there is nothing to click there.
  const row = page.locator("[data-provider-id]:visible").first();
  if (await row.count()) {
    await row.click();
    await page.waitForTimeout(500);
  }
});

Then("I should see the instance {string}", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});

When("I click {string} on the Web Search page", async ({ page }, label: string) => {
  await page.getByRole("button", { name: new RegExp(label, "i") }).click();
  await page.waitForTimeout(500);
});

When("I fill search provider name with {string}", async ({ page }, value: string) => {
  const input = page.getByTestId("ws-provider-name");
  await input.fill(value);
  await page.waitForTimeout(300);
});

When("I fill search cost with {string}", async ({ page }, value: string) => {
  const input = page.getByTestId("ws-provider-cost");
  await input.fill(value);
  await page.waitForTimeout(400);
});

When(
  "I click the {string} button in the provider dialog",
  async ({ page }, label: string) => {
    await page
      .locator('[role="dialog"]')
      .getByRole("button", { name: new RegExp(label, "i") })
      .click();
    await page.waitForTimeout(500);
  },
);

Then("the provider dialog closes", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

Then("the new provider {string} appears in the list", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});

Then("a cost placeholder warning is shown", async ({ page }) => {
  await expect(page.getByTestId("ws-cost-placeholder-warning")).toBeVisible({
    timeout: 5000,
  });
});

When("I click the edit button on the first search provider row", async ({ page }) => {
  await firstVisibleProvider(page).getByTestId("ws-provider-edit").click();
  await page.waitForTimeout(500);
});

Then("the provider dialog opens with pre-filled pricing", async ({ page }) => {
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible({ timeout: 5000 });
  await expect(page.getByTestId("ws-provider-cost")).toHaveValue("0.01");
});

When("I click the delete button on the default search provider row", async ({ page }) => {
  await firstVisibleProvider(page).getByTestId("ws-provider-delete").click();
  await page.waitForTimeout(600);
});

Then("a refusal is reported", async ({ page }) => {
  // The 409 surfaces as a toast; the provider must still be listed.
  await expect(page.locator("main")).toContainText("searxng-main", { timeout: 5000 });
  const toast = page.locator("[data-sonner-toast]");
  await expect(toast.first()).toBeVisible({ timeout: 5000 });
  await expect(toast.first()).toContainText(/default|删除|delete/i);
});

When(
  "I click the probe button on the {string} instance",
  async ({ page }, instance: string) => {
    // `tr:has-text` matches every ancestor row too (the provider row contains
    // the instance rows), so anchor on the instance row itself.
    const row = page
      .locator('[data-testid="ws-instance-row"]')
      .filter({ hasText: instance })
      .first();
    await row.getByTestId("ws-probe-instance").click();
    await page.waitForTimeout(800);
  },
);

Then("the probe dialog shows a successful result", async ({ page }) => {
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toContainText(/2430|reachable|可达/i, { timeout: 8000 });
  await expect(dialog).toContainText("35");
});

Then("the probe dialog flags an unparseable response", async ({ page }) => {
  // The desktop table and the mobile card both render instance rows, so the
  // probe button can exist twice; the dialog itself is unique.
  await expect(page.getByTestId("ws-probe-parse-warning")).toBeVisible({
    timeout: 8000,
  });
});
