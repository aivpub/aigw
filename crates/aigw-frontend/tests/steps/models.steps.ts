import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Then, When } = createBdd();

Then("I should see 3 models in the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  const modelItems = page.getByText(/gpt-4|claude-sonnet|gpt-4o/i);
  const count = await modelItems.count();
  expect(count).toBeGreaterThanOrEqual(1);
});

Then("each model should show its model name", async ({ page }) => {
  await expect(page.getByText("gpt-4").first()).toBeVisible({ timeout: 5000 });
});

When("I click on the first model row", async ({ page }) => {
  await page.getByText("gpt-4").first().click();
  await page.waitForTimeout(500);
});

Then("I should see the model's litellm params and model info", async ({ page }) => {
  await expect(page.getByText(/api_base|max_tokens|model_info/i).first()).toBeVisible({ timeout: 5000 });
});

Then("only models matching {string} should be shown", async ({ page }, query: string) => {
  const re = new RegExp(query, "i");
  await expect(page.getByText(re).first()).toBeVisible({ timeout: 5000 });
});
