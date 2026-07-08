import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis, mockApisUnauthenticated } from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am on the login page", async ({ page }) => {
  // Mock APIs in unauthenticated mode so /v2/login/check returns 401
  await mockApisUnauthenticated(page);
  await page.goto("/dash/login");
});

Given("I am already authenticated via cookie", async ({ page }) => {
  // Use full mocks (authenticated) — no Background override in Already-authenticated scenario
  await mockAllApis(page);
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);
  // Auth is cookie-based — the cookie above + mock for /v2/login/check handles it
  await page.goto("/dash/home");
  // Wait for React auth check to resolve and dashboard to render
  await page.waitForTimeout(3000);
});

When("I type {string} into the username field", async ({ page }, value: string) => {
  const input = page.getByPlaceholder(/username|user/i).first();
  if (await input.isVisible()) {
    await input.fill(value);
  } else {
    // fallback: current login page uses single key input
    const keyInput = page.getByPlaceholder(/sk-|master key/i).first();
    if (await keyInput.isVisible()) {
      await keyInput.fill(value);
    }
  }
});

When("I type {string} into the password field", async ({ page }, value: string) => {
  // Login page password input has placeholder "Password"
  const pwdInput = page.getByPlaceholder(/password/i).first();
  if (await pwdInput.isVisible({ timeout: 2000 }).catch(() => false)) {
    await pwdInput.fill(value);
  }
});

When("I click the Sign In button", async ({ page }) => {
  await page.getByRole("button", { name: /sign in|verifying/i }).click();
});

When("I click the Sign In button without entering credentials", async ({ page }) => {
  // Button is disabled when input is empty, use force click to trigger validation
  await page.getByRole("button", { name: /sign in/i }).click({ force: true });
});

Then("I should see the dashboard home page", async ({ page }) => {
  await expect(page).toHaveURL(/\/dash\/home/);
});

Then("the sidebar should be visible", async ({ page }) => {
  const sidebar = page.locator("aside").first();
  await expect(sidebar).toBeVisible();
});

Then("I should see an error message about invalid credentials", async ({ page }) => {
  await expect(page.getByText(/failed|invalid|error|enter/i).first()).toBeVisible();
});

Then("I should not be redirected to the home page", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page).toHaveURL(/\/dash\/login/);
});

Then("I should be redirected to {string}", async ({ page }, url: string) => {
  await expect(page).toHaveURL(new RegExp(url.replace(/\//g, "\\/")));
});
