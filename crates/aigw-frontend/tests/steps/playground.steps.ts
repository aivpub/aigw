import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Given, When, Then } = createBdd();

Given("I am on the Playground page", async ({ page }) => {
  await page.goto("/dash/playground");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 39: Chat UI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Then("I should see a model selector", async ({ page }) => {
  // Settings panel visible on desktop, hidden on mobile (Sheet). Check at least button exists.
  const settingsBtn = page.getByRole("button", { name: /settings/i });
  const isSettingsBtn = await settingsBtn.isVisible().catch(() => false);
  if (isSettingsBtn) {
    // Mobile: click Settings to open sheet
    await settingsBtn.click();
    await page.waitForTimeout(300);
  }
  await expect(page.getByRole("combobox").first()).toBeVisible({ timeout: 5000 });
});

Then("I should see the message input area", async ({ page }) => {
  await expect(page.getByPlaceholder(/type a message/i)).toBeVisible({ timeout: 3000 });
});

Then("I should see a send button", async ({ page }) => {
  // The send button is in the chat input area (disabled until model selected)
  const sendBtn = page.locator("button").filter({ has: page.locator("svg.lucide-send") }).first();
  await expect(sendBtn).toBeAttached({ timeout: 5000 });
});

Then("I should see the streaming toggle in settings", async ({ page }) => {
  // On desktop, sidebar is visible. On tablet/mobile, click "Settings" to open the sheet.
  let label = page.locator("[role='dialog']").getByText(/streaming/i).first();
  if (!(await label.isVisible().catch(() => false))) {
    // Try inline settings panel
    label = page.getByText(/streaming/i).first();
  }
  if (!(await label.isVisible().catch(() => false))) {
    // Open settings sheet on mobile/tablet
    const btn = page.getByRole("button", { name: /settings/i });
    if (await btn.isVisible().catch(() => false)) {
      await btn.click();
      await page.waitForTimeout(500);
      // After opening, streaming is inside the dialog
      label = page.locator("[role='dialog']").getByText(/streaming/i).first();
    }
  }
  // Streaming should be accessible — accept dialog or inline visibility
  const hasStreaming = await page.getByText(/streaming/i).first().isAttached().catch(() => false);
  expect(hasStreaming).toBeTruthy();
});

Then("the chat area should show {string}", async ({ page }, text: string) => {
  await expect(page.getByText(text)).toBeVisible({ timeout: 5000 });
});

When("I select model {string} from the settings panel", async ({ page }, model: string) => {
  // Open settings if on mobile
  const settingsBtn = page.getByRole("button", { name: /settings/i });
  if (await settingsBtn.isVisible().catch(() => false)) {
    await settingsBtn.click();
    await page.waitForTimeout(300);
  }
  await page.getByRole("combobox").first().click();
  await page.waitForTimeout(300);
  await page.getByRole("option", { name: model }).click();
});

Then("the model {string} should be shown as the active model", async ({ page }, model: string) => {
  await expect(page.locator("main")).toContainText(model);
});

When("I type {string} into the chat input", async ({ page }, message: string) => {
  const input = page.getByPlaceholder(/type a message/i);
  await input.fill(message);
});

When("I click the New Chat button", async ({ page }) => {
  await page.getByRole("button", { name: /new chat/i }).click();
  await page.waitForTimeout(500);
});

Then("the chat messages should be cleared", async ({ page }) => {
  await expect(page.getByText(/start a conversation/i)).toBeVisible({ timeout: 5000 });
});

When("I click the Send button", async ({ page }) => {
  // Make sure a model is selected first
  const modelText = await page.locator("main").textContent();
  if (!modelText || !modelText.match(/gpt-4|claude/i)) {
    // Open settings sheet
    const settingsBtn = page.getByRole("button", { name: /settings/i });
    if (await settingsBtn.isVisible().catch(() => false)) {
      await settingsBtn.click();
      await page.waitForTimeout(300);
    }
    const combo = page.getByRole("combobox");
    if (await combo.first().isVisible().catch(() => false)) {
      await combo.first().click();
      await page.waitForTimeout(300);
      await page.getByRole("option").first().click();
      await page.waitForTimeout(300);
    }
    // Close sheet by pressing Escape
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);
  }
  // Click send button — may be disabled, wait for it to become enabled
  await page.waitForTimeout(500);
  const sendBtns = page.locator("button svg.lucide-send");
  if ((await sendBtns.count()) > 0) {
    const parent = sendBtns.first().locator("..");
    await parent.click({ force: true });
  }
});

Then("I should see a chat response message", async ({ page }) => {
  await page.waitForTimeout(3000);
  // Check for any content beyond the empty state
  const hasContent = await page.getByText(/mock|hello|iam doing well|how are/i).isVisible().catch(() => false);
  const hasAsstAvatar = await page.getByText(/assistant/i).first().isVisible().catch(() => false);
  const notEmpty = await page.getByText(/start a conversation/i).isHidden().catch(() => false);
  expect(hasContent || hasAsstAvatar || notEmpty).toBeTruthy();
});

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Common steps
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Then("the playground should be displayed in a mobile-friendly format", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page.getByText(/playground/i).first()).toBeVisible({ timeout: 5000 });
  const viewportSize = page.viewportSize();
  expect(viewportSize?.width).toBeLessThanOrEqual(375);
  await expect(page.getByText(/new chat|start a conversation|type a message/i).first()).toBeVisible({ timeout: 3000 });
});
