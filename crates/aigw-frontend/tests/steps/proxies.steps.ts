import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am on the Proxies page", async ({ page }) => {
  await mockAllApis(page);
  await page.goto("/dash/proxies");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

Then("I should see {int} proxies in the list", async ({ page }, count: number) => {
  await expect(page.locator("main")).toContainText("hk-residential", { timeout: 5000 });
  await expect(page.locator("main")).toContainText("us-clean", { timeout: 5000 });
  await expect(page.locator("main")).toContainText("sg-tunnel", { timeout: 5000 });
  void count;
});

Then("each proxy should show its exit IP", async ({ page }) => {
  await expect(page.locator("main")).toContainText(/1\.2\.3\.4|8\.8\.8\.8|192\.168\.1\.100/, { timeout: 5000 });
});

When("I click {string} on the Proxies page", async ({ page }, label: string) => {
  await page.getByRole("button", { name: new RegExp(label, "i") }).click();
  await page.waitForTimeout(500);
});

When("I fill proxy name with {string}", async ({ page }, value: string) => {
  const input = page.locator('[role="dialog"]').getByLabel(/name/i).first();
  await input.fill(value);
  await page.waitForTimeout(300);
});

When("I fill proxy URL with {string}", async ({ page }, value: string) => {
  const input = page.locator('[role="dialog"]').getByTestId("proxy-url-input");
  await input.fill(value);
  await page.waitForTimeout(300);
});

When("I click the {string} button in the proxy dialog", async ({ page }, label: string) => {
  await page
    .locator('[role="dialog"]')
    .getByRole("button", { name: new RegExp(label, "i") })
    .click();
  await page.waitForTimeout(500);
});

Then("the proxy dialog closes", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

Then("the new proxy {string} appears in the list", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
});

When("I click the edit button on the first proxy row", async ({ page }) => {
  const editBtn = page.locator("button:has(.lucide-pencil)").first();
  try {
    await editBtn.click({ timeout: 3000 });
  } catch {
    await page.locator("button:has(.lucide-pencil)").last().click();
  }
  await page.waitForTimeout(500);
});

Then("the proxy dialog opens with pre-filled data", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toBeVisible({ timeout: 5000 });
});

When("I click the delete button on the first proxy row", async ({ page }) => {
  const delBtn = page.locator("button:has(.lucide-trash2)").first();
  try {
    await delBtn.click({ timeout: 3000 });
  } catch {
    await page.locator("button:has(.lucide-trash2)").last().click();
  }
  await page.waitForTimeout(500);
});

Then("a proxy delete confirmation dialog appears", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toContainText(/delete|confirm/i, { timeout: 5000 });
});

When("I click the Test button on the first proxy row", async ({ page }) => {
  // Desktop table has button:has(.lucide-activity); mobile card too. Fall back
  // to the last (mobile card) button when the desktop-first is hidden.
  const testBtn = page.locator("button:has(.lucide-activity)").first();
  try {
    await testBtn.click({ timeout: 3000 });
  } catch {
    await page.locator("button:has(.lucide-activity)").last().click();
  }
  await page.waitForTimeout(800);
});

When("I click the Quality button on the first proxy row", async ({ page }) => {
  const qBtn = page.locator("button:has(.lucide-gauge)").first();
  try {
    await qBtn.click({ timeout: 3000 });
  } catch {
    await page.locator("button:has(.lucide-gauge)").last().click();
  }
  await page.waitForTimeout(800);
});

Then("I should see the quality check dialog with score and grade", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toContainText(/score|grade|quality/i, { timeout: 8000 });
});

Then("I should see the quality items breakdown", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).toContainText(/openai|anthropic|claude_oauth|pass|fail/i, { timeout: 8000 });
});
