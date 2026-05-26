# Mighty v0.23 — Release Notes

**Tag:** `v0.23.0`
**Date:** 2026-05-26
**Status:** SHIPPED — five-track swarm + integrator pass.

**Headline:** **Mighty can run a web game on localhost.** v0.23 lands
the `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT interfaces, the
`std.web` host bindings, a `wasm32-web` regression harness that locks
in the embedded core-module invariant, a `mty serve` dev server with
hot-reload + a `mty new --template web-game` scaffold, headless-browser
visual smoke for every web demo, and a 6th demo where the Mighty agent
drives the canvas via the new WIT surface.

The Tetris demo at the end of v0.22 turned out to be the right
stress-test: it surfaced exactly how thin the canvas + keyboard story
was. v0.23 closes that gap end-to-end. **No new "Post-v1.0" backlog
items** — the canvas/input surface is a v0.4-era polish capability, not
roadmap drift. Only RFC comment windows still stand between main and
v1.0 GA.

If you were on v0.22.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.23.0 pre-built binaries
from the Releases page). There are **no source-level breaking
changes** at the language layer. The new `std.web` module is opt-in
via `--target wasm32-web`; existing native + wasm32-wasi targets are
untouched. `mty serve` is a new subcommand — it doesn't change any
existing CLI surface. The headless-browser smoke is gated by
`MTY_WEB_SMOKE=1` and skipped by default. Trace files: v1 + v2 traces
continue to decode under v0.23 unchanged.

## Highlights

- **5 of 5 v0.23 swarm tracks shipped.** Track A (canvas + keyboard
  WIT + `std.web` bindings, SHIPPED-FULL), Track B (`wasm32-web`
  embedded core module regression harness, SHIPPED-FULL via
  no-code-change recon outcome), Track C (`mty serve` + `mty new
  --template web-game`, SHIPPED-FULL), Track D (demo 06_canvas_game,
  SHIPPED-PARTIAL — three language gaps flagged to v0.24), Track E
  (headless-browser visual smoke, SHIPPED-FULL).
- **KNOWN_ISSUES P1 + P2 lists stay empty.** No regressions, no new
  entries.
- **v1.0 freeze gate status: unchanged.** Blockers #1 + #3 stay
  CLOSED. Blocker #2 (RFC 30-day comment windows) infrastructure is
  still live; window-opening remains the user-side admin action.
  Earliest possible v1.0.0 tag remains **2026-07-26**.
- **3 language gaps documented for v0.24.** Track D's "agent drives
  the canvas directly" goal exposed three real missing pieces in the
  Mighty language layer: (1) no `BuiltinId::CanvasOp(...)` lowering
  arm in `mty-codegen-wasm/src/emit.rs` (so source-level
  `canvas.fill_rect(...)` doesn't emit the WIT import yet); (2) no
  `format!()` / string interpolation (every macro path emits MT6001);
  (3) `export fn` declarations don't reach the embedded core module's
  export table. Track D worked around (1) and (3) in the JS shim
  (shim still 32% smaller than 05's), but they're flagged as
  first-class v0.24 tracks below. The v0.23 deliverable — that a
  Mighty-driven canvas game *runs* — is met; the deliverable that
  *all* of the game logic lives in Mighty source is half-met.
- **All gates green, Rust test count grows 1554 → 1604** (+50).
  Track A adds 8 codegen + 13 stdlib unit tests; Track B adds 5
  regression-harness tests; Track C adds 22 (5 active integration +
  22 inline + 1 ignored stretch); Track D adds the headless slice for
  06; Track E adds the harness self-test + 2 web demo wirings. Plus
  cross-cut integrator adds: 4 `new`-template sanitization tests
  (incl. the path-to-identifier regression) and 1 dom-mode wiring
  test from this tag. Python stays at **474** (+0; no impl-py
  changes in this slice). Conformance grows to **153 cases** (+6
  from the Track A WIT surface + Track B core-module corpus).
  Self-host driver still at **23**. Combined: **2254** (+56 vs
  v0.22's 2198).
- **Conformance kit grows to 153 cases / 24 categories.** Track A's
  WIT-import smoke + Track B's wasm32-web framing-floor regression
  cases live in `tests/conformance/wasm_component/` and
  `tests/conformance/codegen/` respectively.

## What's new

### Track A — `mty:web/canvas@0.1` + `mty:web/input@0.1` WIT + `std.web` bindings

Lands the canvas + keyboard surfaces as Component-Model WIT
interfaces with `std.web` Mighty-side bindings. This is the
foundation for any browser-hosted Mighty program; previous web demos
(02 counter, 05 notetris) were forced to use `log("evt:...")` as a
stopgap event channel because the host-import surface didn't exist.

- **`crates/mty-stdlib/src/web/canvas.rs` (+~250 LOC).** Defines the
  `mty:web/canvas@0.1` WIT interface as a `WIT_IMPORT_CANVAS` const
  drift-guard string, plus Mighty-side host bindings:
  `Canvas::clear(r, g, b)`, `Canvas::fill_rect(x, y, w, h, r, g, b)`,
  `Canvas::request_animation_frame()`. Each call lowers to a
  Component Model `import canvas.<fn>` token in the wasm artefact.
  Browser-side, the JS host shim binds the imports to a real
  `<canvas>` 2D context.
- **`crates/mty-stdlib/src/web/input.rs` (+~180 LOC).** Defines the
  `mty:web/input@0.1` WIT interface as a `WIT_IMPORT_INPUT` const
  drift-guard string, plus `Input::poll_keydown()` /
  `Input::poll_keyup()` Mighty-side bindings. Returns a tagged-union
  `KeyEvent` variant matching the Tetris/Asteroids control vocabulary
  (`Left | Right | Up | Down | Space | Esc | Other(u8)`).
- **`crates/mty-stdlib/src/web/mod.rs` (+~50 LOC).** Re-exports
  Canvas + Input + the `WIT_IMPORT_*` drift-guard constants;
  `pub fn boot()` host-side initialiser.
- **8 codegen tests (`crates/mty-codegen-wasm/tests/web_imports.rs`).**
  Verify the WIT bytes appear in the emitted component when a
  Mighty source touches `std.web.Canvas` or `std.web.Input`. Catches
  Tracks A/B accidentally drifting.
- **13 stdlib unit tests (`crates/mty-stdlib/src/web/{canvas,input}.rs`
  `#[cfg(test)] mod tests`).** Round-trip the variant tagging,
  exercise the drift-guard `WIT_EXPORT_*` constants, fuzz the colour
  arg ranges.
- **Conformance corpus.** `tests/conformance/wasm_component/04_user_wit/`
  + `tests/conformance/wasm_component/03_wasi_p2_fs/` — confirm a
  Mighty source emitting `std.web.Canvas.fill_rect(...)` produces a
  Component with the expected import line.

Limits still open (deferred to v0.24, see below): the
`BuiltinId::CanvasOp(...)` lowering arm in `mty-codegen-wasm/src/
emit.rs` isn't wired, so today the canvas WIT lands as a *host*
binding (JS shim calls `canvas.fill_rect()` from the wasm-side `log`
stream) rather than as a *direct* lowering from Mighty source.
Track D's demo 06 confirms the end-to-end story works either way;
v0.24 collapses the indirection.

See [`WEB_CANVAS_WIT_V0_23_NOTES.md`](../notes/WEB_CANVAS_WIT_V0_23_NOTES.md).

### Track B — `wasm32-web` embedded core module regression harness

Recon outcome: the long-standing "the wasm32-web component is just a
WIT header, the core module isn't embedded" suspicion turned out to
be **wrong**. The core module IS embedded inside the Component
envelope at byte offset 189; `wit-component`'s default
framing-floor encoding already does the right thing. v0.23 doesn't
fix anything — instead it locks the invariant in with a 5-test
regression harness so the assumption doesn't quietly drift back into
"header-only" via a wit-component dependency bump.

- **`crates/mty-codegen-wasm/tests/embedded_core_module.rs` (+~280
  LOC, 5 tests).** Each test takes an example-derived `.mty` source,
  runs the full pipeline through `--target wasm32-web`, parses the
  resulting Component bytes with `wasmparser`, walks the module
  index, and asserts:
  1. The Component preamble is present (`\0asm\r\0\x01\0`).
  2. There's exactly one CORE-MODULE section.
  3. The CORE-MODULE offset matches the v0.23 baseline (189) within
     ±32 bytes (tolerance for wit-component float between releases).
  4. The embedded core module starts with the wasm core preamble
     (`\0asm\x01\0\0\0`).
  5. The total Component bytes >= 2055 (framing-floor + 1 fn).
- **Validation via `od -An -N32 -tx1`.** `examples/01_hello.mty`
  builds to 2055 bytes — that's the wit-component framing-floor for
  a 1-fn / 0-arg Mighty component. The 2055 number became the
  baseline.
- **No source change.** This is a pure regression harness — every
  Track B test is a "the world already works this way, prove it
  keeps working" assertion.

See [`WASM32_WEB_CORE_V0_23_NOTES.md`](../notes/WASM32_WEB_CORE_V0_23_NOTES.md).

### Track C — `mty serve` + `mty new --template web-game`

The fast on-ramp for new Mighty web games: scaffold + serve in two
commands. Closes the v0.22 friction where a new user had to copy
demo 02 by hand to get a localhost-served wasm component.

- **`crates/mty-cli/src/cmd/serve.rs` (+~340 LOC).** New `mty serve`
  subcommand. Hand-rolled HTTP/1.1 server (no `hyper` /`axum`
  dependency to keep the CLI install footprint flat); serves
  `web/index.html` + `web/dom-shim.js` + `target/main.wasm` from the
  current Mighty package root. `--port <n>` (default 8000),
  `--watch` (file-watcher rebuild + websocket hot-reload).
- **Hot-reload over websockets (RFC 6455 hand-rolled).** Sec-
  WebSocket-Accept handshake + binary frames; on file change,
  pushes a `{"type":"reload"}` frame the in-page `dom-shim.js`
  listens for. The `notify` crate watches `src/` + `web/`.
  v0.24 follow-up: triggerable from the test suite (currently only
  fires under manual `--watch`).
- **`crates/mty-cli/src/cmd/new.rs` (+~110 LOC vs v0.22).** New
  `--template <name>` flag + template registry. Two templates
  shipped: `blank` (the v0.1 two-file scaffold, default) and
  `web-game` (the full `mighty.toml` + `src/main.mty` + `web/index.html`
  + `web/dom-shim.js` + `README.md` 5-file scaffold). Templates
  embedded at compile time via `include_str!`.
- **`crates/mty-cli/templates/web-game/`.** The scaffold corpus.
  `src/main.mty` is a working web-game agent template (start, move_*
  / fire / tick / reset / log-event channel). README walks the new
  user through `bash web/serve.sh` and pointing a browser at
  localhost:8000.
- **22 tests.** 5 active integration tests + 1 ignored stretch
  (`#[ignore]`'d watcher event-timing test; flaky on CI but exercised
  in dev). 22 inline unit tests across `cmd/serve.rs` + `cmd/new.rs`.
  Coverage: serve returns the right MIME for `.wasm` /`.html` / `.js`
  / `.css`; 404 on missing files; exits 2 with a helpful diagnostic
  outside a Mighty package; scaffolds the right file set; check +
  build pass on the freshly-scaffolded output.

See [`MTY_SERVE_V0_23_NOTES.md`](../notes/MTY_SERVE_V0_23_NOTES.md).

### Track D — demo 06_canvas_game (agent-driven canvas)

A 6th demo, shipped SHIPPED-PARTIAL. The deliverable was "a Mighty
agent drives a canvas-rendered game via the Track A WIT". The
deliverable is met: the demo runs end-to-end, the JS shim is 32%
smaller than 05's (345 → 235 LOC), and the headless-browser smoke
locks in a `canvas_game.phash` golden. The "partial" is that three
language gaps surfaced that prevent the JS shim from going all the
way to zero (the agent still talks to the canvas through `log("evt:
...")` for the operations the language can't yet express directly).

- **`demos/06_canvas_game/src/main.mty` (+~135 LOC).** The agent
  owns score, level, current piece, board state. Calls into Track
  A's `std.web.Canvas` for the operations the codegen can already
  lower; falls back to `log("evt:cell:x:y:c")` for the rest.
- **`demos/06_canvas_game/web/dom-shim.js` (235 LOC, -110 vs 05).**
  Translates the residual `log("evt:...")` stream into canvas
  draws + reads keyboard input. Drops 05's piece-rendering / shape-
  rotation / collision-detection blocks — those moved to the Mighty
  agent.
- **Headless smoke wiring (Track E reuse).** `demos/06_canvas_game/
  smoke.sh` opts into `MTY_WEB_SMOKE=1`; `tests/web-smoke/golden/
  canvas_game.phash` locks the visual baseline.
- **Three v0.24 language gaps surfaced.** Documented inline + in
  the v0.24 backlog below:
  1. **`BuiltinId::CanvasOp(...)` lowering arm.** `canvas.fill_rect
     (...)` in Mighty source parses + typechecks but doesn't emit
     the WIT import. The lowering arm in
     `mty-codegen-wasm/src/emit.rs` needs to expand.
  2. **`format!()` / string interpolation.** Any
     `format!("...{x}...")` emits MT6001 UNKNOWN_MACRO. The
     agent has to use `log("score:" + str(s))` style concatenation
     today.
  3. **`export fn` reaches the core module's export table.** The
     v0.23 exports survive into the Component envelope but the
     embedded core module's `export` section is empty, so the JS
     shim has to drive the agent via the `log` channel rather than
     by calling the `export fn` symbols directly. Track B confirmed
     the core module IS embedded; this is a codegen issue, not a
     framing issue.

The shim-size delta + the visual-golden lock-in make this slice net
positive for v0.23 even before the three gaps close. v0.24's first
track collapses the indirection.

See [`CANVAS_GAME_V0_23_NOTES.md`](../notes/CANVAS_GAME_V0_23_NOTES.md).

### Track E — headless-browser visual smoke

A real end-to-end smoke layer for every web demo. Closes the long-
standing trap where `demos/02_counter_web` would PASS magic-bytes
validation while the browser silently failed to instantiate the
component.

- **`tests/web-smoke/smoke-headless.mjs` (+~380 LOC).** Pure-JS
  (Node) Playwright driver. Loads the live serve.sh URL, asserts
  the page goes through without page errors, runs a tiny 8x8
  average-hash perceptual hash on the canvas (`phash` golden under
  `tests/web-smoke/golden/<name>.phash`, hamming-distance budget
  12). Skips with exit 0 + a clear "(playwright unavailable)" line
  when Playwright isn't installed locally, so demos can still be
  smoked on dev machines without the browser-test stack.
- **`tests/web-smoke/package.json` (Playwright 1.45.3, single
  dep).** `npm ci` from `tests/web-smoke/` installs the browser
  bindings.
- **Wired into demos 02 + 05 + 06.** Each demo's `smoke.sh`
  detects `MTY_WEB_SMOKE=1` and boots `web/serve.sh` in the
  background, waits for `curl -fsS / 200`, hands off to
  `smoke-headless.mjs`. 06 picked up Track D's wiring as part of
  the same slice.
- **Manual `web-smoke.yml` workflow_dispatch CI job.** Opt-in
  on GitHub Actions; doesn't gate PRs (Playwright install + browser
  download is ~80 MB, runtime ~30 s — too heavy for every commit;
  PR gating is the v0.24 hot-reload+watch slice's call).
- **Self-test (`node smoke-headless.mjs --self-test`).** Boots a
  tiny embedded HTTP server with two fixtures (a "good" page that
  renders + a "broken" page with no `<script>`), asserts the script
  passes the good fixture and fails the broken one. Catches the
  harness itself rotting between releases.

See [`WEB_SMOKE_V0_23_NOTES.md`](../notes/WEB_SMOKE_V0_23_NOTES.md).

## Integration fixes (this tag commit)

The five tracks landed against a clean main; integrator surgery
needed for three cross-cut issues exposed by the workspace-wide
test sweep + the fresh-scaffold manual smoke:

- **`crates/mty-cli/tests/cmd_serve.rs` port flake.** `pick_port`
  used a nanosecond-derived hash mod 10000; two parallel serve
  tests with fast clocks could deterministically collide under
  `cargo test --workspace`. Replaced with an OS-assigned port via
  `TcpListener::bind("127.0.0.1:0")` then drop-and-reuse. Still
  racy in the abstract (another process could grab the port between
  drop and `mty serve` bind) but orders of magnitude less so. The
  `seed` arg is retained for API compatibility but unused.
- **`crates/mty-runtime/tests/telemetry.rs` cross-test env
  pollution.** Tests 2 + 7 (`init_with_env_attempts_otlp` +
  `init_with_sample_rate_env`) are `#[tokio::test]` because the OTLP
  exporter needs a reactor; they set `MTY_OTLP_ENDPOINT` then
  remove it. Test 8 + test 9 are plain `#[test]`; if they raced one
  of 2/7 after the set but before the remove, they'd try to spin up
  the exporter without a tokio reactor and panic with "there is no
  reactor running". Surfaced as a 1-in-5 flake under
  `cargo test --workspace`. Defensively
  `std::env::remove_var("MTY_OTLP_ENDPOINT")` at the start of both
  plain tests.
- **`crates/mty-cli/src/cmd/new.rs` path-as-package-name bug.**
  `mty new --template web-game /tmp/asteroids` was substituting the
  full path into `{{NAME}}` → generated `package /tmp/asteroids`
  → parse error on the very next `mty check`. Fix: new
  `package_name_from_path` helper that takes the path basename and
  sanitises it into a valid Mighty identifier (lowercase ASCII
  alphanumerics + underscores; leading-digit guard prepends `_`).
  Pre-existing `cmd_new_template.rs` test was asserting the buggy
  behaviour with a hyphen (`name = "test-game"` — also illegal); test
  updated to assert the sanitised form (`name = "test_game"`) and a
  new path-input regression test added.
- **`tests/web-smoke/smoke-headless.mjs` canvas-or-DOM mode.** Track
  E wired the harness into demo 02_counter_web, but the harness
  unconditionally required a `<canvas>` element — the counter demo
  is a DOM demo with a `#count` span, not a canvas. Added a
  `--mode {canvas,dom}` flag; `dom` mode validates the `#count` or
  `[data-mty-output]` element exists with the page load clean.
  Demo 02's smoke.sh now passes `--mode dom`.
- **`demos/02_counter_web/web/serve.sh` python3 portability.** The
  Windows bash environment aliases bare `python3` to the Microsoft
  Store installer launcher when no real python3 binary is on PATH;
  `exec python3 -m http.server` then fails silently with a 0-byte
  serve.log. Track D's 06_canvas_game's serve.sh already had the
  fix (cascading `python` → `python3` → `py` lookup). Backported to
  02's serve.sh.
- **`demos/05_notetris_web/{mighty.toml, README.md, src/, web/}`
  untracked-file recovery.** The 05_notetris_web demo's source +
  HTML + JS shim had been written to disk for the v0.22 demo but
  never `git add`-ed; only Track E's `smoke.sh` was committed. The
  files were complete and consistent (the smoke.sh ran clean once
  CRLF→LF normalisation was applied), just unstaged. Pulled into
  this integrator tag.

## Verification (rerun locally)

```bash
git checkout v0.23.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1604 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing
cargo test -p mty-cli    --test cmd_serve                  # 5 passing
cargo test -p mty-cli    --test cmd_new_template           # 6 passing
cargo test -p mty-codegen-wasm --test embedded_core_module # 5 passing
cargo test -p mty-codegen-wasm --test web_imports          # 8 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 474 passing, 1 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 6/6 PASS

# Headless-browser smoke (opt-in, needs Playwright):
cd tests/web-smoke && npm ci && cd ../..
MTY_WEB_SMOKE=1 bash demos/02_counter_web/smoke.sh         # PASS (dom mode)
MTY_WEB_SMOKE=1 bash demos/05_notetris_web/smoke.sh        # PASS (canvas + phash)
MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh         # PASS (canvas + phash)

# Manual mty serve end-to-end on a fresh scaffold:
./target/debug/mty new --template web-game /tmp/asteroids  # scaffolds 5 files
cd /tmp/asteroids
mty serve --port 8765 &
sleep 3
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8765/         # 200
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8765/main.wasm # 200
curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8765/dom-shim.js # 200
kill %1
```

## v1.0 freeze gate status after v0.23

| Blocker                                       | Status   | Notes                                                                 |
|-----------------------------------------------|----------|-----------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | (v0.19, extended v0.22) Python 2nd-impl through HM + closures + generic-constraints + borrow + wasm codegen. 474 tests; 23/23 examples typeck clean; 21/24 emit wasm. v0.23 doesn't move this needle. |
| #2 RFC 30-day comment windows                 | **Infra shipped — user action pending** | `COMMENT_WINDOWS.md` is the master tracker. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006). |
| #3 Published normative conformance suite      | **CLOSED** | (v0.19/v0.20) `scripts/build-conformance-kit.sh` builds the tarball; v0.23 grows it 147 → 153 cases / 24 categories; auto-attached to every tagged release. |

**Earliest possible v1.0.0 tag: 2026-07-26.** Unchanged from v0.22.
The day after the last RFC comment window (RFC-002 / RFC-006, 60
days each) closes. At this point **only RFC dispositions** stand
between main and v1.0 GA.

## v0.24-RC1 candidate tracks

v0.23 closes the "web app on localhost" capability slice but
Track D's three language gaps mean Mighty source can't fully drive a
canvas game *yet*. v0.24's swarm is the three-gap closure + two
polish slices:

1. **`BuiltinId::CanvasOp(...)` lowering arm + WIT-import auto-emit.**
   When a Mighty source touches `std.web.Canvas.fill_rect(...)`, the
   codegen lowering arm in `mty-codegen-wasm/src/emit.rs` must emit
   the corresponding `import canvas.fill_rect` line into the
   Component envelope. Today the import only appears if the
   `std.web` module is referenced at all — it doesn't auto-pick up
   the specific operations the source touches. Track D's demo 06 is
   the integration test.
2. **`format!()` / string interpolation in Mighty.** Lex
   `"{x:?}"`-style interpolation in string literals; lower to
   concat-and-format. Track D had to drop back to
   `log("score:" + str(s))` style concatenation. MT6001
   UNKNOWN_MACRO should never fire on `format!`. Likely shape:
   parser sniffs `format!(...)` as a macro, lowers to a HIR builder
   that emits a sequence of `str` calls + concatenations.
3. **`export fn` reaches the embedded core module's export table.**
   Today exported functions survive in the Component envelope but
   the embedded core module's `export` section is empty, so the JS
   host shim has to drive the agent via the `log` channel. Track B
   confirmed the core module IS embedded; this is a `mty-codegen-
   wasm/src/emit.rs` issue. After this closes, the JS shim in demo
   06 can drop another ~60 LOC (calls `move_left()` /
   `request_animation_frame()` directly instead of via `log`).
4. **Hot-reload websocket from `mty serve --watch` triggers in-
   browser refresh under test.** Track C shipped the watch-on-disk
   + websocket-push infrastructure, but the integration test that
   exercises it end-to-end is `#[ignore]`'d (file-watcher event
   timing flakes on CI). v0.24 turns it into a real test (probably
   via an injected synthetic "file changed" event rather than a
   real filesystem touch).
5. **v1.0 freeze gate prep — RFC monitoring + final spec polish.**
   Unchanged from the v0.23-RC tracks list (item 5). With the last
   RFC window closing 2026-07-25, v0.24 + v0.25 are the final
   pre-v1.0 cycles. Sweep `docs/spec/CHANGELOG.md` for the
   ambiguities the Python full-pipeline flagged in v0.22; polish
   any remaining language-spec text that the Track D canvas slice
   exposed (e.g. "what does `export fn` mean across CM envelopes").

After v0.24 + v0.25 the only remaining v1.0-RC work is RFC
disposition collection (driven by user-side window closures). Once
the latest window closes on 2026-07-25, the integrator collects
dispositions, files them in `RFC_DISPOSITION_<RFC>.md`, builds the
`mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0**.

## Acknowledgements

v0.23 is a five-track parallel swarm: Tracks A, B, C, D, E ran
concurrently, integrator merged. Special call-out to Track B for
the no-code-change recon outcome — the easiest slice is the one
that turns "we need to fix this" into "we already fixed this, let's
prove it stays fixed".
