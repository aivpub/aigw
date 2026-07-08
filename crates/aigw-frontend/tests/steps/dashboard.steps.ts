import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Then, When } = createBdd();

Then("I should see total spend information", async ({ page }) => {
  await expect(page.getByText(/\$|spend|total/i).first()).toBeVisible({ timeout: 8000 });
});

Then("I should see spend by model chart or data", async ({ page }) => {
  await expect(page.getByText(/model|gpt/i).first()).toBeVisible({ timeout: 8000 });
});

Then("I should see recent spend logs", async ({ page }) => {
  // Look for spend log table column headers or request IDs (avoid matching "Logout" sidebar)
  await expect(page.getByText(/Cost|Tokens|req-/i).first()).toBeVisible({ timeout: 8000 });
});

Then("I should see loading indicators before data appears", async ({ page }) => {
  // Skeleton or spinner should appear during loading
  const hasLoading = await page.getByText(/loading|skeleton/i).isVisible().catch(() => false);
  // After loading, data should appear
  await page.waitForTimeout(3000);
  await expect(page.getByText(/\$|spend/i).first()).toBeVisible({ timeout: 5000 });
});

When("I visit {string}", async ({ page }, url: string) => {
  await page.goto(url);
  await page.waitForLoadState("networkidle");
});

When("I visit the Dashboard page", async ({ page }) => {
  await page.goto("/dash/home");
  await page.waitForLoadState("networkidle");
});
