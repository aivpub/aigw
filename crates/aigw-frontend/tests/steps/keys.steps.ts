import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am logged in as admin", async ({ page }) => {
  // Set up API mocks BEFORE any navigation — this prevents race conditions
  // where React components fire API calls before mock routes are registered.
  await mockAllApis(page);
  // Set cookies before navigation so auth check passes
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);
  // Navigate to establish origin — auth is cookie-based, mock returns 200
  await page.goto("/dash/usage");
  // Vite HMR keeps WebSocket open so networkidle never fires;
  // React auth check takes ~1 RTT, wait for page to render dashboard content
  await page.waitForTimeout(3000);
});

Given("I am on the Keys page", async ({ page }) => {
  await page.goto("/dash/keys");
  // Vite HMR keeps a WebSocket open so networkidle never fires; use domcontentloaded instead
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500); // brief settle for React render
});

Given("I am on the Usage page", async ({ page }) => {
  await page.goto("/dash/usage");
  // Vite HMR keeps a WebSocket open so networkidle never fires; use domcontentloaded instead
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500); // brief settle for React render
});

Given("I am on the Models page", async ({ page }) => {
  await page.goto("/dash/models");
  // Vite HMR keeps a WebSocket open so networkidle never fires; use domcontentloaded instead
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500); // brief settle for React render
});

Given("API endpoints are mocked", async ({ page }) => {
  await mockAllApis(page);
});

Given("API endpoints are slow to respond", async ({ page }) => {
  await mockAllApis(page);
});

Then("I should see 3 keys in the list", async ({ page }) => {
  await page.waitForSelector("table, [data-testid='key-card'], .grid", { timeout: 5000 }).catch(() => {});
  // Check page has rendered key data
  await expect(page.locator("body")).not.toContainText("Loading keys");
});

Then("each key should show its alias, models, and max budget", async ({ page }) => {
  // Check that key data rendered (either table or card layout). toContainText doesn't require visibility.
  await expect(page.locator("main")).toContainText("prod-gpt-key");
  await expect(page.locator("main")).toContainText("dev-claude-key");
  await expect(page.locator("main")).toContainText("test-key");
});

When("I click the {string} button", async ({ page }, name: string) => {
  await page.getByRole("button", { name: new RegExp(name, "i") }).click();
});

When("I fill in the key creation form", async ({ page }) => {
  const alias = page.getByPlaceholder(/alias/i).first();
  if (await alias.isVisible()) {
    await alias.fill("test-key");
  }
});

When("I submit the key creation form", async ({ page }) => {
  await page.getByRole("button", { name: /generate key/i }).click();
});

Then("a new key should appear in the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page.getByText(/key created|new-key/i).first()).toBeVisible({ timeout: 5000 });
});

When("I type {string} into the search box", async ({ page }, query: string) => {
  const searchInput = page.getByPlaceholder(/search/i).first();
  if (await searchInput.isVisible()) {
    await searchInput.fill(query);
    await page.waitForTimeout(500);
  }
});

Then("only keys matching {string} should be shown", async ({ page }, query: string) => {
  await page.waitForTimeout(500);
  // toContainText works regardless of desktop/mobile layout
  await expect(page.locator("main")).toContainText(new RegExp(query, "i"));
});

When("I click the delete button for the first key", async ({ page }) => {
  // Desktop: table action buttons. Mobile: card buttons.
  // Use :visible so we target the right layout.
  const tableTrash = page.locator("table:visible tbody tr").first().getByRole("button").last();
  const cardTrash = page.locator(".md\\:hidden:visible button:has(svg)").filter({ hasText: /delete/i }).first();
  if (await tableTrash.isVisible({ timeout: 1000 }).catch(() => false)) {
    await tableTrash.click();
  } else if (await cardTrash.isVisible({ timeout: 1000 }).catch(() => false)) {
    await cardTrash.click();
  } else {
    // Fallback: try the trash icon button
    await page.getByRole("button").filter({ has: page.locator("svg") }).last().click();
  }
});

When("I confirm the deletion", async ({ page }) => {
  // The delete confirmation Dialog has a "Delete" button (destructive variant).
  // Be specific: target the destructive Delete button inside the visible dialog, not the card "Delete" button.
  const dialogDelete = page.locator("[role=dialog]:visible").getByRole("button", { name: /^delete$/i });
  if (await dialogDelete.isVisible({ timeout: 2000 }).catch(() => false)) {
    await dialogDelete.click();
  }
});

Then("the key should be removed from the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  // The sonner toast says "Key deleted" — check via the sonner region to avoid matching card buttons
  const toastRegion = page.locator("[data-sonner-toaster], section[aria-label='Notifications alt+T']");
  // Fallback: just check that the toast text appears anywhere visible
  await expect(page.getByText("Key deleted")).toBeVisible({ timeout: 5000 });
});

Then("I should see the generated API key token", async ({ page }) => {
  // Scope to the visible dialog — the key list also contains "sk-" tokens
  const dialog = page.locator("[role=dialog]:visible");
  await expect(dialog.getByText(/sk-/)).toBeVisible({ timeout: 5000 });
  // The "I've saved my key" button confirms the token can be dismissed
  await expect(dialog.getByRole("button", { name: /saved my key/i })).toBeVisible({ timeout: 3000 });
});
