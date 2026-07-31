import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

// We use addInitScript to set navigator.language BEFORE i18next initializes.
// Playwright-bdd uses a shared page/browserContext across steps within the same scenario,
// so we set up init scripts in the Given step, before navigating to the page.

Given("I prepare Chinese browser locale", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "language", { value: "zh-CN", configurable: true });
    Object.defineProperty(navigator, "languages", { value: ["zh-CN"], configurable: true });
  });
});

Given("I prepare English browser locale", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "language", { value: "en-US", configurable: true });
    Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });
  });
});

Given("I clear the aigw-language storage", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.removeItem("aigw-language");
  });
});

Given("I pre-set localStorage {string} to {string}", async ({ page }, key: string, value: string) => {
  await page.addInitScript(([k, v]) => {
    localStorage.setItem(k, v);
  }, [key, value]);
});

When("I load {string}", async ({ page }, path: string) => {
  // Mock all APIs first, then navigate. The baseURL is http://localhost:5173 so
  // we need the full URL, but mockAllApis mocks relative paths.
  await mockAllApis(page);
  // Navigate relative to baseURL (localhost:5173)
  const fullUrl = `http://localhost:5173${path}`;
  await page.goto(fullUrl);
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1500);
});

Then("the sidebar should show {string} menu item", async ({ page }, text: string) => {
  // The sidebar could be invisible on mobile
  const hamburger = page.locator('button:has(svg.lucide-menu)').first();
  if (await hamburger.isVisible({ timeout: 1000 }).catch(() => false)) {
    await hamburger.click();
    await page.waitForTimeout(500);
  }
  const sidebar = page.locator("aside").first();
  await expect(sidebar).toBeVisible({ timeout: 5000 });
  await expect(sidebar.getByText(text, { exact: false }).first()).toBeVisible({ timeout: 5000 });
});
