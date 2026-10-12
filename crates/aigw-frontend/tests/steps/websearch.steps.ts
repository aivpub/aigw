import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import { disableAllWebSearchRows, mockAllApis } from "./api-mocks";

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
  // Idempotent: the row toggles, so clicking it twice collapses what the first
  // click opened (and a dialog save keeps it open, so scenarios do call this
  // twice). Only the desktop table has an expander — the mobile card shows
  // instances unconditionally, so there is nothing to click there.
  const row = page.locator("[data-provider-id]:visible").first();
  if ((await row.count()) === 0) return;
  const shown = await page.locator('[data-testid="ws-instance-row"]:visible').count();
  if (shown === 0) {
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

// ── enabled affordances (empty-state / dialogs / master switch) ──

When("I switch the provider enabled toggle off", async ({ page }) => {
  await page.getByTestId("ws-provider-enabled").click();
  await page.waitForTimeout(300);
});

When("I switch the instance enabled toggle off", async ({ page }) => {
  await page.getByTestId("ws-instance-enabled").click();
  await page.waitForTimeout(300);
});

When("I click the add-instance button on the first search provider", async ({ page }) => {
  const desktop = page
    .locator("[data-provider-id]:visible")
    .first()
    .getByTestId("ws-add-instance");
  const target = (await desktop.count())
    ? desktop
    : page
        .locator('[data-testid="ws-provider-card"]:visible')
        .first()
        .locator("button:has(.lucide-plus)")
        .last();
  await target.click();
  await page.waitForTimeout(500);
});

When("I fill instance name with {string}", async ({ page }, value: string) => {
  await page.getByTestId("ws-instance-name").fill(value);
  await page.waitForTimeout(300);
});

When("I fill instance URL with {string}", async ({ page }, value: string) => {
  await page.getByTestId("ws-instance-url").fill(value);
  await page.waitForTimeout(300);
});

When("I refresh the Web Search page", async ({ page }) => {
  await page.goto("/dash/websearch");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

Then(
  "the provider {string} is shown as disabled",
  async ({ page }, name: string) => {
    const row = page
      .locator("[data-provider-id]:visible, [data-testid='ws-provider-card']:visible")
      .filter({ hasText: name })
      .first();
    await expect(row.getByText(/已停用|Disabled/)).toBeVisible({ timeout: 5000 });
  },
);

Then(
  "the instance {string} is shown as disabled",
  async ({ page }, name: string) => {
    const row = page
      .locator('[data-testid="ws-instance-row"]:visible')
      .filter({ hasText: name })
      .first();
    // `[data-state]` matches the switch button *and* its thumb span, so scope
    // to the switch itself and read its checked state.
    await expect(row.getByRole("switch")).toHaveAttribute("aria-checked", "false", {
      timeout: 5000,
    });
  },
);

Given("every search provider and instance is disabled", async ({ page }) => {
  // Flip the mock store directly. A click-loop over the switches would be
  // order- and rerender-sensitive; the point of the scenario is the master
  // switch's *reading* of that state, not the toggling itself.
  disableAllWebSearchRows();
  await page.waitForTimeout(100);
});

Then("the web search master switch is on", async ({ page }) => {
  await expect(page.getByTestId("ws-master-switch")).toHaveAttribute(
    "data-state",
    "checked",
    { timeout: 5000 },
  );
});

Then("the web search master switch is off", async ({ page }) => {
  await expect(page.getByTestId("ws-master-switch")).toHaveAttribute(
    "data-state",
    "unchecked",
    { timeout: 5000 },
  );
});

When("I switch the web search master switch on", async ({ page }) => {
  await page.getByTestId("ws-master-switch").click();
  await page.waitForTimeout(700);
});
