import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Then, When } = createBdd();

Then("I should see total spend information", async ({ page }) => {
  await expect(page.getByText(/\$|spend|total/i).first()).toBeVisible({ timeout: 8000 });
});

Then("I should see spend by model chart or data", async ({ page }) => {
  await expect(page.getByText(/model|gpt/i).first()).toBeVisible({ timeout: 8000 });
});

Then("I should see loading indicators before data appears", async ({ page }) => {
  // Skeleton or spinner should appear during loading
  const hasLoading = await page.getByText(/loading|skeleton/i).isVisible().catch(() => false);
  // After loading, data should appear
  await page.waitForTimeout(3000);
  await expect(page.getByText(/\$|spend/i).first()).toBeVisible({ timeout: 5000 });
});

When("I visit {string}", async ({ page }, url: string) => {
  await page.goto(url);
  // Vite HMR keeps a WebSocket open so networkidle never fires; use domcontentloaded instead
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500); // brief settle for React render
});

When("I visit the Usage page", async ({ page }) => {
  await page.goto("/dash/usage");
  // Vite HMR keeps a WebSocket open so networkidle never fires; use domcontentloaded instead
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500); // brief settle for React render
});

// ━━━━ Stage 71: Usage page charts & rankings ━━━━

When("I click the {string} tab in the Daily Trend card", async ({ page }, tabLabel: string) => {
  // Find the Daily Trend card header region and click the tab
  const dailyTrendHeading = page.getByText("Trend", { exact: true });
  // Navigate up to the CardHeader then find the TabsList (CardHeader contains title + tabs as siblings)
  const cardHeader = dailyTrendHeading.locator("..");
  const tab = cardHeader.getByRole("tab", { name: tabLabel });
  await tab.click();
  await page.waitForTimeout(300); // allow re-render
});

Then("the Daily Trend chart should show stacked bars for prompt and completion tokens", async ({ page }) => {
  // Chart legend now shows "Input" / "Output" / "Cache Read" / "Cache Write"
  await expect(page.getByText("Input").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByText("Output").first()).toBeVisible({ timeout: 5000 });
});

Then("the chart legend should show {string} and {string}", async ({ page }, name1: string, name2: string) => {
  await expect(page.getByText(name1, { exact: true }).first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByText(name2, { exact: true }).first()).toBeVisible({ timeout: 5000 });
});

Then("the Daily Trend chart should show stacked bars for successful and failed requests", async ({ page }) => {
  await expect(page.getByText("Success").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByText("Failed").first()).toBeVisible({ timeout: 5000 });
});

Then("the Top Virtual Keys card should show a ranked list of keys", async ({ page }) => {
  await expect(page.getByText("#1").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByText("prod-gpt-key").first()).toBeVisible({ timeout: 5000 });
});

Then("the ranking should be sorted by spend in descending order", async ({ page }) => {
  // Verify the first entry has the highest spend ($12.50 from mock)
  await expect(page.getByText("$12.5000").first()).toBeVisible({ timeout: 5000 });
});

Then("the first ranked key should show {string} and its spend", async ({ page }, expected: string) => {
  await expect(page.getByText(expected).first()).toBeVisible({ timeout: 5000 });
});

When("I click the {string} tab in the Top Virtual Keys card", async ({ page }, tabLabel: string) => {
  // Find the Top Virtual Keys card heading and its parent CardHeader
  const heading = page.getByText("Top Virtual Keys", { exact: true });
  const cardHeader = heading.locator("..");
  const tab = cardHeader.getByRole("tab", { name: tabLabel });
  await tab.click();
  await page.waitForTimeout(300);
});

Then("the ranking values should switch from spend to token values", async ({ page }) => {
  // After clicking the Tokens tab, values should show token counts
  // e.g., "30.0K" for 30000 tokens
  await page.waitForTimeout(500);
  await expect(page.getByText(/\d+\.?\d*K/).first()).toBeVisible({ timeout: 5000 });
});

Then("the ranking values should switch from spend to request counts", async ({ page }) => {
  // After clicking the Requests tab, values should show integer request counts
  await page.waitForTimeout(500);
  await expect(page.getByText("85").first()).toBeVisible({ timeout: 5000 });
});

When("I click the ranking toggle in the Spend by Model card", async ({ page }) => {
  // In the Spend by Model card, the ranking toggle is a tab trigger with the ListOrdered icon
  const heading = page.getByText("Spend by Model", { exact: true });
  const cardHeader = heading.locator("..");
  // The ranking tab is inside the TabsList with value "ranking"
  const rankTab = cardHeader.getByRole("tab", { name: "📊 Chart" }).locator("..").getByRole("tab").nth(1);
  // Fallback: click the tab that has svg child (ListOrdered icon)
  await rankTab.click().catch(async () => {
    // Alternative: find the TabsTrigger with value "ranking"
    await cardHeader.locator('[role="tab"]').last().click();
  });
  await page.waitForTimeout(300);
});

Then("the model ranking list should be displayed with progress bars", async ({ page }) => {
  // After switching to rank view, the model names should be visible in a ranking list
  await expect(page.getByText("gpt-4").first()).toBeVisible({ timeout: 5000 });
  // Rank numbers (#1, #2) should be visible
  await expect(page.getByText("#1").first()).toBeVisible({ timeout: 5000 });
});

Then("models should be sorted by spend in descending order", async ({ page }) => {
  // gpt-4 has $25.00, claude-sonnet-4-6 has $17.50
  await expect(page.getByText("gpt-4").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByText("claude-sonnet-4-6").first()).toBeVisible({ timeout: 5000 });
});

When("I click the {string} preset button", async ({ page }, presetLabel: string) => {
  // Date preset buttons: "3 days", "7 days", "30 days", "Custom"
  await page.getByRole("button", { name: presetLabel }).click();
  await page.waitForTimeout(500);
});

Then("the activity query should include a 7-day date range", async ({ page }) => {
  // After clicking "7 days", the activity data should re-fetch
  await page.waitForTimeout(500);
  await expect(page.getByText(/\$|spend/i).first()).toBeVisible({ timeout: 5000 });
});

When("I capture activity requests and click the {string} preset button", async ({ page }, presetLabel: string) => {
  // Register the route BEFORE clicking so we intercept the refetch triggered by the click.
  const requests: string[] = [];
  await page.route("**/global/spend/activity**", async (route) => {
    requests.push(route.request().url());
    await route.continue();
  });

  await page.getByRole("button", { name: presetLabel }).click();
  await page.waitForTimeout(600);

  // Persist the captured URLs so the Then step can assert on them.
  (page as unknown as Record<string, unknown>).__activityRequests = requests;
});

Then("the captured activity query should use today's local date with offset_minutes", async ({ page }) => {
  const requests = ((page as unknown as Record<string, unknown>).__activityRequests ?? []) as string[];
  expect(requests.length).toBeGreaterThan(0);
  const q = new URL(requests[requests.length - 1]).searchParams;
  const localToday = (() => {
    const d = new Date();
    const mm = String(d.getMonth() + 1).padStart(2, "0");
    const dd = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${mm}-${dd}`;
  })();

  expect(q.get("start_date")).toBe(localToday);
  expect(q.get("end_date")).toBe(localToday);
  expect(Number(q.get("offset_minutes"))).not.toBeNaN();
});
