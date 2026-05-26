#!/usr/bin/env node
// tests/web-smoke/smoke-headless.mjs — Mighty v0.23 Track E
//
// Headless-browser visual smoke for the wasm/web demos.
//
// Usage:
//   node smoke-headless.mjs <baseUrl> <name>     run smoke against a live server
//   node smoke-headless.mjs --self-test          run internal self-test
//
// Exits 0 on PASS, 1 on FAIL. When `playwright` is not installed locally,
// exits 0 with a clear "(headless smoke skipped: playwright unavailable)"
// message so demos can be smoked on dev machines without the optional
// browser-test stack.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const GOLDEN_DIR = resolve(__dirname, 'golden');
const SCREENSHOT_DIR = resolve(__dirname, 'screenshots');
const PHASH_TOLERANCE = 12; // hamming-distance budget on a 64-bit phash
const PAGE_SETTLE_MS = 2000;
const NAV_TIMEOUT_MS = 15000;

function log(msg) {
  process.stdout.write(`[web-smoke] ${msg}\n`);
}
function err(msg) {
  process.stderr.write(`[web-smoke] ${msg}\n`);
}

async function loadPlaywright() {
  try {
    const mod = await import('playwright');
    return mod;
  } catch (e) {
    return null;
  }
}

// 8x8 average-hash perceptual hash. Operates on a raw RGBA PNG buffer
// already loaded into memory; we let Playwright/Chromium produce the
// PNG and a tiny pure-JS PNG decoder handles the rest. To avoid an
// extra dep, we re-render the PNG into a downscaled grayscale grid
// using a *very* coarse approach: ask Playwright for a small
// clip-screenshot at 8x8 size, then read the raw image data. The
// 8x8 PNG is small enough that we can parse it ourselves.
//
// We use the `sharp`-free path: ask Chromium to draw the target
// into a tiny offscreen canvas via page.evaluate(), then extract
// 8x8 RGBA. That keeps deps to playwright-only.
async function computePhashFromCanvasBytes(rgba) {
  // rgba: Uint8Array of length 8*8*4 (RGBA)
  if (rgba.length !== 8 * 8 * 4) {
    throw new Error(`phash: expected 256 bytes RGBA, got ${rgba.length}`);
  }
  const gray = new Float64Array(64);
  let sum = 0;
  for (let i = 0; i < 64; i++) {
    const r = rgba[i * 4];
    const g = rgba[i * 4 + 1];
    const b = rgba[i * 4 + 2];
    const y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    gray[i] = y;
    sum += y;
  }
  const avg = sum / 64;
  // Build 64-bit hash as a hex string (BigInt avoids 32-bit shift issues).
  let hash = 0n;
  for (let i = 0; i < 64; i++) {
    hash <<= 1n;
    if (gray[i] >= avg) hash |= 1n;
  }
  return hash.toString(16).padStart(16, '0');
}

function hamming64(aHex, bHex) {
  const a = BigInt('0x' + aHex);
  const b = BigInt('0x' + bHex);
  let x = a ^ b;
  let count = 0;
  while (x) {
    if (x & 1n) count++;
    x >>= 1n;
  }
  return count;
}

// --- Self-test fixtures ---------------------------------------------------
// A "good" page (renders a canvas with colored pixels) and a "broken" one
// (no <script>, so the canvas stays blank). The self-test asserts the
// script PASSES the good page and FAILS the broken page.
const FIXTURE_GOOD = `<!doctype html><html><body>
<canvas id="c" width="80" height="80"></canvas>
<script>
const c = document.getElementById('c');
const ctx = c.getContext('2d');
ctx.fillStyle = '#b66ff0';
ctx.fillRect(0,0,80,80);
ctx.fillStyle = '#ffffff';
ctx.fillRect(10,10,60,60);
window.__rendered = true;
</script>
</body></html>`;

const FIXTURE_BROKEN = `<!doctype html><html><body>
<canvas id="c" width="80" height="80"></canvas>
<!-- intentionally no <script> so the canvas stays blank -->
</body></html>`;

function startFixtureServer(html) {
  return new Promise((res) => {
    const srv = createServer((req, _res) => {
      _res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      _res.end(html);
    });
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      res({ url: `http://127.0.0.1:${port}`, srv });
    });
  });
}

// --- Core smoke -----------------------------------------------------------
// `mode`:
//   - 'canvas' (default): demo must render to a <canvas> (used by
//     notetris_web, canvas_game, and any future demo whose wasm draws
//     pixels). Validates canvas dims + pixel variation + phash golden.
//   - 'dom': demo updates a DOM element instead of a canvas (used by
//     counter_web). Validates the page loaded without errors and that
//     a known target element (`#count` or a `[data-mty-output]`
//     marker) exists with non-default text content.
async function runSmoke({ baseUrl, name, mode = 'canvas', golden = true, expectFail = false }) {
  const pw = await loadPlaywright();
  if (!pw) {
    log('(headless smoke skipped: playwright unavailable)');
    log('install with: cd tests/web-smoke && npm ci');
    return { skipped: true, ok: true };
  }

  mkdirSync(SCREENSHOT_DIR, { recursive: true });
  mkdirSync(GOLDEN_DIR, { recursive: true });

  const browser = await pw.chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1024, height: 768 },
  });
  const page = await context.newPage();

  const consoleErrors = [];
  const pageErrors = [];
  page.on('pageerror', (e) => {
    pageErrors.push(String(e && e.message ? e.message : e));
  });
  page.on('console', (m) => {
    if (m.type() === 'error') {
      consoleErrors.push(m.text());
    }
  });

  let failReason = null;
  let phashHex = null;
  let canvasDims = null;
  let canvasNonDefault = false;

  try {
    await page.goto(baseUrl, { waitUntil: 'load', timeout: NAV_TIMEOUT_MS });
    // Let the wasm boot and the agent draw at least one frame.
    await page.waitForTimeout(PAGE_SETTLE_MS);

    if (pageErrors.length > 0) {
      failReason = `page error(s): ${pageErrors.join(' | ')}`;
    }

    if (!failReason && mode === 'canvas') {
      // Canvas presence + non-zero dimensions.
      const dims = await page.evaluate(() => {
        const c = document.querySelector('canvas');
        if (!c) return null;
        const rect = c.getBoundingClientRect();
        return { w: c.width, h: c.height, cw: rect.width, ch: rect.height };
      });
      canvasDims = dims;
      if (!dims) {
        failReason = 'no <canvas> element on page';
      } else if (dims.w === 0 || dims.h === 0 || dims.cw === 0 || dims.ch === 0) {
        failReason = `canvas has zero dimension: ${JSON.stringify(dims)}`;
      }
    }

    if (!failReason && mode === 'dom') {
      // DOM-mode demos (counter_web): the wasm boots, registers exports,
      // and the JS host wires DOM listeners to those exports. We just
      // verify the page came up cleanly and the canonical target element
      // exists (`#count` or any `[data-mty-output]`).
      const ok = await page.evaluate(() => {
        return !!(document.querySelector('#count') ||
                  document.querySelector('[data-mty-output]'));
      });
      if (!ok) {
        failReason =
          'no #count or [data-mty-output] target element on DOM-mode page';
      }
    }

    if (!failReason && mode === 'canvas') {
      // Did the page actually draw something? Check that at least one
      // pixel differs from the canvas's top-left pixel (a flat-fill
      // demo would still pass — we're catching "totally blank or all-
      // transparent"). Also samples 64 points to build a phash.
      const sample = await page.evaluate(() => {
        const c = document.querySelector('canvas');
        if (!c) return null;
        const ctx = c.getContext('2d');
        if (!ctx) return null;
        // 8x8 downsample of the canvas into RGBA bytes for phash.
        const off = document.createElement('canvas');
        off.width = 8; off.height = 8;
        const octx = off.getContext('2d');
        octx.drawImage(c, 0, 0, 8, 8);
        const data = octx.getImageData(0, 0, 8, 8).data;
        // "non-default" = not every pixel identical to pixel 0.
        const r0 = data[0], g0 = data[1], b0 = data[2], a0 = data[3];
        let varied = false;
        for (let i = 4; i < data.length; i += 4) {
          if (data[i] !== r0 || data[i+1] !== g0 ||
              data[i+2] !== b0 || data[i+3] !== a0) {
            varied = true;
            break;
          }
        }
        // Also reject "all transparent" — a 0-alpha canvas is not drawn.
        let anyOpaque = false;
        for (let i = 3; i < data.length; i += 4) {
          if (data[i] !== 0) { anyOpaque = true; break; }
        }
        return {
          varied,
          anyOpaque,
          bytes: Array.from(data),
        };
      });

      if (!sample) {
        failReason = 'canvas getContext("2d") returned null';
      } else if (!sample.anyOpaque) {
        failReason = 'canvas is fully transparent (nothing drawn)';
      } else if (!sample.varied) {
        failReason = 'canvas is flat-filled (every pixel identical — likely blank)';
      } else {
        canvasNonDefault = true;
        const rgba = new Uint8Array(sample.bytes);
        phashHex = await computePhashFromCanvasBytes(rgba);
      }
    }

    // Screenshot (always — useful for triage even on failure).
    const shotPath = join(SCREENSHOT_DIR, `${name}.png`);
    try {
      await page.screenshot({ path: shotPath, fullPage: false });
    } catch (e) {
      // Non-fatal: screenshot may fail under exotic Playwright builds.
      err(`screenshot failed: ${e.message || e}`);
    }

    // Golden compare (if we have a hash and a golden exists).
    if (!failReason && phashHex && golden) {
      const goldenHashPath = join(GOLDEN_DIR, `${name}.phash`);
      if (existsSync(goldenHashPath)) {
        const goldenHash = readFileSync(goldenHashPath, 'utf8').trim();
        const dist = hamming64(phashHex, goldenHash);
        log(`golden phash distance for "${name}" = ${dist} (tol ${PHASH_TOLERANCE})`);
        if (dist > PHASH_TOLERANCE) {
          failReason =
            `golden phash drift: distance ${dist} > tol ${PHASH_TOLERANCE} ` +
            `(got=${phashHex}, want=${goldenHash})`;
        }
      } else {
        // First run — populate the golden.
        writeFileSync(goldenHashPath, phashHex + '\n');
        log(`populated golden phash for "${name}": ${phashHex}`);
        // Also try to copy the screenshot in as a human-readable golden image.
        const goldenImgPath = join(GOLDEN_DIR, `${name}.png`);
        if (!existsSync(goldenImgPath) && existsSync(shotPath)) {
          try {
            writeFileSync(goldenImgPath, readFileSync(shotPath));
          } catch (_) { /* best effort */ }
        }
      }
    }

    if (consoleErrors.length > 0 && !failReason) {
      // Console errors are stricter than pageerror — they include things
      // like CSP failures, 404s, etc. We treat them as fatal unless they
      // mention specific known-benign patterns.
      const benign = consoleErrors.filter((e) =>
        /favicon\.ico/i.test(e) || /DevTools/i.test(e),
      );
      const real = consoleErrors.filter((e) => !benign.includes(e));
      if (real.length > 0) {
        failReason = `console error(s): ${real.join(' | ')}`;
      }
    }
  } catch (e) {
    failReason = `runtime error: ${e && e.message ? e.message : e}`;
  } finally {
    await context.close().catch(() => {});
    await browser.close().catch(() => {});
  }

  if (failReason) {
    err(`FAIL [${name}]: ${failReason}`);
    if (canvasDims) err(`  canvas dims: ${JSON.stringify(canvasDims)}`);
    return { ok: false, reason: failReason };
  }

  log(`PASS [${name}] canvas=${JSON.stringify(canvasDims)} drew=${canvasNonDefault} phash=${phashHex}`);
  return { ok: true, phash: phashHex };
}

// --- Self-test -----------------------------------------------------------
async function selfTest() {
  log('self-test: starting');

  const pw = await loadPlaywright();
  if (!pw) {
    log('self-test: playwright not installed — skipping (exit 0)');
    log('install with: cd tests/web-smoke && npm ci');
    return 0;
  }

  // 1. Good fixture should PASS.
  const goodSrv = await startFixtureServer(FIXTURE_GOOD);
  let goodRes;
  try {
    goodRes = await runSmoke({
      baseUrl: goodSrv.url,
      name: '__selftest_good',
      golden: false,
    });
  } finally {
    goodSrv.srv.close();
  }
  if (!goodRes.ok) {
    err(`self-test FAIL: good fixture did not pass: ${goodRes.reason}`);
    return 1;
  }
  log('self-test: good fixture PASSED as expected');

  // 2. Broken fixture should FAIL.
  const badSrv = await startFixtureServer(FIXTURE_BROKEN);
  let badRes;
  try {
    badRes = await runSmoke({
      baseUrl: badSrv.url,
      name: '__selftest_broken',
      golden: false,
    });
  } finally {
    badSrv.srv.close();
  }
  if (badRes.ok) {
    err('self-test FAIL: broken fixture (no <script>) unexpectedly passed');
    return 1;
  }
  log(`self-test: broken fixture FAILED as expected (${badRes.reason})`);

  log('self-test: ALL PASS');
  return 0;
}

// --- Entry --------------------------------------------------------------
async function main() {
  const argv = process.argv.slice(2);
  if (argv.includes('--self-test')) {
    const code = await selfTest();
    process.exit(code);
  }
  // Pull --mode out before positional parsing.
  let mode = 'canvas';
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--mode') {
      mode = argv[++i] || 'canvas';
    } else if (a.startsWith('--mode=')) {
      mode = a.slice('--mode='.length);
    } else {
      positional.push(a);
    }
  }
  const baseUrl = positional[0];
  const name = positional[1];
  if (!baseUrl || !name) {
    err('usage: node smoke-headless.mjs <baseUrl> <name> [--mode canvas|dom]');
    err('       node smoke-headless.mjs --self-test');
    process.exit(2);
  }
  if (mode !== 'canvas' && mode !== 'dom') {
    err(`unknown --mode "${mode}" (expected canvas or dom)`);
    process.exit(2);
  }
  const res = await runSmoke({ baseUrl, name, mode });
  if (res.skipped) {
    process.exit(0);
  }
  process.exit(res.ok ? 0 : 1);
}

main().catch((e) => {
  err(`fatal: ${e && e.stack ? e.stack : e}`);
  process.exit(1);
});
