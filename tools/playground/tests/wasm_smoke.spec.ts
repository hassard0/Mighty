// v0.35 T1 — Playground WASM smoke tests.
//
// Boots the production build under `vite preview` (see
// `playwright.config.ts`) and exercises the same path a human user
// would: pick an example from the dropdown, click Run, read the
// stdout / diagnostics tabs.
//
// What this verifies:
//
//   - The wasm-pack artifact loads from `/wasm/mty_cli.js` (not the
//     mock fallback). We assert the backend mode pill reads "wasm".
//   - 01_hello_agent runs end-to-end: parse + type-check pass and the
//     `log("hello, Mighty")` reaches stdout.
//   - 05_taint_safety still flags MT4099 against the real compiler —
//     this is the v0.33 T4 fix-envelope path; we just check the code
//     surface, not the structured fixes (those land in T4).
//   - Every bundled gallery example (02-07) type-checks cleanly OR
//     emits a structured diagnostic the renderer can format. We don't
//     allow the runner to throw.
//
// If the wasm artifact didn't ship, the runner falls back to the mock;
// `expects(wasm)` would fail, surfacing the regression.

import { expect, test } from "@playwright/test";

test.describe("playground wasm backend", () => {
  test("loads the real wasm runner (not the mock)", async ({ page }) => {
    await page.goto("/");
    // The pill in the toolbar displays "mock" or "wasm" after init.
    await expect(page.locator("#backend-mode")).toHaveText("wasm", {
      timeout: 15_000,
    });
  });

  test("01_hello_agent runs end-to-end via wasm", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("#backend-mode")).toHaveText("wasm", {
      timeout: 15_000,
    });
    await page.locator("#example-picker").selectOption("01_hello_agent");
    await page.locator("#btn-run").click();
    // Status pill flips to "ok" once the run completes cleanly.
    await expect(page.locator("#status-pill")).toHaveAttribute(
      "data-state",
      "ok",
      { timeout: 10_000 },
    );
    // stdout tab should contain "hello, Mighty".
    await page.locator('.output-tabs__tab[data-tab="stdout"]').click();
    await expect(page.locator("#output-stdout")).toContainText("hello, Mighty");
  });

  test("05_taint_safety surfaces MT4099 via wasm check", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("#backend-mode")).toHaveText("wasm", {
      timeout: 15_000,
    });
    await page.locator("#example-picker").selectOption("05_taint_safety");
    await page.locator("#btn-check").click();
    await page.locator('.output-tabs__tab[data-tab="diagnostics"]').click();
    // The taint-flow diagnostic is MT4099. The status pill flips to
    // "error" once the check completes.
    await expect(page.locator("#status-pill")).toHaveAttribute(
      "data-state",
      "error",
      { timeout: 10_000 },
    );
    await expect(page.locator("#output-diagnostics")).toContainText("MT4099", {
      timeout: 5_000,
    });
  });

  test("every gallery example checks without throwing", async ({ page }) => {
    await page.goto("/");
    await expect(page.locator("#backend-mode")).toHaveText("wasm", {
      timeout: 15_000,
    });
    const ids = [
      "01_hello_agent",
      "02_tool_calling",
      "03_swarm_review",
      "04_eval_suite",
      "05_taint_safety",
      "06_observability",
      "07_computer_use",
    ];
    for (const id of ids) {
      await page.locator("#example-picker").selectOption(id);
      await page.locator("#btn-check").click();
      // Status pill always flips to ok|error|idle; "runner failure"
      // would mean the wasm threw, which is what we want to catch.
      await expect(page.locator("#status-pill")).not.toHaveText(
        "runner failure",
        { timeout: 10_000 },
      );
    }
  });
});
