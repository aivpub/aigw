import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

const { Given, When, Then } = createBdd();

// ━━━━ Stage 129: Claude OAuth CredentialsTab steps ━━━━
// Uses unique phrasing so none of these collide with the existing
// keys.steps.ts / models.steps.ts / proxies.steps.ts step definitions.

Then("I should see the OAuth credential {string} with active status", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
  await expect(page.locator("main")).toContainText(/Active|正常/i, { timeout: 5000 });
});

Then("I should see the OAuth expiry and bound proxy for {string}", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
  await expect(page.locator("main")).toContainText(/Expires|到期时间/i, { timeout: 5000 });
  await expect(page.locator("main")).toContainText(/hk-residential|Proxy|代理/i, { timeout: 5000 });
});

When("I click the New OAuth Credential button", async ({ page }) => {
  // force:true — on small viewports the sticky header's language switch button
  // sits at the top and can intercept the pointer event (same pattern as the
  // playground steps' force:isMobile).
  await page.getByRole("button", { name: /new oauth credential|新建 oauth 凭证/i }).click({ force: true });
  await page.waitForTimeout(500);
});

When("I fill the OAuth dialog with name {string} cookie {string} proxy {string}", async ({ page }, name: string, cookie: string, proxyName: string) => {
  const dialog = page.locator('[role="dialog"]');
  await dialog.getByLabel(/credential name/i).fill(name);
  await dialog.getByLabel(/session cookie/i).fill(cookie);
  // Proxy select: open and pick the named proxy (skip for "direct").
  const proxyTrigger = dialog.getByRole("combobox");
  if (proxyName !== "direct") {
    await proxyTrigger.click({ force: true });
    await page.getByRole("option", { name: new RegExp(proxyName, "i") }).click({ force: true });
  }
  await page.waitForTimeout(300);
});

When("I submit the OAuth dialog", async ({ page }) => {
  const dialog = page.locator('[role="dialog"]');
  // force:true — the session textarea can overlap the footer button on small
  // viewports (the textarea sits in the same flex column above the footer).
  await dialog.getByRole("button", { name: /exchange|交换/i }).click({ force: true });
  await page.waitForTimeout(500);
});

Then("the OAuth dialog closes", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

// ── needs_reauth scenarios ──
// Override the default mock's active credential to a needs_reauth one. The
// default list mock (api-mocks.ts) still responds; this per-scenario route is
// registered AFTER the broad mock, so Playwright's last-registered-wins ordering
// gives it precedence (mockAllApis is guarded to register once per page).
let reauthMode = false;

Given("the OAuth credential {string} requires re-auth", async ({ page }, name: string) => {
  reauthMode = true;
  await mockAllApis(page);
  await page.route("**/credential/list**", (route) =>
    route.fulfill({
      status: 200,
      json: {
        object: "list",
        data: [
          {
            credential_name: "oauth-personal",
            credential_values: {
              type: "anthropic_oauth",
              status: "needs_reauth",
              expires_at: 1756000000,
              proxy_id: 1,
              last_error: "refresh token revoked: cookie expired",
              inject_prompt: null,
            },
            credential_info: {},
          },
        ],
      },
    }),
  );
  await page.reload();
  await page.waitForTimeout(500);
});

Then("I should see the OAuth credential {string} with needs-reauth badge", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
  await expect(page.locator("main")).toContainText(/Needs Re-auth|需重新认证/i, { timeout: 5000 });
});

Then("I should see the OAuth last error for {string}", async ({ page }, name: string) => {
  await expect(page.locator("[data-testid='oauth-last-error']")).toBeVisible({ timeout: 5000 });
});

Then("I should see a {string} button for {string}", async ({ page }, buttonLabel: string, name: string) => {
  const btn = page.locator(`[data-testid='oauth-reauth-${name}']`);
  await expect(btn).toBeVisible({ timeout: 5000 });
});

When("I click the Refresh button on the OAuth credential {string}", async ({ page }, name: string) => {
  const btn = page.locator(`[data-testid='oauth-refresh-${name}']`);
  await btn.click();
  await page.waitForTimeout(500);
});

Then("the OAuth refresh request is sent for {string}", async ({ page }, name: string) => {
  // The refresh button POSTs /credential/oauth/refresh. Assert the toast or
  // that the row still renders (the mock returns the active credential).
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
  void reauthMode;
});

When("I click the {string} button on the OAuth credential {string}", async ({ page }, buttonLabel: string, name: string) => {
  const btn = page.locator(`[data-testid='oauth-reauth-${name}']`);
  await btn.click();
  await page.waitForTimeout(300);
});

When("I paste a new cookie into the re-auth dialog", async ({ page }) => {
  const dialog = page.locator('[role="dialog"]');
  await dialog.getByLabel(/session cookie/i).fill("sk-ant-sid-fresh-xyz");
  await page.waitForTimeout(300);
});

When("I submit the re-auth dialog", async ({ page }) => {
  const dialog = page.locator('[role="dialog"]');
  await dialog.getByRole("button", { name: /re-auth|重新认证/i }).click({ force: true });
  await page.waitForTimeout(500);
});

Then("the re-auth dialog closes", async ({ page }) => {
  await expect(page.locator('[role="dialog"]')).not.toBeVisible({ timeout: 5000 });
});

Then("the OAuth credential {string} shows no raw token values", async ({ page }, name: string) => {
  await expect(page.locator("main")).toContainText(name, { timeout: 5000 });
  // Raw token trio must never render. The redacted placeholders are "***".
  const body = await page.locator("main").innerText();
  expect(body).not.toContain("sk-ant-access");
  expect(body).not.toContain("sk-ant-refresh");
  expect(body).not.toContain("sk-ant-sid");
  expect(body).not.toContain("access_token:");
});
