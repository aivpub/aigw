import { test as base, Page } from "@playwright/test";
import { mockAllApis } from "./api-mocks";

// Extended test fixtures
export type BddFixtures = {
  adminPage: Page;
  mobilePage: Page;
};

// Auth state that gets passed to steps via world
export type World = {
  page: Page;
  isAdmin: boolean;
};

export const test = base.extend<BddFixtures>({
  adminPage: async ({ page }, use) => {
    await mockAllApis(page, { role: "proxy_admin" });
    await page.goto("/dash/usage");
    await use(page);
  },
  mobilePage: async ({ page }, use) => {
    await page.setViewportSize({ width: 375, height: 667 });
    await mockAllApis(page, { role: "proxy_admin" });
    await page.goto("/dash/usage");
    await use(page);
  },
});

export { expect } from "@playwright/test";
