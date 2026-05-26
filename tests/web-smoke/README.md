# Mighty web-smoke (headless-browser visual smoke)

Catches regressions where a wasm artifact passes the byte-level magic-bytes
check but **fails to instantiate in a real browser** — the trap that hid
demo 02's broken state from v0.4 through v0.22. The web demos' existing
`smoke.sh` only validated the component preamble, so any runtime instantiate
failure went unnoticed.

This headless-browser stage runs every web demo through a real Chromium
instance via Playwright, asserting:

1. No `pageerror` events (uncaught JS exceptions during boot).
2. No `console.error` calls (CSP failures, decode errors, etc.).
3. A `<canvas>` element exists with non-zero `width`/`height` and a
   non-zero rendered bounding rect.
4. The canvas actually drew something (not all-transparent, not flat-
   filled).
5. (Optional) The page's 8x8 perceptual hash stays within a hamming
   distance tolerance of the stored golden in `golden/<name>.phash`.

## One-time setup

```bash
cd tests/web-smoke
npm ci                          # installs playwright at the pinned version
npx playwright install chromium # ~135 MiB browser download
```

If those steps are skipped, `node smoke-headless.mjs` exits **0** with
`(headless smoke skipped: playwright unavailable)` — the headless stage
never blocks the existing magic-bytes smoke on dev machines that haven't
opted in.

## Run against a demo

```bash
# build the wasm first
cargo build -p mty-cli
bash demos/05_notetris_web/smoke.sh           # magic-bytes only (default)

# headless stage
MTY_WEB_SMOKE=1 bash demos/05_notetris_web/smoke.sh
MTY_WEB_SMOKE=1 bash demos/02_counter_web/smoke.sh
```

`MTY_WEB_SMOKE_PORT=<n>` overrides the loopback port (defaults: demo 02
uses 8764, demo 05 uses 8765).

## Self-test

The script ships with an internal self-test that asserts a known-good page
PASSES and a deliberately broken page (HTML without the `<script>` tag)
FAILS:

```bash
node tests/web-smoke/smoke-headless.mjs --self-test
```

## CI

`.github/workflows/web-smoke.yml` runs both demo headless smokes on
`workflow_dispatch` (manual trigger only, intentionally not on push to
keep a flaky browser test from blocking regular CI).

To trigger:

1. GitHub → Actions → "web-smoke" → "Run workflow"
2. Or via `gh`: `gh workflow run web-smoke.yml`

## Goldens

The first time a demo's headless smoke runs, `golden/<name>.phash` is
populated with the page's 8x8 perceptual hash. Subsequent runs assert the
new hash stays within a hamming distance of 12 from the stored value.
To re-baseline (e.g. after an intentional visual change), delete the
`.phash` file and re-run.

Goldens checked into git as of v0.23:

- `golden/notetris.phash` (+ `golden/notetris.png` for human triage)

`golden/counter-web.phash` is populated on first CI run (demo 02's
`serve.sh` uses `python3`, which is shadowed by the MS Store launcher on
some Windows hosts, so locally generating the golden requires a real
`python3` on PATH — CI has one).

## Files

- `smoke-headless.mjs` — the Playwright driver (entry point).
- `package.json` — pins `playwright` to a known version.
- `golden/` — committed perceptual hashes + reference images.
- `screenshots/` — gitignored; populated every run for triage.
