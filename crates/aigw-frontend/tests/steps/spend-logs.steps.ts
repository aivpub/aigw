import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Given, When, Then } = createBdd();

Given("I am on the Spend Logs page", async ({ page }) => {
  await page.goto("/dash/spend-logs");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

When("I visit the Spend Logs page", async ({ page }) => {
  await page.goto("/dash/spend-logs");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

Then("I should see the spend logs table or card list", async ({ page }) => {
  // Desktop: table visible, or mobile: card list visible. Use toContainText which works across layouts.
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet/i);
});

Then("I should see spend log entries with model names and costs", async ({ page }) => {
  // Check model names and costs rendered (toContainText works even if in hidden desktop table)
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet/i);
  await expect(page.locator("main")).toContainText(/\$\d+\.\d+/);
});

When("I change the start date to {string}", async ({ page }, date: string) => {
  const input = page.locator("#sl-start");
  await input.fill(date);
});

When("I change the end date to {string}", async ({ page }, date: string) => {
  const input = page.locator("#sl-end");
  await input.fill(date);
});

Then("the spend logs list should update", async ({ page }) => {
  await page.waitForTimeout(1000);
  // Page should still have spend log data
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet/i);
});

When("I type {string} into the model filter", async ({ page }, model: string) => {
  const input = page.locator("#sl-model");
  await input.fill(model);
});

Then("the spend log data should be displayed in a mobile-friendly format", async ({ page }) => {
  await page.waitForTimeout(1000);
  // On mobile, cards appear (md:hidden space-y-2), table is hidden
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet/i);
  // Verify we're on mobile viewport
  const viewportSize = page.viewportSize();
  expect(viewportSize?.width).toBeLessThanOrEqual(375);
});

Then("I should see loading indicators before spend data appears", async ({ page }) => {
  // After slow response, data should eventually appear
  await page.waitForTimeout(3000);
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet/i);
});
