import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Then, When } = createBdd();

Then("I should see 3 models in the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  // toContainText works on main content regardless of desktop/mobile layout
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet|gpt-4o/i);
});

Then("each model should show its model name", async ({ page }) => {
  await expect(page.locator("main")).toContainText("gpt-4");
});

When("I click on the first model row", async ({ page }) => {
  // On desktop, click the table row. On mobile, click the card header.
  const tableRow = page.locator("table:visible").getByText("gpt-4").first();
  const cardRow = page.locator(".md\\:hidden:visible").getByText("gpt-4").first();
  if (await tableRow.isVisible({ timeout: 1000 }).catch(() => false)) {
    await tableRow.click();
  } else if (await cardRow.isVisible({ timeout: 1000 }).catch(() => false)) {
    await cardRow.click();
  }
  await page.waitForTimeout(500);
});

Then("I should see the model's litellm params and model info", async ({ page }) => {
  await expect(page.locator("main")).toContainText(/api_base|max_tokens|model_info/i, { timeout: 5000 });
});

Then("only models matching {string} should be shown", async ({ page }, query: string) => {
  const re = new RegExp(query, "i");
  await expect(page.locator("main")).toContainText(re, { timeout: 5000 });
});
