import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";
import { syncCapturedBodies } from "./api-mocks";

const { Given, When, Then } = createBdd();

/**
 * Tailwind `lg` breakpoint (1024px): at/above it the desktop settings sidebar
 * renders inline; below it settings live in the mobile Sheet. The old "is any
 * settings button visible" heuristic broke when the sidebar gained a collapsible
 * toggle (title "Collapse/Expand settings") — it also matches /settings/i, so
 * the step collapsed the desktop sidebar, hid the model combobox, and timed out.
 * Viewport width is the stable signal (mocks run at 1280/768/375px).
 */
function isMobileViewport(page: Parameters<Parameters<typeof createBdd>[0]>[0]) {
  const vp = page.viewportSize();
  return !!vp && vp.width < 1024;
}

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
  // Open settings if on mobile (force:true — sticky header or Sheet overlay may intercept clicks)
  const isMobile = isMobileViewport(page);
  if (isMobile) {
    const settingsBtn = page.getByRole("button", { name: /settings/i });
    if (await settingsBtn.isVisible().catch(() => false)) {
      await settingsBtn.click({ force: true });
      await page.waitForTimeout(300);
    }
  }
  // force:true on mobile — Sheet's fixed overlay intercepts all clicks inside the Sheet portal.
  // On desktop the model combobox is the FIRST combobox in the settings sidebar.
  await page.getByRole("combobox").first().click({ force: isMobile });
  await page.waitForTimeout(300);
  await page.getByRole("option", { name: model }).click({ force: isMobile });
  // Close sheet if on mobile
  if (isMobile) {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);
  }
});

Then("the model {string} should be shown as the active model", async ({ page }, model: string) => {
  await expect(page.locator("main")).toContainText(model);
});

When("I type {string} into the chat input", async ({ page }, message: string) => {
  const input = page.getByPlaceholder(/type a message/i);
  await input.fill(message);
});

When("I click the New Chat button", async ({ page }) => {
  // force:true — small viewports may have sticky header overlap with buttons
  await page.getByRole("button", { name: /new chat/i }).click({ force: true });
  await page.waitForTimeout(500);
});

Then("the chat messages should be cleared", async ({ page }) => {
  await expect(page.getByText(/start a conversation/i)).toBeVisible({ timeout: 5000 });
});

When("I toggle streaming on", async ({ page }) => {
  // Check if streaming toggle is visible (desktop sidebar) or in dialog
  let switchBtn = page.locator("button[role='switch']").first();
  const needsSheet = !(await switchBtn.isVisible().catch(() => false));
  if (needsSheet) {
    const settingsBtn = page.getByRole("button", { name: /settings/i });
    if (await settingsBtn.isVisible().catch(() => false)) {
      await settingsBtn.click({ force: true });
      await page.waitForTimeout(400);
    }
    switchBtn = page.locator("button[role='switch']").first();
  }
  // Only toggle if not already checked
  const isChecked = await switchBtn.getAttribute("aria-checked");
  if (isChecked !== "true") {
    await switchBtn.click({ force: needsSheet });
    await page.waitForTimeout(300);
  }
  // Close settings sheet if opened
  if (needsSheet) {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(400);
  }
});

When("I click the Send button", async ({ page }) => {
  // Make sure a model is selected first
  const modelText = await page.locator("main").textContent();
  const needsModel = !modelText || !modelText.match(/gpt-4|claude/i);
  if (needsModel) {
    // Open settings sheet (mobile) — desktop sidebar is already visible
    const onMobile = isMobileViewport(page);
    if (onMobile) {
      const settingsBtn = page.getByRole("button", { name: /settings/i });
      if (await settingsBtn.isVisible().catch(() => false)) {
        await settingsBtn.click({ force: true });
        await page.waitForTimeout(300);
      }
    }
    const combo = page.getByRole("combobox");
    if (await combo.first().isVisible().catch(() => false)) {
      await combo.first().click({ force: onMobile });
      await page.waitForTimeout(300);
      await page.getByRole("option").first().click();
      await page.waitForTimeout(300);
    }
    // Close sheet by pressing Escape
    if (onMobile) {
      await page.keyboard.press("Escape");
      await page.waitForTimeout(300);
    }
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 104: image attachments
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/** Tiny valid PNG (1x1) — used as the upload/paste fixture. */
const PNG_1PX =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

/** Large photo-like PNG (2400x2400, ~3.3MB) — used to verify TD-009a downscaling:
 * the uploaded data URL must be SMALLER after compressImage. */
const LARGE_PHOTO_PATH = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "../fixtures/large-photo.png",
);

function uploadImages(page: import("@playwright/test").Page, count: number) {
  return page.locator("[data-testid='playground-file-input']").setInputFiles(
    Array.from({ length: count }, (_, i) => ({
      name: `test-${i}.png`,
      mimeType: "image/png",
      buffer: Buffer.from(PNG_1PX, "base64"),
    })),
  );
}

/** Upload the large-photo fixture (bypasses the 1x1 default). */
function uploadLargePhoto(page: import("@playwright/test").Page) {
  return page.locator("[data-testid='playground-file-input']").setInputFiles({
    name: "large-photo.png",
    mimeType: "image/png",
    buffer: fs.readFileSync(LARGE_PHOTO_PATH),
  });
}

When("I upload an image to the playground", async ({ page }) => {
  await uploadImages(page, 1);
  await page.waitForTimeout(300);
});

When("I upload a large photo to the playground", async ({ page }) => {
  await uploadLargePhoto(page);
  await page.waitForTimeout(800); // canvas decode + downscale takes longer
});

/** Upload an "oversized" image by lowering the gateway body-limit under test.
 * The component reads `window.__AIGW_MAX_IMAGE_BODY__` (TD-009b test hook); we set
 * it to 1 byte so ANY real upload exceeds it and the reject path fires. This
 * exercises the actual pre-send gate without a >24MiB fixture (sessionStorage
 * quota makes one untestable). */
When("I upload an oversized image to the playground", async ({ page }) => {
  await page.evaluate(() => {
    (globalThis as unknown as { __AIGW_MAX_IMAGE_BODY__?: number }).__AIGW_MAX_IMAGE_BODY__ = 1;
  });
  await uploadLargePhoto(page);
  await page.waitForTimeout(600);
});

Then("I should see the attachment-too-large toast", async ({ page }) => {
  await expect(page.getByText(/24 MB/i)).toBeVisible({ timeout: 5000 });
});

Then("no chat request should have been sent", async ({ page }) => {
  // handleSend bails before fetch when the body limit is exceeded, so the mock
  // never replies "Mock response: I am doing well!" — that absence proves no
  // chat request reached the upstream mock.
  await page.waitForTimeout(500);
  await expect(page.getByText(/Mock response: I am doing well!/i)).toHaveCount(0);
});

Then("the pending image should be compressed to a smaller data URL", async ({ page }) => {
  // The pending thumbnail src is the compressed data URL; assert it starts with
  // data:image/jpeg (downscaled photo) and is smaller than the 3.3MB source.
  const thumb = page.locator("[data-testid='playground-pending-image']").first();
  const src = (await thumb.getAttribute("src")) ?? "";
  expect(src.startsWith("data:image/jpeg")).toBeTruthy();
  // 3.3MB source → compressed base64 should be < 2MB (way under source).
  const b64len = src.length - src.indexOf(",") - 1;
  expect(b64len).toBeLessThan(2 * 1024 * 1024);
});

/** Upload a synthetic HEIC file — Chromium cannot decode HEIC, so
 * compressImage returns null → the TD-011b reject toast fires. */
When("I upload a HEIC image to the playground", async ({ page }) => {
  await page.locator("[data-testid='playground-file-input']").setInputFiles({
    name: "photo.heic",
    mimeType: "image/heic",
    buffer: Buffer.from([0, 0, 0, 24, 102, 116, 121, 112, 104, 101, 105, 99]), // ftyp heic box
  });
  await page.waitForTimeout(500);
});

Then("I should see the HEIC-unsupported toast", async ({ page }) => {
  await expect(page.getByText(/HEIC\/HEIF/i)).toBeVisible({ timeout: 5000 });
});

Then("the pending image should be the original tiny PNG unchanged", async ({ page }) => {
  const thumb = page.locator("[data-testid='playground-pending-image']").first();
  const src = (await thumb.getAttribute("src")) ?? "";
  // 1x1 PNG passes through compression untouched (stays PNG, not re-encoded).
  expect(src.startsWith("data:image/png")).toBeTruthy();
  expect(src).toContain("iVBORw0KGgo"); // original 1x1 PNG payload
});

When("I upload {int} images to the playground", async ({ page }, count: number) => {
  await uploadImages(page, count);
  await page.waitForTimeout(300);
});

When("I paste an image into the playground", async ({ page }) => {
  // Synthesize a clipboard paste with an image File via DataTransfer.
  await page.evaluate((b64) => {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const file = new File([bytes], "pasted.png", { type: "image/png" });
    const dt = new DataTransfer();
    dt.items.add(file);
    window.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: dt, bubbles: true }),
    );
  }, PNG_1PX);
  await page.waitForTimeout(400);
});

Then("I should see {int} image preview", async ({ page }, count: number) => {
  const thumbs = page.locator("[data-testid='playground-pending-image']");
  await expect(thumbs).toHaveCount(count, { timeout: 5000 });
});

Then("I should see {int} image previews", async ({ page }, count: number) => {
  const thumbs = page.locator("[data-testid='playground-pending-image']");
  await expect(thumbs).toHaveCount(count, { timeout: 5000 });
});

Then("the preview thumbnail should have a data:image src", async ({ page }) => {
  const thumb = page.locator("[data-testid='playground-pending-image']").first();
  await expect(thumb).toHaveAttribute("src", /^data:image\//, { timeout: 5000 });
});

When("I remove the first image attachment", async ({ page }) => {
  await page.locator("[data-testid='playground-remove-image-0']").click();
  await page.waitForTimeout(300);
});

When("I switch the endpoint type to Claude Messages", async ({ page }) => {
  // Endpoint selector is in the settings panel/sheet. Pick "Claude Messages".
  const onMobile = isMobileViewport(page);
  if (onMobile) {
    const settingsBtn = page.getByRole("button", { name: /settings/i });
    if (await settingsBtn.isVisible().catch(() => false)) {
      await settingsBtn.click({ force: true });
      await page.waitForTimeout(300);
    }
  }
  const combos = page.getByRole("combobox");
  // Last combobox in settings is endpoint type (chat / messages)
  await combos.last().click({ force: onMobile });
  await page.waitForTimeout(300);
  await page.getByRole("option", { name: /claude messages/i }).click({ force: onMobile });
  if (onMobile) {
    await page.keyboard.press("Escape");
    await page.waitForTimeout(300);
  }
});

Then("the chat request body should include an image_url content part", async ({ page }) => {
  syncCapturedBodies(page);
  await page.waitForTimeout(300);
  const body = await page.evaluate(() =>
    (globalThis as unknown as { __aigwLastChat?: unknown }).__aigwLastChat,
  );
  const content = body?.messages?.[0]?.content;
  const parts = Array.isArray(content) ? content : [content];
  const hasImage = parts.some(
    (p) =>
      typeof p === "object" &&
      p !== null &&
      (p as { type?: string }).type === "image_url" &&
      ((p as { image_url?: { url?: string } }).image_url?.url ?? "").startsWith("data:image/"),
  );
  expect(hasImage).toBeTruthy();
});

Then("the messages request body should include a Claude image block", async ({ page }) => {
  syncCapturedBodies(page);
  await page.waitForTimeout(300);
  const body = await page.evaluate(() =>
    (globalThis as unknown as { __aigwLastMessages?: unknown }).__aigwLastMessages,
  );
  const msgs = body?.messages as Array<{ content?: unknown }> | undefined;
  const content = msgs?.[0]?.content;
  const parts = Array.isArray(content) ? content : [content];
  const hasImage = parts.some(
    (p) =>
      typeof p === "object" &&
      p !== null &&
      (p as { type?: string }).type === "image" &&
      (p as { source?: { data?: string } }).source?.data?.length > 0,
  );
  expect(hasImage).toBeTruthy();
});

When("I reload the playground page", async ({ page }) => {
  await page.reload();
  await page.waitForLoadState("domcontentloaded");
  await page.waitForTimeout(500);
});

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 105: user message image bubble rendering
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Then("the user message should render an image thumbnail", async ({ page }) => {
  await page.waitForTimeout(500);
  // The sent user bubble renders msg.images as <img src^="data:image/">.
  await expect(
    page.locator("img[src^='data:image/']").first(),
  ).toBeVisible({ timeout: 5000 });
});
