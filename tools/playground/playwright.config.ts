// v0.35 T1 — Playwright smoke tests for the browser playground.
//
// Strategy: vite-preview the production build and drive it via the
// page object. Each spec sets the example via the picker, runs the
// program, and asserts on the rendered output.
//
// The webServer block spins up `vite preview` on port 4173 before any
// test runs; `reuseExistingServer` lets `npm run dev` developers run
// the tests against their already-running dev server.

import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:4173",
    headless: true,
  },
  webServer: {
    command: "npm run preview -- --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173/",
    reuseExistingServer: !process.env.CI,
    timeout: 30_000,
  },
});
