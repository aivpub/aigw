import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Given, When, Then } = createBdd();

Given("I am on the Playground page", async ({ page }) => {
  await page.goto("/dash/playground");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

Then("I should see the model selector dropdown", async ({ page }) => {
  // shadcn/ui Select renders button[role="combobox"] without accessible name.
  // The placeholder text is inside the combobox button.
  await expect(page.getByRole("combobox").filter({ hasText: /select a model/i })).toBeVisible({ timeout: 5000 });
});

Then("I should see the system prompt textarea", async ({ page }) => {
  await expect(page.getByPlaceholder(/You are a helpful assistant/i)).toBeVisible({ timeout: 3000 });
});

Then("I should see the user message textarea", async ({ page }) => {
  await expect(page.getByPlaceholder(/Enter your message/i)).toBeVisible({ timeout: 3000 });
});

Then("I should see the send button", async ({ page }) => {
  await expect(page.getByRole("button", { name: /^send$/i })).toBeVisible({ timeout: 3000 });
});

Then("I should see the streaming toggle", async ({ page }) => {
  await expect(page.getByText(/streaming/i)).toBeVisible({ timeout: 3000 });
});

Then("the response area should show {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible({ timeout: 5000 });
});

When("I select model {string} from the dropdown", async ({ page }, model: string) => {
  // Click the combobox to open dropdown
  await page.getByRole("combobox").first().click();
  await page.waitForTimeout(300);
  // Select the model option from the portal
  await page.getByRole("option", { name: model }).click();
});

Then("the model {string} should be selected", async ({ page }, model: string) => {
  // After selection, the combobox should show the model name instead of placeholder
  await expect(page.getByRole("combobox").first()).toContainText(model);
});

When("I type {string} into the user message", async ({ page }, message: string) => {
  const textarea = page.getByPlaceholder(/Enter your message/i);
  await textarea.fill(message);
});

When("I type {string} into the system prompt", async ({ page }, prompt: string) => {
  const textarea = page.getByPlaceholder(/You are a helpful assistant/i);
  await textarea.fill(prompt);
});

When("I click the Send button", async ({ page }) => {
  // Ensure a model is selected before sending
  const trigger = page.getByRole("combobox").first();
  const modelText = await trigger.textContent();
  if (!modelText || modelText.includes("Select a model")) {
    await trigger.click();
    await page.waitForTimeout(300);
    await page.getByRole("option").first().click();
    await page.waitForTimeout(300);
  }
  await page.getByRole("button", { name: /^send$/i }).click();
});

Then("I should see a response in the response area", async ({ page }) => {
  // Wait for response content to appear — either prose div or any non-empty content
  await page.waitForTimeout(2000);
  // The response card should no longer show the empty placeholder
  await expect(page.getByText("Enter a message and click Send to test")).not.toBeVisible({ timeout: 10000 });
  // Something should be rendered in the response card
  const hasContent = await page.locator(".prose, [class*='prose']").isVisible().catch(() => false);
  const hasProseLike = await page.getByText(/mock|hello/i).isVisible().catch(() => false);
  expect(hasContent || hasProseLike).toBeTruthy();
});

When("I toggle streaming on", async ({ page }) => {
  // Click the Switch component role="switch"
  const switchButton = page.locator("button[role='switch']");
  await switchButton.click();
  await page.waitForTimeout(300);
});

Then("the playground should be displayed in a mobile-friendly format", async ({ page }) => {
  await page.waitForTimeout(1000);
  // On mobile, the page should render with stacked layout
  await expect(page.getByText(/playground/i).first()).toBeVisible({ timeout: 5000 });
  // Verify mobile viewport
  const viewportSize = page.viewportSize();
  expect(viewportSize?.width).toBeLessThanOrEqual(375);
  // Content should be visible (not just blank)
  await expect(page.getByText(/system prompt|enter your message|select a model/i).first()).toBeVisible({ timeout: 3000 });
});
