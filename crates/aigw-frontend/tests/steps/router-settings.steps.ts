//! Step bindings for router-settings.feature — Stage 118 dropdown unlock

import { createBdd } from "playwright-bdd";
import { expect } from "@playwright/test";

const { Given, When, Then } = createBdd();

Given("I am on the Router Settings page", async ({ page }) => {
  await page.goto("/dash/router-settings");
  // Wait for the global tab form to load its router settings.
  await page.waitForURL("**/router-settings");
});

When("I open the routing strategy dropdown", async ({ page }) => {
  // Radix Select trigger renders as role="combobox" (ARIA 1.2). Older versions
  // of @radix-ui/react-select emitted `aria-haspopup="listbox"` on a plain
  // <button>; the current version drops that attribute, so a
  // `button[aria-haspopup="listbox"]` selector never matches. Query by the
  // accessible role + label instead — resilient to future ARIA changes.
  await page.getByRole("combobox", { name: "Strategy" }).first().click();
});

Then("I should see {string} as an enabled option", async ({ page }, label: string) => {
  const option = page.getByRole("option", { name: label });
  await option.waitFor({ state: "visible", timeout: 5000 });
  const disabled = await option.getAttribute("aria-disabled");
  expect(disabled).not.toBe("true");
});

When("I select {string} as the routing strategy", async ({ page }, label: string) => {
  await page.getByRole("option", { name: label }).click();
});

When("I click the save button", async ({ page }) => {
  await page.getByRole("button", { name: /save/i }).last().click();
});

Then("a success toast should appear for the global router settings", async ({ page }) => {
  await page.getByText(/saved|updated/i).first().waitFor({ state: "visible", timeout: 5000 });
});
