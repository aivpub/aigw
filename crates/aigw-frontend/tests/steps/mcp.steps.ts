import { createBdd } from "playwright-bdd";
import { expect, type Page } from "@playwright/test";
import {
  mockAllApis,
  MCP_TOKEN_PLAINTEXT,
  getLastMcpServerBody,
  getLastMcpRejectionsQuery,
  hideMcpReloadFromLatest,
} from "./api-mocks";

const { Given, When, Then } = createBdd();

Given("I am on the MCP page", async ({ page }) => {
  await mockAllApis(page);
  await page.goto("/dash/mcp");
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

/** Server rows exist in a desktop table and a mobile card; both carry the same testids. */
function serverRow(page: Page, alias: string) {
  return page
    .locator('[data-server-id]:visible')
    .filter({ hasText: alias })
    .first();
}

function mcpDialog(page: Page) {
  return page.locator('[role="dialog"]');
}

Then("I should see {int} MCP servers in the list", async ({ page }, count: number) => {
  await expect(page.locator('[data-testid="mcp-server-alias"]')).toHaveCount(count * 2, {
    timeout: 8000,
  });
  await expect(page.locator("main")).toContainText("github", { timeout: 5000 });
  await expect(page.locator("main")).toContainText("hr-portal", { timeout: 5000 });
});

Then("the MCP page header has both action buttons", async ({ page }) => {
  await expect(page.getByTestId("mcp-new-server").first()).toBeVisible({ timeout: 5000 });
  await expect(page.getByTestId("mcp-reload-all").first()).toBeVisible({ timeout: 5000 });
});

When("I click {string} on the MCP page", async ({ page }, label: string) => {
  await page.getByTestId("mcp-new-server").click();
  void label;
  await page.waitForTimeout(400);
});

When("I fill MCP alias with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-alias-input").fill(value);
  await page.waitForTimeout(300);
});

When("I fill MCP display name with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-display-name-input").fill(value);
});

When("I fill MCP URL with {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-server-url-input").fill(value);
});

When("I click the save button in the MCP server dialog", async ({ page }) => {
  await mcpDialog(page).getByTestId("mcp-server-save").click();
  await page.waitForTimeout(700);
});

Then("the MCP server dialog closes", async ({ page }) => {
  await expect(mcpDialog(page)).not.toBeVisible({ timeout: 5000 });
});

Then("the new MCP server {string} appears in the list", async ({ page }, alias: string) => {
  await expect(
    page.locator('[data-testid="mcp-server-alias"]:visible').filter({ hasText: alias }).first(),
  ).toBeVisible({ timeout: 8000 });
});

Then("the MCP page has no horizontal page scroll", async ({ page }) => {
  await expectNoHorizontalScroll(page);
});

/** 320px is narrower than any configured project, so the gate resizes. */
When("the MCP page is viewed on a 320px wide screen", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 640 });
  await page.waitForTimeout(300);
});

Then("the MCP tabs and call log filters fit inside the screen", async ({ page }) => {
  for (const testId of ["mcp-tabs", "mcp-calls-filters"]) {
    const overflow = await page
      .getByTestId(testId)
      .first()
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(overflow, `${testId} overflowed by ${overflow}px`).toBeLessThanOrEqual(0);
  }
  await expectNoHorizontalScroll(page);
});

Then("the MCP progress dialog fits inside the screen", async ({ page }) => {
  await expect(progressDialog(page)).toBeVisible({ timeout: 5000 });
  await expectNoHorizontalScroll(page);
});

async function expectNoHorizontalScroll(page: Page) {
  const overflow = await page.evaluate(() => {
    const doc = document.documentElement;
    return doc.scrollWidth - doc.clientWidth;
  });
  expect(overflow).toBeLessThanOrEqual(0);
}

Then("I see an MCP success toast", async ({ page }) => {
  await expect(page.locator("[data-sonner-toast]").first()).toBeVisible({ timeout: 5000 });
});

Then("the MCP alias error is shown", async ({ page }) => {
  await expect(page.getByTestId("mcp-alias-error")).toBeVisible({ timeout: 5000 });
});

Then("the MCP save button is disabled", async ({ page }) => {
  await expect(page.getByTestId("mcp-server-save")).toBeDisabled();
});

Then("the MCP alias error mentions a duplicate", async ({ page }) => {
  await expect(page.getByTestId("mcp-alias-error")).toContainText(
    /duplicate|already registered|已.*注册|重复/i,
    { timeout: 5000 },
  );
});

When("I select the MCP status filter {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-status-filter").selectOption(value);
  await page.waitForTimeout(700);
});

Then("the MCP server {string} is not listed", async ({ page }, alias: string) => {
  await expect(
    page.locator('[data-testid="mcp-server-alias"]:visible').filter({ hasText: alias }),
  ).toHaveCount(0, { timeout: 8000 });
});

When("I click the probe button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-probe").click();
  await page.waitForTimeout(900);
});

Then(
  "the MCP probe dialog shows the protocol version {string}",
  async ({ page }, version: string) => {
    await expect(page.getByTestId("mcp-probe-outcome")).toContainText(version, {
      timeout: 8000,
    });
  },
);

Then("the MCP probe dialog shows {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-probe-tool-count")).toHaveText(String(count), {
    timeout: 8000,
  });
});

Then("the MCP probe dialog shows a conflict block with {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-probe-conflicts")).toBeVisible({ timeout: 8000 });
  await expect(page.getByTestId("mcp-probe-conflict-row")).toHaveCount(count);
});

When(
  "I click the credentials button on the {string} MCP server",
  async ({ page }, alias: string) => {
    await serverRow(page, alias).getByTestId("mcp-server-credentials").click();
    await page.waitForTimeout(800);
  },
);

Then("every MCP token value is redacted", async ({ page }) => {
  const values = page.getByTestId("mcp-token-value");
  await expect(values.first()).toBeVisible({ timeout: 5000 });
  for (const value of await values.allTextContents()) {
    expect(value.trim()).toBe("***");
  }
});

Then("the token plaintext never appears in the page", async ({ page }) => {
  const html = await page.content();
  expect(html).not.toContain(MCP_TOKEN_PLAINTEXT);
});

When("I set the {string} shared token as default", async ({ page }, tokenName: string) => {
  const row = page
    .getByTestId("mcp-token-shared-section")
    .getByTestId("mcp-token-row")
    .filter({ hasText: tokenName })
    .first();
  await row.getByTestId("mcp-token-set-default").click();
  await page.waitForTimeout(800);
});

Then("only {string} carries the default badge", async ({ page }, tokenName: string) => {
  const badged = page
    .getByTestId("mcp-token-shared-section")
    .getByTestId("mcp-token-row")
    .filter({ has: page.getByTestId("mcp-token-default-badge") });
  await expect(badged).toHaveCount(1, { timeout: 5000 });
  await expect(badged.first()).toContainText(tokenName);
});

When("I click the tools button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-tools").click();
  await page.waitForTimeout(800);
});

Then("the MCP tools dialog lists {int} tools", async ({ page }, count: number) => {
  await expect(page.getByTestId("mcp-tool-row")).toHaveCount(count, { timeout: 5000 });
});

When("I click the toggle button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-toggle").click();
  await page.waitForTimeout(800);
});

Then("the MCP server {string} is shown as disabled", async ({ page }, alias: string) => {
  await expect(
    serverRow(page, alias).getByTestId("mcp-status-badge").first(),
  ).toContainText(/停用|Disabled/i, { timeout: 5000 });
});

When("I click the delete button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-delete").click();
  await page.waitForTimeout(800);
});

When("I click the edit button on the {string} MCP server", async ({ page }, alias: string) => {
  await serverRow(page, alias).getByTestId("mcp-server-edit").click();
  await page.waitForTimeout(500);
});

When("I choose the custom auth header in the MCP server dialog", async ({ page }) => {
  await mcpDialog(page).getByTestId("mcp-auth-custom").click();
});

When("I fill MCP auth header name with {string}", async ({ page }, value: string) => {
  await mcpDialog(page).getByTestId("mcp-auth-name-input").fill(value);
});

When("I fill MCP auth header value with {string}", async ({ page }, value: string) => {
  await mcpDialog(page).getByTestId("mcp-auth-value-input").fill(value);
});

When(
  "I add an MCP extra header {string} with value {string}",
  async ({ page }, name: string, value: string) => {
    const dialog = mcpDialog(page);
    await dialog.getByTestId("mcp-extra-header-add").click();
    await dialog.getByTestId("mcp-extra-header-name-0").fill(name);
    await dialog.getByTestId("mcp-extra-header-value-0").fill(value);
  },
);

Then(
  "the last MCP server request names the auth header {string}",
  async ({}, name: string) => {
    expect(getLastMcpServerBody().auth_header_name).toBe(name);
  },
);

Then(
  "the last MCP server request carries the extra header {string}",
  async ({}, name: string) => {
    const headers = getLastMcpServerBody().extra_headers as { name: string }[];
    expect(headers.map((h) => h.name)).toContain(name);
  },
);

Then("the MCP auth name error mentions a reserved header", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-name-error")).toContainText(/gateway|网关/i, {
    timeout: 5000,
  });
});

Then("the MCP auth value is shown masked", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-value-masked")).toHaveText("***");
});

Then("the MCP auth value input is empty", async ({ page }) => {
  await expect(page.getByTestId("mcp-auth-value-input")).toHaveValue("");
});

Then(
  "the last MCP server request does not carry a masked auth value",
  async ({}) => {
    const value = getLastMcpServerBody().auth_header_value;
    expect(value ?? "").not.toBe("***");
  },
);

Then("the typed MCP auth value never appears in the page", async ({ page }) => {
  expect(await page.content()).not.toContain("tapd-secret");
});

// ── Stage 148: the "re-probe all" progress dialog ──

function progressDialog(page: Page) {
  return page
    .locator('[role="dialog"]')
    .filter({ has: page.getByTestId("mcp-progress-bar") });
}

function progressRows(page: Page) {
  return progressDialog(page).getByTestId("mcp-progress-row");
}

When("I click re-probe all on the MCP page", async ({ page }) => {
  await page.getByTestId("mcp-reload-all").first().click();
});

Then("the MCP progress dialog appears", async ({ page }) => {
  await expect(
    page.getByTestId("mcp-progress-bar").first(),
  ).toBeVisible({ timeout: 5000 });
});

Then("the MCP progress bar reports it is not yet complete", async ({ page }) => {
  // Asserting mid-flight is the whole point: with a mock that answered within a
  // frame, "the bar moved" would be indistinguishable from "it is done".
  const bar = page.getByTestId("mcp-progress-bar").first();
  await expect(bar).toBeVisible({ timeout: 5000 });
  const now = Number(await bar.getAttribute("aria-valuenow"));
  const max = Number(await bar.getAttribute("aria-valuemax"));
  expect(max).toBeGreaterThan(0);
  expect(now, "the sweep must still be running on first sight").toBeLessThan(max);
});

Then("the MCP progress dialog shows {int} servers", async ({ page }, count: number) => {
  await expect(progressRows(page)).toHaveCount(count, { timeout: 8000 });
});

Then("the MCP progress dialog finishes with a partial summary", async ({ page }) => {
  await expect(page.getByTestId("mcp-progress-result")).toBeVisible({
    timeout: 15000,
  });
  await expect(page.getByTestId("mcp-progress-result")).toContainText("1");
});

Then(
  "the MCP progress row for {string} is marked {string}",
  async ({ page }, alias: string, state: string) => {
    const row = progressRows(page).filter({ hasText: alias }).first();
    await expect(row).toHaveAttribute("data-state", state, { timeout: 15000 });
  },
);

Then(
  "the MCP progress row for {string} carries an error summary",
  async ({ page }, alias: string) => {
    const row = progressRows(page).filter({ hasText: alias }).first();
    // Asserted on the prose, not the leading status number: the number's
    // origin (upstream vs gateway) is the backend's business and is pinned in
    // `a_failure_row_names_the_upstream_status_once`. Matching "502" here
    // would look like coverage while saying nothing about it.
    await expect(row).toContainText("upstream refused", { timeout: 15000 });
  },
);

Then("the MCP page shows the attach-to-running toast", async ({ page }) => {
  await expect(page.locator("[data-sonner-toast]")).toContainText(
    /already running|in progress/i,
    { timeout: 5000 },
  );
});

Then("no MCP error toast is shown", async ({ page }) => {
  await page.waitForTimeout(600);
  const toasts = page.locator("[data-sonner-toast]");
  const count = await toasts.count();
  for (let i = 0; i < count; i += 1) {
    await expect(toasts.nth(i)).not.toHaveAttribute("data-type", "error");
  }
});

Then(
  "the probe button on the {string} MCP server is disabled",
  async ({ page }, alias: string) => {
    await expect(serverRow(page, alias).getByTestId("mcp-server-probe")).toBeDisabled();
  },
);

Then("I close the MCP progress dialog", async ({ page }) => {
  // Two "Close" affordances: the footer button and the corner ✕. Both dismiss
  // the dialog; `.first()` keeps the locator from tripping strict mode.
  await progressDialog(page)
    .getByRole("button", { name: /close/i })
    .first()
    .click();
  await expect(page.getByTestId("mcp-progress-bar")).toHaveCount(0);
});

When("I click re-probe all again", async ({ page }) => {
  await page.getByTestId("mcp-reload-all").first().click();
});

Then("the MCP page is on the servers tab", async ({ page }) => {
  await page.getByTestId("mcp-tab-servers").first().click();
  await expect(page.getByTestId("mcp-reload-all").first()).toBeVisible({
    timeout: 5000,
  });
});

Then("the latest reload run is hidden from this tab", async ({}) => {
  hideMcpReloadFromLatest();
});

When("I start a probe on the {string} MCP server", async ({ page }, alias: string) => {
  // No settle wait: the point of this scenario is what is true *during* the
  // probe, and the mock holds the response open for that window.
  await serverRow(page, alias).getByTestId("mcp-server-probe").click();
});

// ── Stage 150: auth rejections ──

function rejectionRows(page: Page) {
  return page.locator(
    '[data-testid="mcp-rejection-row"]:visible, [data-testid="mcp-rejection-card"]:visible',
  );
}

When("I open the MCP rejections tab", async ({ page }) => {
  await page.getByTestId("mcp-tab-rejections").first().click();
  await page.waitForTimeout(800);
});

When("I choose the MCP rejection reason {string}", async ({ page }, value: string) => {
  await page.getByTestId("mcp-rejections-reason-filter").selectOption(value);
  await page.waitForTimeout(600);
});

/** Singular and plural are separate step texts in Cucumber expressions, so
 *  the single-row scenarios need their own registration. */
Then("the MCP rejections list shows {int} rows", async ({ page }, count: number) => {
  await expect(rejectionRows(page)).toHaveCount(count, { timeout: 8000 });
});

Then("the MCP rejections list shows {int} row", async ({ page }, count: number) => {
  await expect(rejectionRows(page)).toHaveCount(count, { timeout: 8000 });
});

/** The badge carries the translated label, so the step maps the wire value the
 *  same way the component does rather than asserting the raw code. */
const REJECTION_LABEL: Record<string, string> = {
  missing_header: "Missing header",
  invalid_format: "Invalid format",
  invalid_key: "Invalid key",
};

/** The desktop table and the mobile card each render a badge; at phone width
 *  the table one is in the DOM but hidden, so `.first()` would pick a hidden
 *  element and time out. Only the visible one is the answer. */
Then("the MCP rejections list shows the reason {string}", async ({ page }, reason: string) => {
  const label = REJECTION_LABEL[reason] ?? reason;
  await expect(
    page
      .locator('[data-testid="mcp-rejection-reason"]:visible')
      .filter({ hasText: label })
      .first(),
  ).toBeVisible({ timeout: 8000 });
});

Then(
  "the MCP rejection row for {string} names the client IP {string}",
  async ({ page }, reason: string, ip: string) => {
    const label = REJECTION_LABEL[reason] ?? reason;
    await expect(
      rejectionRows(page).filter({ hasText: label }).filter({ hasText: ip }),
    ).toHaveCount(1, { timeout: 8000 });
  },
);

Then("the MCP rejections request carries reason {string}", async ({ page }, reason: string) => {
  void page;
  await expect
    .poll(() => getLastMcpRejectionsQuery()?.get("reason"), { timeout: 8000 })
    .toBe(reason);
});

Then(
  "the MCP rejection row for {string} shows a shortened token hash",
  async ({ page }, reason: string) => {
    const row = rejectionRows(page).filter({ hasText: REJECTION_LABEL[reason] ?? reason }).first();
    const text = (await row.textContent()) ?? "";
    expect(text).toMatch(/[0-9a-f]{12}…/);
    expect(text).not.toContain("9f2c1d8e4b7a5036c9d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3");
  },
);

Then("the MCP rejection row for {string} shows no token hash", async ({ page }, reason: string) => {
  const row = rejectionRows(page).filter({ hasText: REJECTION_LABEL[reason] ?? reason }).first();
  expect((await row.textContent()) ?? "").not.toMatch(/[0-9a-f]{12}…/);
});

Then("the MCP tabs and rejection filters fit inside the screen", async ({ page }) => {
  for (const testId of ["mcp-tabs", "mcp-rejections-filters"]) {
    const overflow = await page
      .getByTestId(testId)
      .first()
      .evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(overflow, `${testId} overflowed by ${overflow}px`).toBeLessThanOrEqual(0);
  }
  await expectNoHorizontalScroll(page);
});

// ── Stage 151: each tab is a page route ──

/** A path with a query is compared whole — that is the shareable part. */
function expectLocation(page: Page, expected: string) {
  return expect
    .poll(
      () => {
        const url = new URL(page.url());
        return expected.includes("?")
          ? `${url.pathname}${url.search}`
          : url.pathname;
      },
      { timeout: 8000 },
    )
    .toBe(expected);
}

Then("the browser is at {string}", async ({ page }, path: string) => {
  await expectLocation(page, path);
});

Then("the browser is still at {string}", async ({ page }, path: string) => {
  await expectLocation(page, path);
});

When("I open the URL {string}", async ({ page }, path: string) => {
  await page.goto(path);
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(1000);
});

When("I click the MCP tab {string}", async ({ page }, tab: string) => {
  await page.getByTestId(`mcp-tab-${tab}`).first().click();
  await page.waitForTimeout(800);
});

When("I go back in the browser", async ({ page }) => {
  await page.goBack();
  await page.waitForTimeout(800);
});
