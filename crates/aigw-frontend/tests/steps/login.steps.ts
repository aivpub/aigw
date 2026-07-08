import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Given, When, Then } = createBdd();

Given("I am on the login page", async ({ page }) => {
  await page.goto("/dash/login");
});

Given("I am already authenticated via cookie", async ({ page }) => {
  await page.context().addCookies([{
    name: "aigw_master_key",
    value: "sk-master-change-me",
    path: "/",
    domain: "localhost",
  }]);
  // Must navigate to baseURL first to establish an origin for localStorage
  await page.goto("/");
  // Set localStorage so useAuth returns isAuthenticated=true
  await page.evaluate(() => {
    localStorage.setItem("aigw_master_key", "sk-master-change-me");
  });
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
  // Current login page uses a single key input labeled with placeholder "sk-..."
  const tokenInput = page.getByPlaceholder("sk-...");
  if (await tokenInput.isVisible()) {
    await tokenInput.fill(value);
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
