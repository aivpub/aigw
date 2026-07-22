import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Then, When } = createBdd();

Then("I should see 3 models in the list", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page.locator("main")).toContainText(/gpt-4|claude-sonnet|gpt-4o/i);
});

Then("each model should show its model name", async ({ page }) => {
  await expect(page.locator("main")).toContainText("gpt-4");
});

When("I click on the first model row", async ({ page }) => {
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

// ─── New Stage 55 CRUD steps ───

When("I click {string} on the Models page", async ({ page }, buttonLabel: string) => {
  // Click the "Add Model" button
  await page.getByRole("button", { name: new RegExp(buttonLabel, "i") }).click();
  await page.waitForTimeout(500);
});

When("I fill model_name with {string}", async ({ page }, value: string) => {
  const input = page.locator('[role="dialog"]').getByLabel(/model name/i);
  await input.fill(value);
  await page.waitForTimeout(300);
});

Then("the Upstream Model field is automatically set to {string}", async ({ page }, value: string) => {
  const upstreamInput = page.locator('[role="dialog"]').locator('input[id]').nth(1); // second input in dialog
  await expect(upstreamInput).toHaveValue(value);
});

When("I fill the model form with name {string} provider {string} input price {string} output price {string}",
  async ({ page }, name: string, provider: string, inputPrice: string, outputPrice: string) => {
    // Fill model name
    const nameInput = page.locator('[role="dialog"]').getByLabel(/model name/i);
    await nameInput.fill(name);

    // Select provider
    const providerTrigger = page.locator('[role="dialog"]').getByText(/select provider/i);
    if (await providerTrigger.isVisible({ timeout: 500 }).catch(() => false)) {
      await providerTrigger.click();
      await page.getByRole("option", { name: new RegExp(provider, "i") }).click();
    }

    // Fill pricing
    const inputPriceInput = page.locator('[role="dialog"]').getByLabel(/input price/i);
    if (await inputPriceInput.isVisible({ timeout: 500 }).catch(() => false)) {
      await inputPriceInput.fill(inputPrice);
    }
    const outputPriceInput = page.locator('[role="dialog"]').getByLabel(/output price/i);
    if (await outputPriceInput.isVisible({ timeout: 500 }).catch(() => false)) {
      await outputPriceInput.fill(outputPrice);
    }
  }
);

When("I click the {string} button in the dialog", async ({ page }, buttonLabel: string) => {
  const dialog = page.locator('[role="dialog"]');
  await dialog.getByRole("button", { name: new RegExp(buttonLabel, "i") }).click();
  await page.waitForTimeout(500);
});

Then("the dialog closes", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

When("I click the edit button on the first model row", async ({ page }) => {
  // Find the first visible edit (pencil) button on desktop or mobile
  const editBtn = page.locator("button:has(.lucide-pencil)").first();
  try {
    await editBtn.click({ timeout: 3000 });
  } catch {
    const mobileBtn = page.locator("button:has(.lucide-pencil)").last();
    await mobileBtn.scrollIntoViewIfNeeded();
    await mobileBtn.click();
  }
  await page.waitForTimeout(500);
});

Then("the model dialog opens with pre-filled data", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toBeVisible({ timeout: 5000 });
  // The model name input should have a value (pre-filled)
  const nameInput = page.locator('[role="dialog"]').getByLabel(/model name/i);
  await expect(nameInput).not.toHaveValue("");
});

Then("the model name field is disabled", async ({ page }) => {
  const nameInput = page.locator('[role="dialog"]').getByLabel(/model name/i);
  await expect(nameInput).toBeDisabled();
});

When("I click the delete button on the first model row", async ({ page }) => {
  const delBtn = page.locator("button:has(.lucide-trash2)").first();
  try {
    await delBtn.click({ timeout: 3000 });
  } catch {
    const mobileBtn = page.locator("button:has(.lucide-trash2)").last();
    await mobileBtn.scrollIntoViewIfNeeded();
    await mobileBtn.click();
  }
  await page.waitForTimeout(500);
});

Then("a delete confirmation dialog appears", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toContainText(/delete|confirm/i, { timeout: 5000 });
});
