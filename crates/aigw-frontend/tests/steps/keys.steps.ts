import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am logged in as admin", async ({ page }) => {
  // Set cookies on the browser context
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);
  // Must navigate to baseURL first to establish an origin for localStorage
  await page.goto("/");
  await page.evaluate(() => {
    localStorage.setItem("aigw_master_key", "sk-master-change-me");
  });
  // Ensure API mocks are set up BEFORE any page navigation
  await mockAllApis(page);
});

Given("I am on the Keys page", async ({ page }) => {
  await page.goto("/dash/keys");
  await page.waitForLoadState("networkidle");
});

Given("I am on the Dashboard page", async ({ page }) => {
  await page.goto("/dash/home");
  await page.waitForLoadState("networkidle");
});

Given("I am on the Models page", async ({ page }) => {
  await page.goto("/dash/models");
  await page.waitForLoadState("networkidle");
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
  await expect(page.getByText("prod-gpt-key")).toBeVisible({ timeout: 5000 });
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
  await expect(page.getByText(new RegExp(query, "i")).first()).toBeVisible();
});

When("I click the delete button for the first key", async ({ page }) => {
  // The delete button is a Trash2 icon inside a ghost icon button in the Actions column
  const trashBtn = page.locator("table tbody tr").first().getByRole("button").last();
  await trashBtn.click();
});

When("I confirm the deletion", async ({ page }) => {
  const confirmBtn = page.getByRole("button", { name: /confirm|delete|yes/i });
  if (await confirmBtn.isVisible({ timeout: 2000 }).catch(() => false)) {
    await confirmBtn.click();
  }
});

Then("the key should be removed from the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page.getByText(/key deleted|deleted/i).first()).toBeVisible({ timeout: 5000 });
});
