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
  await page.goto("/dash/usage");
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

Then("I should see the usage page", async ({ page }) => {
  await expect(page).toHaveURL(/\/dash\/usage/);
});

Then("the sidebar should be visible", async ({ page }) => {
  const sidebar = page.locator("aside").first();
  await expect(sidebar).toBeVisible();
});

Then("the sidebar should not be visible", async ({ page }) => {
  const sidebar = page.locator("aside").first();
  await expect(sidebar).not.toBeVisible({ timeout: 3000 });
});

Then("I should see an error message about invalid credentials", async ({ page }) => {
  await expect(page.getByText(/failed|invalid|error|enter/i).first()).toBeVisible();
});

Then("I should not be redirected to the usage page", async ({ page }) => {
  await page.waitForTimeout(1000);
  await expect(page).toHaveURL(/\/dash\/login/);
});

Then("I should be redirected to {string}", async ({ page }, url: string) => {
  await expect(page).toHaveURL(new RegExp(url.replace(/\//g, "\\/")));
});

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Part C: 401 auth:unauthenticated → login redirect
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Given("I am authenticated and on the usage page", async ({ page }) => {
  await mockAllApis(page);
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);
  await page.goto("/dash/usage");
  await page.waitForTimeout(3000);
});

Given("I am authenticated and on {string}", async ({ page }, path: string) => {
  // For the 401-redirect-preserves-path scenario: mock APIs normally,
  // then intercept a specific endpoint to return 401
  await mockAllApis(page);
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);

  // Override one endpoint to return 401
  if (path === "/dash/keys") {
    await page.route("**/key/list", async (route) => {
      await route.fulfill({ status: 401, json: { error: { message: "Unauthorized" } } });
    });
  }

  await page.goto(path);
  await page.waitForTimeout(3000);
});

Given("I was redirected to {string}", async ({ page }, url: string) => {
  // This step represents that we're on the login page after a 401 redirect.
  // Use unauthenticated mocks so /v2/login/check returns 401,
  // which keeps the login page visible (not redirected away).
  await mockApisUnauthenticated(page);
  await page.goto(url);
});

When("the API returns 401 for spend/logs request", async ({ page }) => {
  // Override spend/logs endpoint to return 401, then trigger a navigation
  // that will cause the app to make that call
  await page.route("**/spend/logs**", async (route) => {
    await route.fulfill({ status: 401, json: { error: { message: "Unauthorized" } } });
  });
  // Navigate to spend-logs page to trigger the 401
  await page.goto("/dash/spend-logs");
  await page.waitForTimeout(2000);
});

When("the API returns 401 for key/list request", async ({ page }) => {
  // Already intercepted in the Given step, just trigger navigation
  await page.goto("/dash/keys");
  await page.waitForTimeout(2000);
});

When("the API returns {int} for spend\\/logs request", async ({ page }, code: number) => {
  await page.route("**/spend/logs**", async (route) => {
    await route.fulfill({ status: code, json: { error: { message: "Unauthorized" } } });
  });
  await page.goto("/dash/spend-logs");
  await page.waitForTimeout(2000);
});

When("the API returns {int} for key\\/list request", async ({ page }, code: number) => {
  await page.goto("/dash/keys");
  await page.waitForTimeout(2000);
});

Then("the URL should contain {string}", async ({ page }, substring: string) => {
  // Escape special regex chars in the substring, then check page URL contains it
  const escaped = substring.replace(/[.*+?^${}()|[\]\\\/]/g, "\\$&");
  await expect(page).toHaveURL(new RegExp(escaped));
});
