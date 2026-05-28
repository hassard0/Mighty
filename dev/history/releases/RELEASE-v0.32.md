# Mighty v0.32 — Release Notes

**Tag:** `v0.32.0`
**Date:** 2026-05-28
**Status:** SHIPPED — the *debugger + multi-arch + replay-closure* release.

**Headline:** **Mighty v0.32 — debugger + multi-arch + replay closure.
`mty dap` ships across VS Code + JetBrains (Community + Ultimate), 2 new
release targets (macOS x86_64 + Linux aarch64), and the 3 v0.29 replay
backlog items are all closed.**

v0.31 was the *DX shell* — the editor surfaces, the install manifests,
the GH Actions library. v0.32 fills those surfaces in:

- The debugger that v0.31's Track 2 + Track 3 left as a follow-up now
  ships end-to-end as **Track A** (`mty dap`) wired into both editors.
- The CodeLens + webview that v0.31's Track 2 sketched ship as **Track B**.
- The Community-edition fallback that v0.31's Track 3 deferred ships as
  **Track C**.
- The 2 missing release targets (`x86_64-apple-darwin`,
  `aarch64-unknown-linux-gnu`) and the homebrew-core runbook ship as
  **Track D**.
- The `cost-delta` + `mty-explain` composite actions and the
  `error_code` output that v0.31's Track 5 promised ship as **Track E**.
- The 3 long-standing v0.29 replay backlog items ship as **Track F**,
  unblocking Track A.

Six tracks merge in parallel. All 9 demos pass `smoke.sh` pre and
post; clippy / fmt / audit green.

## Track-by-track

### Track A — DAP debug adapter

Branch `v032-track-dap`, merged as `761267c → 2790926`. Track A is
the only track that depended on another (Track F's structured
`tool_uses` + `ReplayDriver::replay_all`) — that's why the branch
landed with Track F's commits already merged into it.

What ships:

- **`mty dap`** — new CLI subcommand under `crates/mty-cli/src/cmd/dap.rs`
  speaking the Debug Adapter Protocol over stdio. Mounts the existing
  interpreter (`crates/mty-ir/src/interp/`) and exposes breakpoints,
  step-in / step-over / step-out, the variables view (each `let`
  binding + Track-F structured `tool_uses` for LLM calls), and the
  call stack.
- **`crates/mty-ir/src/interp/breakpoints.rs`** + `interp/debug.rs` —
  the interpreter side of the adapter: breakpoint registry, stop
  reasons, frame snapshot construction.
- **VS Code launcher** — `tools/vscode/src/dap.ts` registers a debug
  adapter descriptor factory + a default config resolver so users
  can hit F5 on any `.mty` file **without** writing a `launch.json`.
  The extension synthesises one. Custom configs are still supported
  for `replayTrace` / `recordTrace` overrides.
- **JetBrains run configuration** — `tools/jetbrains/src/main/kotlin/dev/mighty/jetbrains/debug/`
  ships `MightyDebugConfigurationType` + `MightyDebugRunConfiguration` +
  `MightyDebugSettingsEditor`. New "Mighty Debug" entry in **Run →
  Edit Configurations…**. Works in both Community + Ultimate JetBrains
  IDEs (the debug API is in platform core, unlike the LSP API).
- **`examples/37_debug_demo.mty`** — minimal program with a few
  breakpoint-friendly call sites.
- **Tests** — `+33` across the new crates and the LSP.

**Ship constraint:** the JetBrains debug configuration uses run-target
plumbing + console mode in v0.32. The full XDebugger UI integration
(step-in / step-over / variables panel in the JetBrains debug tool
window) is the v0.33 follow-up — what ships here is the run-target
plumbing, console mode, and the option surfaces. VS Code gets the
full UI in v0.32 because VS Code's DAP wiring **is** the debugger
UI.

### Track B — VS Code polish

Branch `v032-track-vscode-polish`, merged as `4ea405f → c95f42d`.

What ships:

- **Cost CodeLens** — `tools/vscode/src/codelens.ts`. Every
  `@tool(`, `swarm(`, `Member.<vendor>(`, and `.ask(` line gets a
  CodeLens showing today's per-file cost + call count. Polls
  `mty inspect --cost --json` every 60s and on document save.
  Click the lens to open the per-file breakdown in a terminal.
- **Cost side-panel webview** — `tools/vscode/src/webview/costPanel.ts`.
  Theme-aware HTML panel with summary cards (today / 7d / 30d /
  all-time), per-provider + per-model bar breakdowns, and a top-10
  most expensive calls table. Auto-refreshes every 30s. Replaces
  the terminal `mty inspect` command; the old terminal flavour
  stays available as `Mighty: Inspect cost (terminal)`.
- **Tree-sitter semantic-tokens stub** — `tools/vscode/src/tree-sitter.ts`.
  Registers a placeholder provider so theme files can target our
  forward-compatible token legend (incl. the custom `taintedType`
  token) today.

**Ship constraint:** tree-sitter semantic tokens is a **stub** in
v0.32. The provider registers + publishes the legend; the WASM
grammar artifact isn't shipped yet. Theme authors can target the
custom tokens today; the actual tree walk lands in v0.33. CodeLens
and the webview are real and shipping.

### Track C — JetBrains Community-edition fallback

Branch `v032-track-jetbrains-ce`, merged as `a22c227 → 6aafb0c`.

What ships:

- **TextMate fallback** — bundled `mighty.tmLanguage.json` registered
  via `org.jetbrains.plugins.textmate`'s bundle facility so syntax
  highlighting works on Community editions that lack the LSP API.
  Bundle lives at `tools/jetbrains/src/main/resources/textmate/`.
- **Adaptive LSP load** — LSP-dependent extensions moved behind
  `<depends optional="true" config-file="mighty-lsp.xml">com.intellij.modules.lsp</depends>`
  so the plugin loads cleanly on Community editions and silently
  skips the LSP-only extensions.
- **`since-build` 232** — broader compat (IDEA 2023.2+ Community +
  Ultimate).
- **Cost TreeTable** — the cost tool window now renders a proper
  sortable TreeTable with Date / Provider:Model / Calls / Cost
  columns and a right-click "Copy as JSON" action. Replaces the
  v0.31 HTML pre-block stub.

**Ship constraint:** the TextMate grammar is **duplicated** between
`tools/vscode/syntaxes/` and `tools/jetbrains/src/main/resources/textmate/`.
JetBrains plugin packaging doesn't follow symlinks, so two copies
live in the tree. **v0.33 chore:** extract a single canonical grammar
at `tools/grammars/mighty.tmLanguage.json` and point both builds
at it via a build-time copy step.

### Track D — release multi-arch + Homebrew

Branch `v032-track-multiarch`, merged as `384f50e → c6079a6`.

What ships:

- **`release.yml`** extended from 3 → **5 platforms**:
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`. Each platform produces a tarball + sha256.
- **Homebrew formula** audit-clean (`brew audit` passes) +
  `tools/distribution/homebrew/HOMEBREW_CORE_SUBMISSION.md` runbook
  documenting the homebrew-core submission process.
- **`tools/distribution/asdf-mty/`** plugin skeleton — `bin/download`,
  `bin/install`, `bin/list-all`, `lib/utils.bash`, README. Ready to
  spin out into `hassard0/asdf-mty` once v0.32.0 binaries publish.
- **Multi-arch dry-run** in CI — `release.yml` gains a workflow_dispatch
  knob that builds the 5-platform matrix without publishing, so we can
  catch cross-compile breakage before tagging.
- **Cosign + SBOM** gated step — `vars.PUBLISH_DOCKER == 'true'`
  enables Docker push + cosign signing + SBOM publish. Off by default
  until the Docker push pipeline lands.

**Ship constraints:**
- The 5-platform release.yml has been validated with the dry-run
  matrix on CI, but the **first real release on the new matrix is
  v0.32.0 itself**. If any new target fails at tag time, the recovery
  path is a surgical `v0.32.1` retag (don't amend `v0.32.0`).
- Cosign + SBOM are **gated off** by default. Flipping them on is a
  v0.33 follow-up once Docker push lands in `release.yml`.
- The Homebrew formula carries 2 placeholder SHAs for the new
  targets (macOS x86_64, Linux aarch64) until v0.32.0 binaries
  publish; a follow-up commit pins them.

### Track E — GH Actions cost-delta + explain

Branch `v032-track-gha-extend`, merged as `287a5e0 → d2752cf`.

What ships:

- **`cost-delta`** composite action — `tools/gh-actions/cost-delta/action.yml`.
  PR comment with per-provider cost delta vs the base branch.
  Example workflow at `tools/gh-actions/examples/cost-delta-pr.yml`.
- **`mty-explain`** composite action — `tools/gh-actions/mty-explain/action.yml`.
  Wraps `mty explain MTxxxx` and pastes the rendered diagnostic
  into a PR comment. Example workflow at
  `tools/gh-actions/examples/mty-explain-on-failure.yml`.
- **`error_code` output** — `tools/gh-actions/mty-check/action.yml`
  now emits an `error_code` output so downstream steps can branch
  on the diagnostic class.
- **`tools/gh-actions/examples/dependabot.yml`** — example pinning
  the composite actions via dependabot.

### Track F — replay completion

Branch `v032-track-replay-complete`, merged as `bb14912` (folded
into Track A's branch before A merged).

What ships — **all 3 v0.29 replay backlog items are now closed**:

1. **`MemberReply.tool_uses`** structural payload — `crates/mty-stdlib/src/swarm/member.rs`.
   Replaces the v0.29 ad-hoc tool-use-string field with a typed
   `Vec<ToolUse>`. Surfaces in `mty inspect` and (new in Track A)
   the DAP variables view.
2. **`ReplayDriver::replay_all` interleaved with `with_provider`** —
   `crates/mty-runtime/src/replay.rs`. The driver can now swap
   provider implementations mid-replay so users can re-score a
   recorded trace against a different model without re-running the
   live program.
3. **`MTY_RECORD_TRACE` env auto-captures via the recorder
   integration** — setting the env var on a live run now appends
   every event the runtime emits to the named trace file.
   `mty dap` flips this on when `recordTrace` is set in a
   `launch.json`.

Tests `+24`.

**Ship constraint:** `Case::from_trace` is now native-only. The
JSON-lines auto-route is **retired** — the JSON-lines recorder
variant ships with a deprecation shim for one cycle (v0.32) and is
removed entirely in v0.33.

## Gates

Validated on vulcan (4×V100, Ubuntu 24.04, Rust 1.95.0). All green:

- `cargo build --workspace` — clean.
- `cargo test --workspace --no-fail-fast` — see CHANGELOG for the
  exact count (~2559 = 2502 prior + 57 from A+F).
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- `cargo audit --deny warnings` — clean.
- `cargo test -p mty-driver --test conformance_full` — 159/159.
- All 9 demos pass `smoke.sh`. `MTY_AGENT_SMOKE=1 bash demos/08_swarm_review/smoke.sh`
  passes.

## v0.33 follow-ups (rolled up)

- **Track A** — JetBrains XDebugger frontend over the DAP run-target
  plumbing; per-line breakpoint serialization across IDE restarts;
  DAP `attach` request for long-running agent processes.
- **Track B** — finish the tree-sitter semantic-token provider
  (ship the WASM grammar artifact and fill in
  `provideDocumentSemanticTokens`); per-span CodeLens granularity
  once `mty inspect` exposes a `--by span` flag; ship the
  cost-panel JS bundle for interactive drill-down; trace replay UI
  for `.mty-trace` files.
- **Track C** — extract a single canonical TextMate grammar at
  `tools/grammars/mighty.tmLanguage.json`; tree-sitter binding via
  the IntelliJ Platform Tree-Sitter API; cost tool window graph
  view.
- **Track D** — fire a real release on the new 5-platform matrix
  (this v0.32.0 tag *is* that release); pin the Homebrew formula's
  2 placeholder SHAs; flip `vars.PUBLISH_DOCKER` once Docker push
  lands; spin up `hassard0/asdf-mty`; submit `mty` to homebrew-core;
  strict-mode snap.
- **Track E** — `cost-delta` polish (replace the bash-only diff with
  a typed JSON walker once `mty inspect` gains `--diff`);
  `mty-explain` Slack/Discord webhook example; document the
  `error_code` output in the action reference.
- **Track F** — drop the legacy `RecorderConfig::JsonLines` variant
  entirely (the v0.32 deprecation shim stays for one cycle); expose
  `MTY_RECORD_TRACE` as a CLI flag (`mty run --record-trace`);
  surface recorded `tool_uses` in `mty inspect --cost --explain`.

## Onward

The v1.0 freeze-gate is unchanged: 8 RFC comment windows opened
2026-05-26, earliest close 2026-06-09 (RFC-005), latest close
2026-07-25 (RFC-002 + RFC-006). Proposed v1.0 freeze date
2026-09-01; earliest tag 2026-07-26.
