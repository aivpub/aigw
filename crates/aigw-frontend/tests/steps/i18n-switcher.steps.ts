import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

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
  await mockAllApis(page);
  await page.goto(path);
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1500);
});

When("I click the language switcher button", async ({ page }) => {
  // On mobile the sidebar may be open (overlay mode z-50). Close it first
  // so the dropdown menu renders above the sidebar.
  const overlay = page.locator('div.fixed.inset-0.z-40.bg-black\\/50').first();
  if (await overlay.isVisible({ timeout: 500 }).catch(() => false)) {
    await overlay.click();
    await page.waitForTimeout(500);
  }
  // Also try closing via hamburger if overlay didn't work
  const hamburger = page.locator('button:has(svg.lucide-menu)').first();
  if (await hamburger.isVisible({ timeout: 500 }).catch(() => false)) {
    await hamburger.click();
    await page.waitForTimeout(500);
  }
  // Now click the language switcher
  const btn = page.locator('button[aria-label*="language" i], button:has(svg.lucide-languages)').first();
  await btn.click();
  await page.waitForTimeout(500);
});

When("I click the Chinese language option", async ({ page }) => {
  await page.getByText("中文").first().click();
  await page.waitForTimeout(500);
});

When("I click the English language option", async ({ page }) => {
  await page.getByText("English").first().click();
  await page.waitForTimeout(500);
});

When("I reload the page", async ({ page }) => {
  await page.reload();
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1500);
});

Then("the sidebar should show {string} menu item", async ({ page }, text: string) => {
  const hamburger = page.locator('button:has(svg.lucide-menu)').first();
  if (await hamburger.isVisible({ timeout: 1000 }).catch(() => false)) {
    await hamburger.click();
    await page.waitForTimeout(500);
  }
  const sidebar = page.locator("aside").first();
  await expect(sidebar).toBeVisible({ timeout: 5000 });
  await expect(sidebar.getByText(text, { exact: false }).first()).toBeVisible({ timeout: 5000 });
});
