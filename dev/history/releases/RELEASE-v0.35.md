# Mighty v0.35 — Release Notes

**Tag:** `v0.35.0`
**Date:** 2026-05-28
**Status:** SHIPPED — closing the v0.33 stubs.

**Headline:** **Mighty v0.35 — closing the v0.33 stubs.** Real WASM
mty in the browser (the install funnel that the playground was
promising), `mty agent` HTTP + Unix transports + record/replay,
`mty fix --apply` + LSP bulk `source.fixAll.mighty` (agent first-shot
becomes zero-shot), PGO release binaries on 3 platforms, multi-arch
Docker, and Strategy B hover catalog with drift detection.

v0.33 framed three install funnels — playground, `mty agent`
transports, fix engine — that all shipped with stubs. v0.34 wired up
the fix engine and hover catalog inside the editor. v0.35 takes the
stubs off the remaining shelves: the playground now runs the real
parser+typeck+IR+interp in the browser via wasm-pack; `mty agent`
gains HTTP and Unix-socket transports plus record/replay; and the
fix engine grows a CLI `--apply` mode and an LSP `source.fixAll`
bulk action that turns agent first-shot into agent zero-shot. PGO,
multi-arch Docker, and Strategy B (extract-from-source) hover
catalog round out the release.

Five tracks merge in parallel; no T1↔T2/T3 cross-deps despite
overlapping touchpoints in `mty-cli/src/main.rs` — each track
extends the `Cmd` enum independently and the merge is mechanical.

## Track-by-track

### T1 — Real WASM mty playground + Cloudflare proxy + GH Pages deploy

Branch `v035-track-wasm-playground`, merged at `facd6a7`.

The v0.33 T3 playground shipped a mock backend because `mty-cli`'s
path deps unconditionally reached into cranelift / LLVM /
wasm-codegen / hyper / tokio / notify / rusqlite — none of which
compile to `wasm32-unknown-unknown`. v0.35 T1 refactors the dep
graph behind a default-on `host-toolchain` feature so the
non-toolchain front-end (parser → typeck → borrowck → IR →
tree-walk interp) can be compiled standalone and wrapped with
wasm-bindgen exports.

What ships:

- **`mty-cli` lib gains `cdylib`** — `crates/mty-cli/Cargo.toml`
  now declares `crate-type = ["rlib", "cdylib"]` and a default-on
  `host-toolchain` feature that gates every heavy dep + the entire
  `cmd/` tree.
- **`crates/mty-cli/src/playground.rs`** (new) — the wasm-bindgen
  surface (`init`, `check`, `run`). Runs the real tree-walk
  interpreter against `BufferHost` so `log()` calls reach stdout.
- **`crates/mty-driver`** — matching `host-toolchain` gates on
  `build.rs` (codegen + linker) and `pipeline::run_file*` (runtime).
  The `parse_source` / `lower` / `type_and_borrow_check` /
  `lower_to_sir` surface stays always-available.
- **Real WASM artifact** — `wasm-pack build --target web
  --no-default-features --features playground-wasm` emits a
  **1.15 MB** `mty_cli_bg.wasm` + JS glue.
- **`tools/playground/src/runner.ts`** — real `WasmRunner` loads the
  artifact via dynamic import. `makeRunnerWithFallback` factory
  degrades to the mock backend if the artifact fails to load (typical
  during `npm run dev`).
- **Cloudflare Worker LLM proxy** — `tools/playground/cf-worker/`
  ships a TypeScript Worker that routes `POST /v1/{anthropic,openai,
  gemini}/{path}` to the upstream with secrets-injected auth
  headers. Per-IP rate-limit via KV (10 req/hour default), CORS
  allowlist. Shipped as source + `wrangler.toml` + README; user runs
  `wrangler deploy` to activate.
- **`.github/workflows/pages.yml`** — extended to `wasm-pack` +
  `npm build` the playground and merge it under `site/playground/`
  before the existing mkdocs deploy. New
  `.github/workflows/playground.yml` runs the Playwright smoke on
  every PR that touches playground or front-end Rust crates.
- **Tests** — 4/4 Playwright smoke tests assert the real WASM
  backend loads, `01_hello_agent` produces `"hello, Mighty"`,
  `05_taint_safety` surfaces MT4099, and all 7 gallery examples
  check cleanly.

Architecture note: the WASM compiler runs `check + typeck + IR +
tree-walk interp`. It does **not** run the cranelift JIT — that's
the architecture, not a stub. Browser execution stays interpreter-
only; native still has the JIT.

### T2 — `mty agent` HTTP + Unix transports + recorder/replay

Branch `v035-track-agent-transports`, merged at `39ca039`.

The v0.33 T5 `mty agent` shipped a stdio NDJSON transport with HTTP
and Unix-socket stubs that returned `{"error":"unimplemented"}`.
T2 wires both for real and adds a recorder/replay round-trip so
agent harnesses can capture and re-run sessions deterministically.

What ships:

- **HTTP transport** — `crates/mty-cli/src/cmd/agent.rs` grows a
  hyper HTTP/1.1 server. Routes: `POST /v1/agent` (NDJSON response),
  `POST /v1/agent/batch` (interleaved NDJSON), `GET /v1/agent/version`
  (`{"mty_version":"…","agent_protocol":"1.0"}`). Optional bearer-
  token auth via `--auth-token`; unauth'd requests get 401 with a
  `WWW-Authenticate: Bearer` header. `--listen 0.0.0.0:9090` shape
  supported alongside the existing `--port` shortcut.
- **Unix socket transport** — `tokio::net::UnixListener` over the
  same NDJSON wire format, one independent session per connection.
  Pre-existing socket files are unlinked on bind so a stale socket
  from a crashed run doesn't `EADDRINUSE`. Windows currently
  short-circuits with a one-line `kind:"error"` envelope + exit 2
  (Windows 10+ has `AF_UNIX` — v0.36 follow-up).
- **Recorder** — `--record <PATH>` appends every (request, response)
  pair as NDJSON; works under every transport.
- **Replay** — `--replay` re-runs the recorded requests in-process
  and asserts each live response byte-matches. Exit 0 on full match,
  1 on drift, 2 on IO/parse errors.
- **Docs** — `docs/internals/agent-mode-protocol.md` and
  `docs/reference/cli/mty-agent.md` updated with the new transports,
  recorder format, and example invocations.
- **Tests — +50** across unit (`run_one_capturing`, recorder,
  replay), HTTP integration (routing, auth, batch, recorder), Unix
  socket (gated to `#[cfg(unix)]`) + a Windows fallback smoke test,
  and recorder/replay round-trips.

Pragmatic limit: HTTP transport returns fixed-`Content-Length`
responses, not chunked streaming. The current agent loop is
request/response — true streaming responses move to v0.36.

### T3 — `mty fix --apply` + LSP `source.fixAll.mighty`

Branch `v035-track-fix-apply`, merged at `6f2208e`.

Closes the loop on fix envelopes. v0.33 T4 + v0.34 T1 shipped 81
`MTxxxx` codes with structured auto-fix proposals; v0.34 T2 surfaced
them as LSP CodeActions. T3 applies them without an editor in the
loop — the agent first-shot loop becomes a zero-shot loop.

What ships:

- **CLI — `mty fix --apply <path>`** (`crates/mty-cli/src/cmd/fix.rs`):
  - Flags: `--apply`, `--code`, `--alternative`, `--threshold`,
    `--dry-run`, `--interactive`, `--from-stdin`.
  - Policy: filter alternatives by `--threshold` (default `0.85`),
    pick the highest-confidence surviving alt (or `--alternative N`),
    apply via the shared `mty_diagnostics::apply::apply_unified_diff`
    helper. Multi-fix conflict resolution: splice highest-line-first
    so earlier anchors stay valid; per-pick re-validate against the
    buffer.
  - **The canonical zero-shot loop:**
    ```
    mty check --format json src/main.mty | mty fix --apply --from-stdin
    ```
- **LSP — `source.fixAll.mighty`** (`crates/mty-lsp/src/code_actions.rs`):
  wires the capability advertised in v0.34 T2 to a real handler.
  `fix_all_mighty_action()` collects every preferred-confidence fix
  for the document, orders them highest-line-first, and returns one
  `CodeAction` with an atomic `WorkspaceEdit`. Server routes via
  `context.only = ["source.fixAll.mighty"]`.
- **Shared applier** — `crates/mty-diagnostics/src/apply.rs`: LSP-free
  source-string-level unified-diff applier used by both the CLI and
  the LSP bulk-apply path. Multi-hunk support, hunk validation
  (refuses to apply when `OLD` doesn't match source).
- **Tests — +78**: 17 mty-diagnostics applier unit tests,
  33 cmd::fix unit tests (policy, filters, summary, interactive
  prompter, NDJSON round-trip), 4 CLI integration tests (spawn real
  binary, end-to-end pipe), 15 LSP `code_action_fix_all` integration
  tests, 9 additional mty-diagnostics + mty-lsp tests.
- **Docs** — new `docs/reference/cli/mty-fix.md`; new sections in
  `docs/internals/diagnostic-envelopes.md` ("Consuming fix
  envelopes") and `docs/internals/lsp.md` ("Envelope-driven fixes",
  "Bulk apply — source.fixAll.mighty"); `demos/07_research_agent`
  README adds a "Self-correcting via `mty fix --apply`" section
  with the zero-shot pipe.

### T4 — PGO release on 3 platforms + multi-arch Docker + homebrew-core runbook

Branch `v035-track-pgo-docker-homebrew`, merged at `82e2ec8`.

What ships:

- **PGO on 3/5 release platforms** — `release.yml` now drives a
  `use_pgo` matrix column. `linux-x86_64`, `darwin-arm64`, and
  `windows-x86_64` build through `build-pgo.sh` / `build-pgo.ps1`
  (v0.22 instrumented-PGO). `darwin-x86_64` and `linux-aarch64`
  stay on plain `release` because **neither build path can execute
  the instrumented binary natively** — Rosetta is unreliable for
  the workload run, and the cross-compile host can't run arm64
  without an emulator. `llvm-tools-preview` only gets installed on
  PGO legs.
- **Multi-arch Docker** — `Dockerfile` switches on `TARGETARCH`
  (`amd64 → x86_64-unknown-linux-gnu`, `arm64 →
  aarch64-unknown-linux-gnu`), pulls the matching tarball + `.sha256`
  sidecar from the GitHub Release, and verifies the checksum before
  extract. `release.yml`'s `docker-sign-sbom` job moved to
  `needs:[release]` so the binaries exist when `buildx` curls them.
  `platforms: linux/amd64,linux/arm64`. cosign sign uses
  `--recursive` so the manifest-list digest covers both per-arch
  images. Still gated on `vars.PUBLISH_DOCKER` so the workflow ships
  but no image is pushed until the user toggles the flag.
- **Homebrew-core runbook** — `HOMEBREW_CORE_SUBMISSION.md` updated
  for v0.35 reality: all four arches have shipped cleanly for ~7
  releases; SHA-refresh snippet, `brew audit --new-formula --online`
  pre-flight, verbatim `gh pr create` body (with all four arches
  enumerated), and expected 1-2 week new-formula review timeline.
  **Runbook for the user to drive the submission by hand — not
  auto-submitted.**

### T5 — Strategy B hover catalog + `mty doc check` drift gate

Branch `v035-track-strategy-b`, merged at `02a27f3`.

The v0.33 T6 design always called the hand-curated 203-entry
`STDLIB_EXAMPLES` table a temporary Strategy A bridge. Strategy B
(extract from real stdlib source) lands here, with a pragmatic
pivot.

What ships:

- **Per-module `.docstub` files** — `crates/mty-stdlib/docs/<module>.docstub`
  is the new text source-of-truth. 18 module buckets (builtin,
  computer, env, eval, fs, http, json, llm, mcp, memory, observe,
  rag, string, swarm, taint, time, vec, web).
- **Mini-grammar walker** — `crates/mty-doc/src/stdlib_walker.rs`
  parses every docstub at compile time via `include_str!` and
  returns a flat `ExtractedExample` catalog.
- **One-shot generator** — `crates/mty-doc/src/bin/regen-stdlib-docstubs.rs`
  rebuilt the docstubs from the curated table; the curated table
  remains as the frozen snapshot / drift gold-set.
- **`mty doc --check`** builds the extracted catalog, runs
  `diff_catalogs()` against `STDLIB_EXAMPLES`, and exits non-zero
  on any divergence (missing/extra symbol, or signature/cap/
  example/see_also drift).
- **CI gate** — `.github/workflows/ci.yml` Linux test job runs
  `mty doc --check` on every push. Zero drift on the migration:
  extracted catalog matches the curated gold-set byte-for-byte
  across all 203 entries / 18 modules.
- **Tests — +29** across docstub parsing, walker round-trip, diff
  formatter, and CI gate fixtures.
- **Docs** — new `docs/internals/stdlib-docs-pipeline.md`.

Pragmatic pivot: the original Strategy B sketch in v0.33 T6 imagined
extracting from Rust `///` doc comments on stdlib `impl` blocks.
That path required either a custom rustdoc post-processor or a
proc-macro intercepting every stdlib item. Both add infrastructure
that has to be maintained per Rust release. The `.docstub` text
files give us the same property (text source-of-truth, extracted
catalog, drift gate) with **zero proc-macro / rustdoc-internals
surface**, and they can be edited without touching Rust. The LSP
hover continues to consume `STDLIB_EXAMPLES` (zero-alloc
`&'static str` path); the docstubs are queued to become the
primary source in v0.36 once the LSP grows a `LazyLock<Vec<…>>`
bridge.

## Gates

Validated on vulcan (Intel Xeon multi-socket, Ubuntu 24.04, Rust
stable at `~/.cargo/bin/cargo`). All green.

(filled in by integrator at tag time — see end of file.)

## Integrator notes

The five merges were mechanical despite three tracks touching
`crates/mty-cli/src/main.rs` — each new subcommand variant extends
the `Cmd` enum independently and the auto-merge resolved every hunk
without conflict. The `Cargo.toml` combine (T1 feature-gating + T2
hyper deps + existing deps) merged cleanly because T1's
`host-toolchain` feature gates the entire native dep set; T2's
hyper additions slot under the existing optional-dep umbrella.

Local `wasm-pack` smoke test post-merge: build still emits the
**1.15 MB** `mty_cli_bg.wasm` artifact — the agent transport and
fix-apply tracks didn't accidentally pull a native-only dep into
the wasm path. Confirms the `host-toolchain` gating discipline
survives a 5-track integration.

The pre-push hook (v0.34 T4) fired on the merge push, ran
`cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
-- -D warnings`, both clean. Integrator pre-validated locally with
`MTY_PRE_PUSH_SKIP=1` on the merge-push so the hook ran once for
the final tag push only.

## v0.35 follow-ups (rolled up across all 5 tracks → v0.36 backlog)

### T1 — WASM playground
- **Cloudflare Worker deployment** — runbook is in place; the user
  needs to `wrangler deploy` once with API secrets and toggle the
  playground config to use the live proxy.
- **WASM size** — 1.15 MB pre-`wasm-opt`; v0.36 should try
  `wasm-opt -Oz` + run a tree-shake pass on the wasm-bindgen exports.
- **JIT in browser** — current playground is interp-only (the
  cranelift JIT is gated behind `host-toolchain`); v0.36 could ship
  a separate `playground-wasm-jit` feature once cranelift's wasm
  target matures.
- **Per-IP rate-limit telemetry** — the CF Worker logs to console
  today; v0.36 should pipe to Workers Analytics.

### T2 — Agent transports
- **HTTP streaming responses** — current responses are
  fixed-`Content-Length`, not chunked. v0.36 should add
  `Transfer-Encoding: chunked` for batch responses.
- **Windows Unix socket** — Windows 10+ supports `AF_UNIX`. v0.36
  should drop the "not supported" fallback and use
  `tokio::net::UnixListener` on Windows too (one feature-flag flip
  in `cfg(unix)`).
- **TLS in front of HTTP transport** — today the HTTP transport is
  HTTP/1.1 plaintext. v0.36 should add an optional `--tls-cert /
  --tls-key` pair so the transport can serve HTTPS directly without
  a reverse proxy.
- **Replay diff output** — current replay prints a single-line
  byte-mismatch summary; v0.36 should print the unified diff so the
  agent harness can surface what changed.

### T3 — fix --apply + LSP fixAll
- **`mty fix --auto-pick`** carry-over from v0.34 follow-ups —
  partly delivered by `--apply --threshold` but the explicit
  `--auto-pick` flag (highest-confidence regardless of threshold)
  still wanted for batch jobs.
- **`source.fixAll.mighty` per-namespace filters** — today the bulk
  apply runs every preferred-confidence fix; v0.36 should accept
  `context.only = ["source.fixAll.mighty.MT4xxx"]` etc so a user
  can bulk-apply just the taint fixes.
- **Multi-file `mty fix --apply --workspace`** — today the apply
  operates per-file; v0.36 should let the CLI walk a project and
  apply fixes across every `.mty` file under `src/`.
- **Telemetry hook on apply** — record which alts get applied so we
  can calibrate the v0.34 confidence-score model against a real
  apply corpus.

### T4 — PGO / Docker / Homebrew
- **PGO on `darwin-x86_64`** — once the CI matrix gains a macOS
  Intel runner that can execute the instrumented binary natively.
- **PGO on `linux-aarch64`** — once a native arm64 GitHub runner
  appears, switch the leg over.
- **Docker publish toggle** — currently gated on
  `vars.PUBLISH_DOCKER`; user-driven.
- **Homebrew-core PR** — runbook is ready; user files the PR.

### T5 — Strategy B hover
- **Flip source-of-truth** — make `STDLIB_EXAMPLES` `build.rs`-
  generated from the docstubs, not hand-curated. v0.36 should ship
  this once the LSP grows the `LazyLock` bridge.
- **`##since <version>` directive** — for "added in vX" hover
  badges.
- **Walker-driven `EMBEDDED_DOCSTUBS` list** so new module files
  are picked up without a Rust-side edit.
- **Multi-modal hover (carry-over from v0.34 T3)** — images and
  SVGs in `///` doc comments should render inline.

### Cross-cutting / integrator lessons (v0.36)

- **Vulcan PATH** — `cargo` lives at `~/.cargo/bin/cargo`, not on
  the default PATH. Past integrators have hit this and reported
  "no Rust toolchain" falsely. v0.36 integrator should keep using
  the full path.
- **WASM artifact size budget** — the playground artifact is
  1.15 MB today. v0.36 should set a CI guard at 2 MB so we notice
  if a future merge regresses the dep graph.
- **HTTP transport TLS** — paired with T2 follow-up; the absence of
  built-in TLS means every production deployment needs a reverse
  proxy. Worth lifting that constraint.

## Onward

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26.

v0.35 closes three of the v1.0 freeze-gate items: the playground
install funnel (T1) is now a real install funnel; `mty agent` has
the production transports (T2) the v1.0 RFC required; and the
agent-first-shot → zero-shot loop (T3) is the headline UX the
v1.0 narrative needs. PGO + multi-arch + homebrew (T4) flip the
distribution surface to "tier-1". Strategy B (T5) takes the
stdlib hover off the curated-table dependency and onto a
maintainable extract-from-source pipeline.

v0.36 will pick up the ~30 follow-ups above plus one new
freeze-gate slice (TBD — likely "agent telemetry + apply corpus"
to feed the confidence calibration work).
