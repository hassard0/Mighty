# Mighty v0.28 — Release Notes

**Tag:** `v0.28.0`
**Date:** 2026-05-27
**Status:** SHIPPED — Track G only; Tracks A–E deferred to v0.29.

**Headline:** **Mighty ships `std.eval`: byte-identical-replay-based
LLM evals as a typed stdlib surface — the "regression-test agents
like any other code" capability the README promises is now real.**
A Mighty program can now declare a `Suite::new("research-agent")`,
attach `Case`s (from raw input, from a recorded `.mty-trace`, or
from a saved transcript), fan the suite across a panel of
`Member`s (Anthropic / OpenAI / Gemini / Bedrock — any subset of
the four v0.27 typed providers, mixed freely), pick a `Compare`
strategy (byte-equal-after-trim-lower, semantic-cosine over the
`std.memory::Embedder`, or order-independent tool-call-set
equality), and read back a per-(case, member) verdict matrix plus
per-divergence rows. The runner shares a `SharedDollarBudget`
across members within a case so the whole suite stays under one
cost cap.

The v0.27 status was "feature-complete LLM-agent stdlib"; the
v0.28 status is "agents become testable code-level artefacts."
The shape that arrived in v0.21 (byte-identical replay) and got
the human-facing surface in v0.27 (typed providers + `std.swarm`)
now has a typed regression-test harness on top — agents move from
"hard to evolve safely" into the same category as every other
typed Rust crate in the workspace.

If you were on v0.27.1, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.28.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The `std.eval` module is
purely additive on top of `std.llm` / `std.memory` / `std.swarm`;
existing programs continue to compile, replay, and run unchanged.

## What didn't ship — Tracks A–E deferred

v0.28 was dispatched as **six** parallel tracks (A–E in-tree
gap-closures harvesting v0.27 Track F's 5 follow-ups + Track G new
`std.eval`). All six worktrees ran on a shared `target/`
directory; disk pressure + cargo target-dir contention killed the
five in-tree tracks mid-build. They had substantive edits but no
verified commits, so v0.28 ships **Track G only** and rolls the
other five forward as v0.29 backlog (their unverified worktrees
were discarded — work re-starts cleanly under v0.29's isolation
discipline). The deferred list, with the v0.27 follow-up sources:

1. **Track A** — `BuiltinId::Swarm` interpreter arm so `mty run`
   exercises the real Tokio-backed swarm rather than the IR's
   stub-Unit builtin (v0.27 Track F follow-up #1).
2. **Track B** — handler-safe carve-out additions to the v0.27
   Track B 12-ADT table: `ConsensusStrategy`, `Member`,
   `DollarBudget`, `Consensus` (v0.27 Track F follow-up #2).
3. **Track C** — typed bang-send return-type lowering so
   `Review(s: Str) -> Str` reaches call sites as `Str` not `Unit`
   (v0.27 Track F follow-up #3).
4. **Track D** — `while let` parser surface + finish v0.27 Track
   E's source-level streaming surface (`for chunk in stream
   { ... }` desugaring).
5. **Track E** — `budget` soft-keyword demotion + per-provider
   `*_BASE_URL` env vars consulted by Track C's `from_env`
   (v0.27 Track F follow-ups #4 + #5).

Plus the **4 replay-runtime hooks Track G surfaced** while wiring
`std.eval` against the v0.21 byte-identical-replay infra — kept in
`mty_stdlib::eval::replay_glue::V029_BACKLOG` so the docs page +
the source stay in sync:

1. `Replay::with_provider(member)` constructor on `ReplayDriver`
   that swaps the recorded `LlmProvider` mid-replay so `std.eval`
   can byte-replay a multi-turn trace + only divert the LLM
   calls.
2. `RecordedTrace::iter_llm_calls()` accessor so `std.eval` can
   fast-path "just rerun the LLM turns" without spinning a fresh
   `Runtime`.
3. Trace wire **v3** capturing LLM request+response shapes
   structurally (prompt + system + tools + reply text +
   tool_uses).
4. `std.eval` divergence reporter integration with `mty replay
   --diff` so eval failures point back at the exact recorded
   turn.

v0.28's `std.eval` works around (1)+(2) for now by reading a
lightweight JSON-lines trace shape via
`replay_glue::decode_trace_baseline`; upgrading to the v3 wire
format is a drop-in once v0.29's integrator pass lands these
hooks.

## Highlights

- **`std.eval` ships full** — Track G shipped a 6-file module
  (~1700 LOC across `case.rs` / `compare.rs` / `mod.rs` /
  `replay_glue.rs` / `runner.rs` / `suite.rs`), typed `Suite` /
  `Case` / `Member` / `Compare` / `Verdict` / `Divergence` /
  `Report` surface, the runner that fans cases across members
  in parallel under a shared dollar budget and stamps verdicts
  per (case, member) cell, three comparators (byte-equal,
  semantic-cosine, tool-call-set), plus
  `examples/31_eval_agent.mty` and `docs/internals/std-eval.md`.
  60 new unit tests (suite=15, case=8, compare=17, runner=6,
  replay_glue=7, top-level=2, plus a tool-call extractor that
  handles both bare `foo(...)` calls and `<tool_use
  name="...">` XML markers).
- **Tracks A–E deferred to v0.29.** All five rolled forward
  honestly — see "What didn't ship" above. Their unverified
  worktrees were discarded; v0.29 re-starts under isolated
  worktrees (one `target/` per track) per the v0.27.x swarm
  efficiency lessons.
- **KNOWN_ISSUES net: 0.** No new entries this slice; P2 #9
  (demo 06 RAF-mid-frame phash flake, 4-of-5 success) stays open
  — not a v0.28 regression and not a required-gate blocker. P1
  stays empty.
- **v1.0 freeze gate status: unchanged structurally.** Blockers
  #1 + #3 stay CLOSED. Blocker #2 (8 RFC comment windows)
  infrastructure stays live; earliest possible v1.0.0 tag
  remains **2026-07-26**. Conformance kit stable at **159
  cases** (the v0.28 surface is stdlib, not normative).
  Rust test count grows **2125 → 2187** (+62; Track G +60 unit
  + 2 doctests). Python stable at **490**. Self-host driver
  stable at **23**. Combined (with 159 conformance cases):
  **2859** (+62 vs v0.27).
- **Eight demos** with `smoke.sh` — unchanged from v0.27. Demo
  08 still uses v0.27 workarounds for the 5 deferred tracks
  (`Plurality`-strategy mock-LLM panel through the swarm
  library tests rather than the interpreter arm); the
  `MTY_AGENT_SMOKE=1` mock-LLM pipeline markers continue to
  pass. The interpreter-side v0.29 work surfaces the real
  swarm under `mty run`.
- **Integrator fixes (this tag commit):** `cargo fmt`
  Windows-CRLF discipline applied to `examples/31_eval_agent.mty`
  (converted from CRLF to LF before tag — the same Linux/Windows
  fmt-drift pattern that caused the v0.27.0 hotfix on examples
  28 + 29). No source-level changes outside the Track G commit
  + the three release-paperwork files (CHANGELOG, README,
  this file).

## Per-track results

### Track G — `std.eval` replay-driven LLM eval harness (SHIPPED-FULL)

`std.eval` is the post-v1.0 "regression-test LLM agents like any
other code" capability the README's Why-Mighty section promised.
The module sits on top of v0.21 byte-identical replay + v0.27's
typed provider stack; surface:

```mighty
Suite::new("research-agent")
  .case(Case::from_input("..."))
  .case(Case::from_trace("traces/research-001.mty-trace"))
  .run_with(Member::anthropic("claude-opus-4-7"))
  .run_with(Member::openai("gpt-5"))
  .compare(Compare::semantic_similarity(0.85))
  .await
```

**Three comparators:**

- `Compare::equal()` — byte-equal after trim + lower-case.
- `Compare::semantic_similarity(threshold)` — cosine distance
  over `std.memory::Embedder` outputs; threshold is the minimum
  cosine to count as a match.
- `Compare::tool_call_set_equal()` — order-independent set
  comparison of `@tool` invocations; the extractor handles both
  bare `foo(arg=...)` Python-style calls and `<tool_use
  name="..." input="{...}">` XML markers as emitted by the v0.27
  Anthropic + Bedrock streamers.

**The runner** dispatches members in parallel within each case
(sharing a `SharedDollarBudget`), then stamps a `Verdict` per
(case, member) cell (`Match` / `Diverge` / `Error` /
`SingleMember`), accumulates `Divergence` rows for the `Report`,
and surfaces suite-level errors only when the suite is
configured wrong (`EmptySuite`, `NoMembers`) or every cell
failed (`AllCellsFailed`). A single divergence is reported in
the matrix rather than aborting the eval — agents diverge by
design across providers; the eval's job is to surface that, not
to fail loud.

**The `Member` enum is re-exported from `std.swarm`** so an eval
panel and a `swarm(...)` consensus call share the same provider
abstraction — no per-cell glue.

**v0.29 replay-runtime backlog** kept in
`mty_stdlib::eval::replay_glue::V029_BACKLOG` (4 hooks listed
in "What didn't ship" above; the docs page at
`docs/internals/std-eval.md` mirrors them so the source + docs
stay in sync as v0.29 lands the integrator pieces).

60 new tests (suite=15, case=8, compare=17, runner=6,
replay_glue=7, top-level=2 + a tool-call extractor).
`cargo test -p mty-stdlib --lib --all-features` reports **265
passed** (205 existing + 60 new). Clippy + fmt clean.

Files in the commit:

- **+** `crates/mty-stdlib/src/eval/{mod,suite,case,runner,compare,replay_glue}.rs`
- **M** `crates/mty-stdlib/src/lib.rs` (`pub mod eval` + module doc)
- **+** `examples/31_eval_agent.mty` (`mty check` + `mty fmt --check` clean)
- **+** `docs/internals/std-eval.md` (~260 lines)
- **M** `docs/internals/agent-features-roadmap.md` (one-line "shipped" row)

### Track A — `BuiltinId::Swarm` interpreter arm (DEFERRED → v0.29)

v0.27 Track F follow-up #1. Worktree existed at
`stardust-v028-A` with substantive edits but no verified commit;
discarded under the cleanup pass. Rolled forward to v0.29 under
isolated-worktree discipline.

### Track B — handler-safe carve-out for swarm ADTs (DEFERRED → v0.29)

v0.27 Track F follow-up #2 — adds `ConsensusStrategy` / `Member`
/ `DollarBudget` / `Consensus` to the v0.27 Track B 12-ADT
handler-safe table. Worktree at `stardust-v028-B`, abandoned,
discarded.

### Track C — typed bang-send return-type lowering (DEFERRED → v0.29)

v0.27 Track F follow-up #3 — `Review(s: Str) -> Str` should
reach call sites as `Str` not `Unit`; the bang-send currently
drops protocol return types. Worktree at `stardust-v028-C`,
abandoned, discarded.

### Track D — `while let` parser + source-level streaming (DEFERRED → v0.29)

v0.27 Track E partial — the runtime-side `MessageStream::next()`
exists; surfacing it as `for chunk in stream { ... }` in source
needs `while let` pattern desugaring in the parser. Worktree at
`stardust-v028-D`, abandoned, discarded.

### Track E — `budget` soft keyword + provider `*_BASE_URL` (DEFERRED → v0.29)

v0.27 Track F follow-ups #4 + #5 — demote `budget` from
reserved keyword to soft keyword (so `let budget =
SharedDollarBudget::new(...)` parses as a local binding); add
per-provider `ANTHROPIC_BASE_URL` / `OPENAI_BASE_URL` /
`GOOGLE_BASE_URL` / `AWS_ENDPOINT_URL_BEDROCK` env-var lookup
to `from_env` so a mock-LLM sidecar covers all four providers.
Worktree at `stardust-v028-E`, abandoned, discarded.

## Working agreements & integrator notes

- **Shared `target/` is unsafe under parallel swarm dispatch.**
  v0.28's only structural lesson: A–E all wrote to one
  `target/` and contended on cargo lockfiles + linker output
  paths. v0.29 re-runs them under per-track `CARGO_TARGET_DIR`
  isolation. The v0.27 swarm-efficiency lessons (worktree
  isolation default + file:line ownership + recon-before-swarm)
  apply here in full.
- **CRLF / LF discipline still applies.** Track G's
  `examples/31_eval_agent.mty` shipped with CRLF line endings
  (Windows worktree default); the integrator pass converted it
  to LF before tag, head-off the v0.27.0-style Linux fmt-drift
  failure pattern.
- **`cargo audit` clean.** `std.eval` adds no new crate
  dependencies — the comparators reuse `std.memory::Embedder`
  (already in v0.27) and the tool-call extractor is hand-rolled
  string-machinery.
- **Selfhost driver stable** — no v0.28 changes to the selfhost
  compiler's surface; the 23 codegen tests stay green.

## v0.29 candidate tracks

Confirmed for v0.29:

1. **Track A** (re-dispatch): `BuiltinId::Swarm` interpreter arm.
2. **Track B** (re-dispatch): handler-safe carve-out for
   `ConsensusStrategy` / `Member` / `DollarBudget` / `Consensus`.
3. **Track C** (re-dispatch): typed bang-send return-type
   lowering.
4. **Track D** (re-dispatch): `while let` parser + finish
   source-level streaming.
5. **Track E** (re-dispatch): `budget` soft keyword + provider
   `*_BASE_URL`.
6. **Replay hook 1**: `Replay::with_provider(member)`.
7. **Replay hook 2**: `RecordedTrace::iter_llm_calls()`.
8. **Replay hook 3**: Trace wire v3 (structural LLM
   request+response capture).
9. **Replay hook 4**: `std.eval` divergence reporter ↔ `mty
   replay --diff` integration.

Plus any RFC comment-window feedback that lands; RFC-005 closes
earliest (2026-06-09).

## Test count delta from v0.27

| Bucket          | v0.27.1 | v0.28.0 | Δ   |
|-----------------|---------|---------|-----|
| Rust            |    2125 |    2187 |  +62|
| Python 2nd-impl |     490 |     490 |   0 |
| Self-host       |      23 |      23 |   0 |
| Conformance     |     159 |     159 |   0 |
| **Combined**    |  **2797**| **2859**|**+62** |

Per-track: G +62 (60 unit + 2 doctests). A / B / C / D / E
deferred — their would-be deltas roll forward to v0.29.

## Demos status

8 demos, all green:

| Demo | Status | Notes |
|------|--------|-------|
| 01_search_api | PASS | |
| 02_counter_web | PASS | `MTY_WEB_SMOKE=1` PASS — phash within tol |
| 03_extract_tool | PASS | |
| 04_kvstore | PASS | |
| 05_notetris_web | PASS | `MTY_WEB_SMOKE=1` PASS — phash within tol |
| 06_canvas_game | PASS | `MTY_WEB_SMOKE=1` PASS — phash within tol |
| 07_research_agent | PASS | `MTY_AGENT_SMOKE=1` PASS (mock-LLM end-to-end; opaque-handle wiring still a v0.27 follow-up) |
| 08_swarm_review | PASS | `MTY_AGENT_SMOKE=1` PASS (mock-LLM pipeline markers; uses v0.27 workarounds for the 5 deferred tracks) |

## Upgrade

```bash
git pull
cargo install --path crates/mty-cli --force
```

Or pull binaries from
<https://github.com/hassard0/Mighty/releases/tag/v0.28.0>.

— Integrator pass, 2026-05-27
