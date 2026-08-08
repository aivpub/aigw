import { defineConfig, devices } from "@playwright/test";
import { defineBddConfig } from "playwright-bdd";

const testDir = defineBddConfig({
  paths: ["tests/features/*.feature"],
  require: ["tests/steps/*.ts"],
  disableWarnings: { importTestFrom: true },
});

export default defineConfig({
  testDir,
  timeout: 30000,
  expect: { timeout: 5000 },
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 1,
  // Local: 5 workers; CI (single-core runners) stays at 1. Workers are bounded
  // by project count (3 viewports), so 5 means up to 5 concurrent pages per run.
  workers: process.env.CI ? 1 : 5,
  reporter: [
    ["html", { outputFolder: "tests/output/html-report" }],
    ["list"],
  ],
  use: {
    baseURL: "http://localhost:5173",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    video: "retain-on-failure",
  },
  outputDir: "tests/output/test-results/",
  projects: [
    {
      name: "chromium-desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 720 } },
    },
    {
      name: "chromium-mobile",
      use: { ...devices["iPhone SE"], defaultBrowserType: "chromium" },
    },
    {
      name: "chromium-tablet",
      use: { ...devices["iPad Mini"], defaultBrowserType: "chromium" },
    },
  ],
  webServer: {
    command: "npx vite --port 5173 --strictPort",
    cwd: ".",
    port: 5173,
    reuseExistingServer: !process.env.CI,
    timeout: 30000,
  },
});
