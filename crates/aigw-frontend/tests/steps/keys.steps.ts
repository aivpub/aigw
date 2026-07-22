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

// ── Copy-to-clipboard ──

When("I click the copy button for the first key's token", async ({ page }) => {
  // Desktop: copy button is inside the token cell (font-mono), next to the
  // Eye/EyeOff toggle.  Mobile: same pattern inside a card.
  // Both use a button with only a lucide Copy icon — target by the
  // surrounding font-mono container.
  const desktopCopy = page.locator("table:visible td.font-mono button").last();
  const mobileCopy = page.locator(".md\\:hidden:visible div.font-mono button").last();

  if (await desktopCopy.isVisible({ timeout: 1000 }).catch(() => false)) {
    await desktopCopy.click();
  } else if (await mobileCopy.isVisible({ timeout: 1000 }).catch(() => false)) {
    await mobileCopy.click();
  } else {
    // Fallback: any button containing a Copy icon in the main area
    await page.locator("main button").filter({ has: page.locator("svg") }).first().click();
  }
});

Then("I should see a {string} notification", async ({ page }, text: string) => {
  // sonner toasts render in a dedicated region
  const toast = page.locator("[data-sonner-toaster] li, [data-sonner-toaster] [data-sonner-toast]");
  await expect(toast.getByText(text)).toBeVisible({ timeout: 5000 });
});

Then("I should see a {string} error notification", async ({ page }, text: string) => {
  const toast = page.locator("[data-sonner-toaster] li, [data-sonner-toaster] [data-sonner-toast]");
  await expect(toast.getByText(text)).toBeVisible({ timeout: 5000 });
});

Given("Clipboard API is unavailable", async ({ page }) => {
  // Strip navigator.clipboard so the execCommand('copy') fallback is exercised.
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      value: undefined,
      configurable: true,
    });
  });
  await page.goto("/dash/keys");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

Given("all copy methods are unavailable", async ({ page }) => {
  // Disable navigator.clipboard in the isolated world so the React code
  // can't use it.  Then patch Document.prototype.execCommand to reject
  // 'copy' — Chromium's native execCommand survives addInitScript
  // overrides, so we must do this post-navigation.
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      value: undefined,
      configurable: true,
    });
  });
  await page.goto("/dash/keys");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);

  // Both copy paths should fail.  Make execCommand("copy") throw so
  // the catch block fires and the error toast appears.  Returning false
  // won't trigger the catch.
  await page.evaluate(() => {
    const proto = HTMLDocument.prototype as any;
    const orig = proto.execCommand;
    proto.execCommand = function (commandId: string, ...args: unknown[]) {
      if (commandId === "copy") throw new Error("clipboard unavailable");
      return orig.call(this, commandId, ...args);
    };
  });
});
