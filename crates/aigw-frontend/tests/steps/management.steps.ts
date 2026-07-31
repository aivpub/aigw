import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { When, Then } = createBdd();

When("I visit the Users page", async ({ page }) => {
  await page.goto("/dash/users");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

When("I fill in the user creation form", async ({ page }) => {
  // Find the dialog's email input by label or id (placeholder "user@example.com" doesn't contain "email")
  const dialog = page.locator("[role=dialog]:visible");
  const email = dialog.locator("#u-email").first();
  if (await email.isVisible({ timeout: 2000 }).catch(() => false)) {
    await email.fill("new@example.com");
  }
  const pw = dialog.locator("#u-password").first();
  if (await pw.isVisible({ timeout: 2000 }).catch(() => false)) {
    await pw.fill("secret123");
  }
});

When("I submit the user creation form", async ({ page }) => {
  await page.locator("[role=dialog]:visible").getByRole("button", { name: /^create$/i }).click();
});

Then("I should see a user named {string} in the list", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});

Then("I should see a success toast", async ({ page }) => {
  // Look for the toast element directly — avoid matching table header "Created" column
  await expect(page.locator("[data-sonner-toast]").getByText(/created|success/i)).toBeVisible({ timeout: 5000 });
});

When("I click the delete button for the first user", async ({ page }) => {
  const tableTrash = page.locator("table:visible tbody tr").first().getByRole("button").last();
  const cardTrash = page.locator(".md\\:hidden:visible button:has(svg)").filter({ hasText: /delete/i }).first();
  if (await tableTrash.isVisible({ timeout: 1000 }).catch(() => false)) {
    await tableTrash.click();
  } else if (await cardTrash.isVisible({ timeout: 1000 }).catch(() => false)) {
    await cardTrash.click();
  }
});

When("I confirm the deletion in the dialog", async ({ page }) => {
  const dialogDelete = page.locator("[role=dialog]:visible").getByRole("button", { name: /^delete$/i });
  if (await dialogDelete.isVisible({ timeout: 2000 }).catch(() => false)) {
    await dialogDelete.click();
  }
});

Then("I should see a deletion success toast", async ({ page }) => {
  await page.waitForTimeout(500);
  await expect(page.getByText(/deleted/i).first()).toBeVisible({ timeout: 5000 });
});

// Orgs
When("I visit the Orgs page", async ({ page }) => {
  await page.goto("/dash/orgs");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

Then("I should see an org named {string} in the list", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});

// Teams
When("I visit the Teams page", async ({ page }) => {
  await page.goto("/dash/teams");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

Then("I should see a team named {string} in the list", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});
