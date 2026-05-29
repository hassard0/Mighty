# Mighty v0.34 — Release Notes

**Tag:** `v0.34.0`
**Date:** 2026-05-28
**Status:** SHIPPED — compounding the agent first-shot success rate.

**Headline:** **Mighty v0.34 — compounding the agent first-shot
success rate.** 81 `MTxxxx` codes (up from 31) now ship structured
auto-fix proposals; every fix appears as a one-click LSP CodeAction
in VS Code + JetBrains; the stdlib hover catalog grew from 58 to 203
examples; and a pre-merge fmt/clippy git hook stops the recurring
Linux drift trap before it reaches CI.

v0.33 turned the corner on the agent first-shot loop. v0.34 makes
that loop **converge**: the auto-fix surface is now wide enough to
cover the day-to-day MT2xxx + MT3xxx + MT4xxx codes most agents hit,
the fixes show up as native one-click quickfixes inside both
flagship IDE surfaces (no JSON scraping required), every stdlib
function the LSP can hover now answers with a worked example or
three, and the integrator's most-common foot-gun — Linux-only fmt
drift after a Windows commit — is now blocked at `git push` time by
a hook the swarm can install with `mty hooks install`.

Four tracks merge in parallel; T2 already includes T1, so the
integrator merges three branch tips.

## Track-by-track

### T1 — 50 more MTxxxx fix engines (full MT2xxx coverage + MT3xxx polish + MT4xxx finish)

Branch `v034-track-fix-coverage`, merged inside T2 at `26454d1`.

What ships:

- **+50 structured fix envelopes** across `MT2xxx` (type / shape
  errors), `MT3xxx` (capability + tool-calling errors), and the
  remaining `MT4xxx` (taint + safety) codes. Total fix-capable
  codes: **31 → 81**.
- **`MT2xxx` complete coverage** — every type / shape diagnostic
  now ships at least one structured fix alternative.
- **Multi-alternative envelopes** — `MT4xxx` codes that have
  multiple legitimate untaint paths (sanitise, capability-narrow,
  manual-review-stub) ship them as ranked alternatives.
- **Tests** — `+56` across `crates/mty-diagnostics/`.

### T2 — LSP CodeAction wiring (fix envelopes as one-click quickfixes)

Branch `v034-track-codeaction`, merged at `26454d1`. (Took T1 with
it — T2 was developed on top of T1.)

What ships:

- **LSP `textDocument/codeAction`** — `crates/mty-lsp/src/code_actions.rs`
  now serves every diagnostic envelope as a `CodeAction` with a
  `WorkspaceEdit` payload derived from the envelope's unified diff.
  Per-alternative actions appear when the envelope has more than one
  fix path.
- **Confidence threshold** — `mighty.codeAction.confidenceThreshold`
  (VS Code) / `Settings > Tools > Mighty > Code action confidence`
  (JetBrains) lets users hide low-confidence fixes from the
  quickfix list.
- **VS Code extension** — `tools/vscode/`: bumped to 0.34.0, README
  documents the new quickfix surface, `package.json` declares the
  new setting.
- **JetBrains plugin** — `tools/jetbrains/`: settings panel +
  `MightySettingsState` field, threshold is forwarded to the LSP via
  initialisation options.
- **Diff apply infrastructure** — `crates/mty-lsp/src/diff_apply.rs`:
  turns a unified-diff envelope into a structured `WorkspaceEdit`
  the LSP can return inline; the same path drives `mty fix --apply`.
- **Tests** — `+53` across `crates/mty-lsp/tests/code_action_envelope.rs`.

### T3 — Stdlib hover catalog 58 → 203 entries

Branch `v034-track-hover-expand`, merged at `46d75e3`.

What ships:

- **+145 stdlib hover examples** added to `crates/mty-doc/src/examples.rs`
  covering `std.rag`, `std.computer`, `std.swarm`, `std.observe`,
  `std.taint`, `std.eval`, `std.web`, `std.fs`, `std.json`,
  `std.string`, `std.vec`. Total catalog: **58 → 203 examples**.
- Every public stdlib item the LSP hover query can resolve now
  answers with at least one worked example (was: ~40%).
- **Docs** — `docs/internals/lsp-hover.md` updated with the
  per-namespace coverage table.
- **Tests** — `+2` catalog assertions (`catalog_size_grew`,
  `every_top_level_namespace_present`); the 145 individual examples
  are tested via the existing hover integration suite.

### T4 — MT4099 span fidelity + schema_version + receiver-type hover + pre-merge hook

Branch `v034-track-quality-bumps`, merged at `69e914b`.

Four small but high-leverage quality bumps.

What ships:

- **`MT4099` emit-site span fidelity** — `crates/mty-types/src/taint.rs`:
  the taint diagnostic now points at the exact `call(tainted)` byte
  range, not the enclosing function. Fixes a class of confused
  `WorkspaceEdit`s where the quickfix targeted the wrong line.
- **`schema_version` field on every envelope** — `crates/mty-diagnostics/src/fix.rs`:
  `DiagnosticEnvelope` now carries `schema_version: "1.0"`, with
  `#[serde(default)]` so pre-v0.34 envelopes round-trip. Versioning
  policy at `docs/internals/diagnostic-envelopes.md`.
- **Receiver-type hover resolution** — `crates/mty-lsp/src/hover.rs`:
  hovering a method on a local binding now resolves the receiver
  type and surfaces the receiver's hover entry. Fixes
  `let r = std.web.client(); r.get(...)` showing nothing for `r`.
- **Pre-push fmt + clippy hook** — `.git-hooks/pre-push` + new
  `mty hooks install` subcommand. Mirrors the two cheapest CI gates
  (fmt + clippy) at `git push` time so Windows-only swarm commits
  don't keep landing Linux-only fmt-drift surprises.
- **Tests** — `+21` across taint span fidelity, hover receiver type,
  schema_version round-trip.

## Gates

Validated on vulcan (Intel Xeon multi-socket, Ubuntu 24.04, Rust
1.95.0). All green:

- `cargo build --workspace` — clean.
- `cargo test --workspace --no-fail-fast` — **2887 passed, 0
  failed** across the workspace (pre-v0.34: 2766; +121 over the 4
  feature tracks; Doc-tests stable).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean (pre-push hook caught a T2/T4
  merge gap before the integrator commit landed — see Integrator
  notes).
- `cargo audit --deny warnings` — clean.
- VS Code extension (`tools/vscode`) `npm run compile` — clean,
  package version 0.34.0.
- Playground (`tools/playground`) `npm run build` — clean.
- JetBrains plugin (`tools/jetbrains`) — Java not installed on the
  integrator workstation; CI matrix builds it.

## Integrator notes

The pre-push hook that T4 added paid for itself on its first run:
the integrator merge of T1+T2 followed by T4 introduced a missing
`schema_version` field in the MT4099 test envelope inside
`crates/mty-lsp/src/code_actions.rs`. The pre-push hook caught it
locally before the commit reached origin/main. Fix shipped as
`5fc6bb0`. **This is exactly the foot-gun the hook was designed for**
and is the first confirmed save against the v0.33 Linux-fmt-drift
recurrence pattern.

The hook is installed in main worktree by default for the
integrator; swarm worktrees can install it via `mty hooks install`.

## v0.34 follow-ups (rolled up across all 4 tracks → v0.35 backlog)

### T1 — Fix coverage
- Backfill the remaining ~30 codes in the v1.0 registry (81 of ~110
  in the registry; v0.35 should reach 100).
- Confidence-score calibration against a labelled fix-success-rate
  corpus — today's scores are hand-tuned per-code.
- `mty fix --auto-pick` to apply the highest-confidence alternative
  without an LSP round-trip, for batch jobs.

### T2 — LSP CodeActions
- Streaming refactor actions — multi-step fixes (rename + split +
  retag) should appear as a single action that opens a wizard,
  not as N separate quickfixes.
- IntelliJ "Show Intention Actions" parity — the current JetBrains
  surface lists quickfixes; v0.35 should also surface them in the
  Alt+Enter intention list.
- Per-fix telemetry hook so the IDE surface can report which
  alternatives users pick most.

### T3 — Hover catalog
- Multi-modal hover — images and SVGs in `///` doc comments should
  render inline (today they linkify).
- Catalog test sharding — the catalog is now 1237 lines in one
  file; v0.35 should split per-namespace.
- User-workspace symbols in hover — today scope is stdlib + current
  file; v0.35 should hover types from `use` imports too.

### T4 — Quality bumps
- Extend span fidelity to `MT3xxx` (capability errors currently
  highlight the whole call, not the offending arg).
- `schema_version` migration tooling — `mty diag migrate-envelopes`
  to rewrite pre-1.0 envelope dumps in shared fixtures.
- Receiver-type hover for chained calls
  (`r.get(...).body().text()`) — today only resolves the first hop.
- Pre-push hook telemetry — record how often the hook saves a
  drift commit, so we can decide whether to upgrade it to a
  pre-commit.

### Cross-cutting / integrator lessons (v0.35)

- **Vulcan disk hygiene** — vulcan was at 91% before clean even
  with `/tmp/v033-*` purged. v0.35 integrator should mount a
  larger /tmp or budget a fresh build cache periodically.
- **`SCHEMA_VERSION` re-export** — `mty_diagnostics::SCHEMA_VERSION`
  is currently accessed via `mty_diagnostics::fix::SCHEMA_VERSION`;
  v0.35 should re-export it from the crate root so downstream code
  doesn't need to know the module layout.
- **Pre-push hook in CI** — the hook is currently install-on-pull
  only. v0.35 should consider running it inside CI as a redundant
  safety net for contributors who haven't installed it.

## Onward

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26. v0.34's auto-fix coverage and
hover catalog growth pull two of the v1.0 freeze-gate items
(structured-fix coverage; stdlib hover surface) into "ready".
