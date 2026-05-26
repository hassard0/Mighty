# Web smoke v0.23 (Track E) — headless-browser visual smoke for web demos

**Status:** shipped. Headless smoke runs on demo 05 today, gated demo 02
behind `MTY_WEB_SMOKE=1`, manual-trigger CI workflow added.

## Why

Every web demo's `smoke.sh` only validated the wasm artifact's *bytes* —
component preamble + size + a known import string. That's why demo
`02_counter_web` was "passing" smoke since v0.4 while emitting a
component the browser couldn't actually instantiate. The artifact was
shaped like a Component but its embedded core module's imports didn't
match what the page's loader supplied, so `WebAssembly.instantiate` trapped
and the page silently displayed nothing. The CI never knew.

This slice adds a **headless Chromium** smoke stage that opens the demo's
real `index.html`, instantiates the wasm, and asserts:

1. **No `pageerror`** — uncaught exceptions during boot crash the demo.
2. **No `console.error`** — CSP/decode/404 failures surface here.
3. **A `<canvas>` exists** with non-zero width/height + non-zero rendered
   bounding rect.
4. **The canvas actually drew something** — sample the canvas via
   `getImageData`, require at least one opaque pixel AND at least two
   distinct pixel values (catches "all-transparent" and "flat-fill"
   blank states).
5. **Perceptual-hash regression check** — 8x8 average-hash compared with
   the stored `golden/<name>.phash` under a hamming distance tolerance
   of 12 (very forgiving — catches "totally different page" while still
   tolerating font-rendering jitter and animation phase).

## Owned files (Track E)

- `tests/web-smoke/smoke-headless.mjs` — Playwright driver (self-contained,
  no extra deps beyond `playwright`).
- `tests/web-smoke/package.json` — pins `playwright@1.45.3`.
- `tests/web-smoke/README.md` — user-facing how-to.
- `tests/web-smoke/golden/notetris.phash` — first-run-populated phash for
  demo 05. `.png` reference image alongside for human triage.
- `tests/web-smoke/.gitignore` — keeps `node_modules/` + `screenshots/`
  out of git, plus self-test artifacts.
- `.github/workflows/web-smoke.yml` — manual-trigger-only CI.
- `dev/history/notes/WEB_SMOKE_V0_23_NOTES.md` — this file.

Appended (opt-in stage only) to:

- `demos/02_counter_web/smoke.sh`
- `demos/05_notetris_web/smoke.sh`

When `MTY_WEB_SMOKE` is unset (default), both scripts behave exactly as
before — strict additive change.

## How the stage plugs in

The appended block in each demo's `smoke.sh` is a no-op unless
`MTY_WEB_SMOKE=1`. When set:

1. Run the existing magic-bytes check (unchanged).
2. Boot `web/serve.sh` in the background on a configurable port
   (`MTY_WEB_SMOKE_PORT`, defaults: demo 02 → 8764, demo 05 → 8765).
3. Wait up to 10 s for the loopback server to answer.
4. Invoke `node tests/web-smoke/smoke-headless.mjs $URL $NAME`.
5. Kill the server via a bash `trap` regardless of outcome.
6. Propagate the headless stage's exit code.

If `node` isn't on `PATH`, the stage prints
`(headless smoke skipped: node not on PATH)` and exits 0 — same shape
as the existing "playwright not installed" path inside the script.

## Self-test

`node tests/web-smoke/smoke-headless.mjs --self-test` spins up two
ephemeral loopback servers serving inline HTML fixtures:

- **good**: `<canvas>` + a `<script>` that fills two coloured rects →
  expected PASS.
- **broken**: same `<canvas>` element with the `<script>` deleted →
  expected FAIL because the canvas stays fully transparent.

Both expectations must hold for the self-test to pass (exit 0).

```
$ node tests/web-smoke/smoke-headless.mjs --self-test
[web-smoke] self-test: starting
[web-smoke] PASS [__selftest_good] canvas={"w":80,"h":80,...} drew=true phash=007e7e7e7e7e7e00
[web-smoke] self-test: good fixture PASSED as expected
[web-smoke] FAIL [__selftest_broken]: canvas is fully transparent (nothing drawn)
[web-smoke] self-test: broken fixture FAILED as expected (...)
[web-smoke] self-test: ALL PASS
```

## CI

`.github/workflows/web-smoke.yml` is `workflow_dispatch`-only on purpose:

- Browser tests are inherently flakier than unit tests.
- A flake here should not block merges.
- The user opts in via the Actions UI:
  ```
  gh workflow run web-smoke.yml
  gh workflow run web-smoke.yml -f demo=05
  ```
- On every run, screenshots are uploaded as an artifact for triage.

When the slice matures (low flake rate over ~10 runs), promote to
`on: push` or wire it into the `release` workflow as a pre-tag gate.

## Verified locally

```
$ node tests/web-smoke/smoke-headless.mjs --self-test
... self-test: ALL PASS

$ MTY_WEB_SMOKE=1 MTY_WEB_SMOKE_PORT=8801 bash demos/05_notetris_web/smoke.sh
smoke OK: .../target/main.wasm (2125 bytes, component magic verified)
[web-smoke] golden phash distance for "notetris" = 0 (tol 12)
[web-smoke] PASS [notetris] canvas={"w":240,"h":480,...} drew=true phash=0038000000000000
05_notetris_web: PASS (headless-browser smoke + magic bytes)

$ bash demos/05_notetris_web/smoke.sh   # default — no env var
smoke OK: .../target/main.wasm (2125 bytes, component magic verified)
next: bash demos/05_notetris_web/web/serve.sh   # opens http://localhost:8000
```

Demo 02 not verified locally on this Windows host (its `serve.sh` shells
to `python3`, which is shadowed by the MS Store launcher when no real
`python3` is on PATH). The headless stage is wired identically and will
exercise on CI's Ubuntu runner, where `python3` is real. If demo 02 ends
up needing the same `python`/`python3`/`py` fallback demo 05's serve.sh
already has, that's a Track A/B cleanup for the next slice — Track E
stays additive.

## Phash details

- 8x8 average hash. The `<canvas>` is drawn into an 8x8 offscreen via
  `drawImage(canvas, 0, 0, 8, 8)`, then `getImageData` gives 256 RGBA
  bytes. Each pixel's luminance is compared to the mean; bit = 1 if
  above. The 64-bit hash is serialized as 16 hex chars.
- Hamming-distance tolerance is **12** out of 64. Empirically, demo 05's
  current page hashes to `0038000000000000` (top-third lit, rest dark —
  the spawn tetromino on a dark board). Even a slight piece-position
  shift comes in at 4–6, well inside tolerance. A wholesale layout
  change blows past 12 immediately.
- To re-baseline after an intentional visual change, delete
  `tests/web-smoke/golden/<name>.phash` and re-run.

## Future work (out of v0.23 scope)

- Click `+1` on demo 02 and assert the count goes up (interactive smoke).
- Send a keypress to demo 05 and assert the piece moves.
- Wire `web-smoke.yml` into the release workflow as a gate once flake
  history confirms stability.
- Patch demo 02's `web/serve.sh` to mirror demo 05's `python`/`python3`/
  `py` fallback (out of this track's strict file ownership).
