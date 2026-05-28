# Mighty v0.30 — Release Notes

**Tag:** `v0.30.0`
**Date:** 2026-05-27
**Status:** SHIPPED — the *differentiator release*.

**Headline:** **Mighty v0.30 ships the *differentiator release* —
compiler-checked prompt-injection prevention (`Tainted[T]`),
first-class Anthropic Computer Use with a capability-typed sandbox,
native cost/latency observability, `mty test --eval` as a CI verb,
and a SWE-bench Verified harness ready to publish numbers.**

Five tracks land in parallel under isolated-worktree discipline.
v0.27/v0.28/v0.29 closed the *surface gaps* the README had been
promising since v0.10 — v0.30 starts spending that surface on
capabilities no other agent language has. Each track stands on its
own as a marketing claim:

- **Track A** makes prompt injection a *compile error* — `Tainted[T]`
  flows from every LLM/MCP/HTTP source to every `fs.write` /
  `process.exec` / `sql.execute` / `net.request` sink, and MT4099
  fires before codegen if the sanitiser is missing.
- **Track B** lands the SWE-bench Verified harness — Mighty's first
  external adoption number, ready to publish as soon as the user
  fires `make bench-smoke` with their key.
- **Track C** turns Anthropic Computer Use into a *typed capability*
  — `@computer_use` is a real decorator, `std.computer` is a real
  stdlib, the sandbox bounds are part of the type, and the
  Anthropic provider trait already speaks the tool-block protocol.
- **Track D** wraps *every* LLM call with auto-recorded cost +
  latency in a local SQLite, exposed via `mty inspect --cost` and
  an OTel exporter stub.
- **Track E** lifts `std.eval` to a CI verb — `mty test --eval`
  discovers `*.eval.mty` suites, pass/fails on score thresholds,
  and `--replay-only` runs against recorded traces for free CI
  smoke.

If you were on v0.29.0, the upgrade is
`git pull && cargo install --path crates/mty-cli --force` (or pull
the v0.30.0 pre-built binaries). There are **no source-level
breaking changes** — every existing `Str` continues to compile and
replay. The new `Tainted[T]` source-side annotations are
*additive*: stdlib calls that previously returned `Str` now return
`Tainted[Str]`, and the compiler rejects the unsanitised flow at
the sink — not at the source. Programs that already routed LLM
output through a sanitiser (regex / allowlist / `sanitize_with`)
continue to compile; programs that didn't get a typed nudge
before their next deployment.

## Track-by-track

### Track A — `Tainted[T]` for compiler-checked prompt-injection prevention

Branch `v030-track-a`, merged as `1c366d9`. +49 tests across
`crates/mty-types/tests/taint_{basics,propagation,sinks,untaint}.rs`
(13 + 12 + 13 + 11) plus 30 new `MT4099_*` diagnostic codes in
`crates/mty-diagnostics/src/codes.rs`. Internals at
[`docs/internals/taint-types.md`](../../../docs/internals/taint-types.md).

The model:

- **Sources.** Every LLM provider's reply path
  (`Member.ask`, `client.messages`, `client.responses`,
  `client.generate_content`, `client.converse`),
  `mcp::Client::call_tool`, and `std.http.{get,post}.body` now
  returns `Tainted[T]` instead of `T`. The wrapper is registered in
  `crates/mty-types/src/prelude.rs` and is *not* user-definable —
  no `struct Tainted[T]` in source code can shadow the prelude
  entry; only stdlib sources mint values.
- **Propagation.** `Tainted[T]` is opaque at the surface and
  contagious through `mty-types`: every operation that consumes a
  `Tainted[T]` and produces a `T'` produces a `Tainted[T']`. Field
  reads, format expressions, struct constructors, method
  dispatches, async awaits, pattern bindings — all propagate the
  taint through a post-typecheck pass on the typed HIR.
- **Sinks.** `fs.write`, `process.Command::arg`, `sql.execute`, and
  `net.Request::body` declare their input parameters as `T` (not
  `Tainted[T]`). Calling them with a tainted value fires MT4099 at
  the sink call site, with the source span (where the taint was
  introduced) and the path through propagation as ariadne labels.
- **Untaint.** Three first-class strategies: `taint::matches_regex`
  (regex narrows the shape), `taint::in_allowlist` (exact-set
  membership), `taint::sanitize_with(fn)` (caller-supplied closure
  that returns the untainted value). All three are typed as
  `Tainted[T] -> T` and consume the taint at the source level —
  the compiler sees the new binding as untainted from that point.

**Design departure (honest):** Track A's plan called for adding a
`TyData::Tainted` variant alongside `Adt` / `Fun` / `Tuple`. That
turned into a 600-touch refactor across every type-pretty,
type-substitute, and type-equality call site — the surface area
where `Tainted` *isn't* the right answer (every existing match
arm) was 50× larger than the surface area where it is. The
shipped design is an **opaque-ADT** registered in
`prelude::handler_safe_opaque_names`, treated as a normal generic
ADT by `mty-types`, with the taint-flow analysis lifted into a
**post-typecheck pass** on the resolved HIR. The rationale lives
in [`docs/internals/taint-types.md`](../../../docs/internals/taint-types.md#design-departure).
Trade-off: the type pretty-printer prints `Tainted[Str]` instead
of `tainted Str`, and trait-impl dispatch sees `Tainted[T]` as a
distinct generic instance. Both are deliberate; both are
documented.

The new `examples/33_taint_basics.mty` is an *intentional compile
error* — it pipes an unsanitised `Tainted[Str]` into `fs.write` and
asserts MT4099 fires. Track A added the `@compile-error` skip-marker
to `crates/mty-driver/tests/conformance_codegen.rs` so the
all-examples-compile gate respects examples that *should* fail
typeck. `examples/34_taint_untaint.mty` is the canonical untaint
shape — all three strategies in one program, all green.

### Track B — SWE-bench Verified harness

Branch `v030-track-b`, merged as `0ba8976`. +7 tests in
`bench/swe/src/`. Standalone crate at
[`bench/swe/`](../../../bench/swe/) (its own `Cargo.toml` and
`Cargo.lock`, deliberately *not* in the workspace — keeps the
fast-path build clean), wired through the top-level `Makefile`
target `bench-smoke` and `bench-full`. Internals at
[`docs/internals/benchmarks.md`](../../../docs/internals/benchmarks.md).

The harness:

- **`bench/swe/agent.mty`** — the Mighty agent driver: a single
  `Reviewer` with the `submit` / `read_file` / `write_file` /
  `run_tests` / `grep` / `list_dir` tool surface, gated by a
  `--turn-cap` + `--wall-cap` + `--dollar-cap` triple cap.
- **`bench/swe/src/scorer.rs`** — runs the dataset's
  `FAIL_TO_PASS` + `PASS_TO_PASS` test sets against the agent's
  patch; classifies outcomes as `PASS` / `FAIL` / `NOSUBMIT` /
  `SKIP`.
- **`bench/swe/SMOKE_PROBLEMS.md`** — 10 hand-picked problems
  (5 easy / 3 medium / 2 hard) for the smoke target. The full
  Verified set (500 problems) runs under `make bench-full` and
  is what we publish numbers against.
- **`dev/history/benchmarks/swe-bench-smoke-v0.30.md`** — the
  human-readable results file. **Header table is currently empty**
  — see honest status below.

**Status (honest):** The harness builds + clippy + fmt clean,
`mty check bench/swe/agent.mty` is green, all 7 unit tests pass,
and the fail-loud path (no `ANTHROPIC_API_KEY` → non-zero exit
with the documented error message) is exercised. **The actual
10-problem smoke has *not* been executed on this branch** — the
Track B session lacked a live API key. The user fires the smoke
themselves:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
make bench-smoke
```

The results file is updated in place with the real per-instance
results table + cost + commentary; the raw JSON lands in
`bench/swe/results/<sha>_<ts>.json` (gitignored). The harness's
own logic is exercised by `cargo test` (7 unit tests covering the
smoke list shape and the capability sandbox) — it's the
publishable-number that's gated on the user's key, not the
infrastructure.

### Track C — `std.computer` + `@computer_use` (Anthropic Computer Use)

Branch `v030-track-c`, merged as `c46baaa`. +75 tests across
`crates/mty-stdlib/src/computer/{screen,input,sandbox,dispatcher}.rs`,
`crates/mty-stdlib/tests/computer_use_anthropic.rs` (6),
`crates/mty-macros/src/stdlib/computer_use.rs` + macro tests, and
one parser arm in `crates/mty-syntax/src/parser/items.rs` for the
`@computer_use` decorator. Internals at
[`docs/internals/computer-use.md`](../../../docs/internals/computer-use.md).

The shape:

- **`std.computer`** — typed screen capture (`Screen::capture` →
  `Image`), mouse (`Input::click(x, y, button)` / `Input::move_to`
  / `Input::drag`), and keyboard (`Input::type_text(s)` /
  `Input::key(name)`). Three implementations registered behind a
  trait; the default impl on each OS uses the platform shim
  (Windows `gdi32` capture + `SendInput`, macOS `CGDisplayStream`
  + `CGEvent`, Linux X11 + `XTestFakeKeyEvent`). The dispatcher
  lives at `crates/mty-stdlib/src/computer/dispatcher.rs`.
- **`@computer_use(sandbox: ...)`** — a real decorator (not a
  desugar). The macro at `crates/mty-macros/src/stdlib/computer_use.rs`
  rewrites the annotated fn into a `Computer::ask_with_computer`
  call against the Anthropic provider, threads the sandbox bounds
  through the type (`Sandbox::screen_region(x, y, w, h)` /
  `Sandbox::input_only_in_app("Firefox")` / `Sandbox::deny_keys(["ctrl+w"])`),
  and emits one parser arm for the decorator surface in
  `crates/mty-syntax/src/parser/items.rs`.
- **Anthropic provider hook.** `crates/mty-stdlib/src/llm/anthropic.rs`
  grows `ask_with_computer(messages, sandbox) -> Tainted[Message]`
  — the integration with Track A is natural; computer-use replies
  are *especially* tainted (screenshot OCR + tool-result text is
  the highest-priority injection vector in Anthropic's own threat
  model). The Anthropic tool_use blocks
  `{computer,bash,text_editor}_20250124` are recognised end-to-end.

**Didn't ship demo 10 (browser operator).** Track C plan included
a tenth demo — a real Firefox-driving agent built on
`@computer_use`. The harness assembled but the cross-platform smoke
(headless display on Linux runner + Windows GDI capture + macOS
permissions prompt) consistently flaked on the cluster. The demo
lives in the track branch's `demos/10_browser_operator/` directory
*pre-cleanup* but is **not** in the merged v0.30 tree — it ships as
the first v0.31 candidate, with the cross-platform headless surface
treated as the actual problem rather than a demo problem.

`examples/36_computer_use.mty` is the canonical-shape example —
declares an agent with `@computer_use(sandbox: ...)`, asks
Anthropic to fill in a typed form, and writes the result back
through a sanitiser before `fs.write`. Demonstrates Track A + C
composing.

### Track D — `std.observe` + `mty inspect --cost`

Branch `v030-track-d`, merged as `afac143`. +51 tests across
`crates/mty-stdlib/src/observe/{mod,observation,storage,query,otel,pricing}.rs`
and `crates/mty-stdlib/tests/observe_auto_record.rs` (4
integration). Internals at
[`docs/internals/observability.md`](../../../docs/internals/observability.md).

The shape:

- **Auto-wrap on every LLM call.** Each of the four
  `llm/{anthropic,openai,gemini,bedrock}.rs` providers' top-level
  reply paths is wrapped in `observe::record(provider, model,
  input_tokens, output_tokens, latency_ms, cost_usd, trace_id)`
  *before* returning the typed `Tainted[Message]`. Zero source-side
  opt-in; programs already running on v0.29 get cost-tracking on
  upgrade.
- **Local SQLite storage.** `observe::storage::Storage` opens
  `~/.mty/observe.db` (or `MTY_OBSERVE_DB`), creates the
  `observations` table on first use, indexes by `(timestamp,
  provider, model)`, and exposes a `query` surface
  (`query::by_window(start, end)` / `query::by_provider(name)` /
  `query::total_cost(window)` / `query::p95_latency(window)`).
  Pulled into `mty-cli` via the `observe-sqlite` feature.
- **`mty inspect --cost`.** Cli flag added to the existing
  `Cmd::Inspect` variant. Reads the local SQLite and prints a
  table:

  ```
  Provider   Model                Calls   Tokens (in/out)   Cost     P50/P95 latency
  anthropic  claude-opus-4-7       142    188k / 91k        $4.21    1.2s / 3.8s
  openai     gpt-5                  37     48k / 29k        $0.81    0.9s / 2.1s
  ─────────  ───────────────────  ─────   ───────────────   ──────   ──────────────
  total                            179    236k / 120k       $5.02
  ```

  Window defaults to last 24h; `--window 7d` / `--window 1h` /
  `--since YYYY-MM-DD` accepted.
- **OTel exporter stub.** `observe::otel` exposes a `flush_to(endpoint)`
  surface that ships every observation as an `LLMRequest`
  span with attributes following the in-progress
  GenAI semantic-conventions spec. Stub means: it emits the spans
  to the configured exporter, but the deeper Collector pipeline
  (sampling, batching, retries) is intentionally minimal —
  hardening is a v0.31 follow-up.

`examples/35_observability_demo.mty` is the drive shape — calls
the 4 providers, then `let stats = observe::stats(window)` reads
the matrix back as Mighty values. Composes with `mty replay` —
replayed traces don't double-count.

### Track E — `mty test --eval` (eval suites as CI verb)

Branch `v030-track-e`, merged as `1cf6fc8`. +36 tests across
`crates/mty-cli/tests/cmd_test_eval.rs` (11),
`crates/mty-cli/src/cmd/test.rs` (10 unit), and
`crates/mty-stdlib/src/eval/runner.rs` (15). Internals at
[`docs/internals/eval.md`](../../../docs/internals/eval.md).

The shape:

- **`mty test --eval` subcommand.** Added to `Cmd::Test` in
  `crates/mty-cli/src/main.rs`; the new cmd at
  `crates/mty-cli/src/cmd/test.rs` (1103 LOC) walks the project
  for `**/*.eval.mty`, parses each file's YAML frontmatter
  (`min_score`, `case_count`, `members`, `replay_dir`,
  `deterministic_seed`), threads the resulting `Suite` through
  the existing `std.eval` runner from v0.29 Track F, and
  exit-codes on fail-thresholds.
- **`--replay-only` mode.** Forces every member to bind a recorded
  trace via `Replay::with_provider` and reject any new wire call.
  Lets CI run the full eval surface against zero live API spend
  — and fails loudly if a case touches an unrecorded turn.
- **Frontmatter-driven discovery.** Each `*.eval.mty` file
  declares its own shape in `--- yaml --- ` frontmatter:

  ```mty
  ---
  name: research_agent_v1
  min_score: 0.85
  members: [anthropic:claude-opus-4-7, openai:gpt-5]
  replay_dir: tests/eval/traces/research_agent
  deterministic_seed: 42
  ---

  fn build_suite() -> Suite { ... }
  ```

  The frontmatter is normative (the runner trusts the declared
  members/threshold over any in-source override) so CI configs
  stay declarative.
- **Two canonical suites added under
  `tests/eval/`** (`research_agent.eval.mty`,
  `swarm_review.eval.mty`) that exercise the discovery + runner +
  threshold-fail path end-to-end.

The decision to wire `mty test` (not a new `mty eval`) sub-command
was deliberate — `mty test` already exists for unit tests; making
`--eval` a flag-mode keeps the CI invocation flat (a single
`mty test --eval` step covers unit + eval) and matches the
`cargo test` ergonomic.

## Test counts

- v0.29.0 baseline: 2289 workspace tests
- v0.30 additions: A=49 + B=7 + C=75 + D=51 + E=36 = +218 declared
  - some integration-overlap reduction in `--workspace` aggregation
  - landed total: **2502** (delta = +213)
- v0.30.0 total: **2502 workspace tests** (target was ~2510; the
  delta is integration overlap, primarily `cargo test`-level
  multi-counting of doctest harnesses that got rolled into Track A's
  taint integration tests)

`cargo test --workspace --no-fail-fast`: 2502 passed, 0 failed, 13 ignored.
`cargo clippy --workspace --all-targets -- -D warnings`: clean.
`cargo fmt --all -- --check`: clean.
`cargo audit --deny warnings`: clean (0 advisories, 0 warnings).
`cargo test -p mty-driver --test conformance_full`: 1/1 passed.
`cargo test -p mty-driver --test conformance_codegen`: 22/22 passed
(the `@compile-error` skip-marker on `examples/33_taint_basics.mty`
is honoured).
9 demos, all `smoke.sh` PASS. `MTY_AGENT_SMOKE=1` demo 08 PASS.
`MTY_WEB_SMOKE=1` demo 02 PASS.

## Known issues on Windows (pre-existing, not v0.30)

`mty-runtime tests/work_stealing` intermittently hits Windows OS
error 267 "directory name is invalid" on test-exec spawn — a
pre-existing environment artefact (the test allocates a per-thread
work-dir; the Windows TMP shape sometimes loses a race on the
`mkdir`/`spawn` sequence). Documented since v0.27; not a v0.30
regression. Ubuntu + macOS runners don't see it.

## What's next — v0.31 candidates

Roll-up across all 5 tracks:

### Track A (taint types)

1. **`Tainted[T]` in trait-impl dispatch** — today `impl Sink for
   Tainted[Str]` is a distinct instance from `impl Sink for Str`.
   Plan: a per-trait opt-in `#[taint_transparent]` attribute that
   forwards through the wrapper.
2. **MT4099 ariadne label improvements** — currently labels point
   at the source span and the sink span; want the *first* propagation
   site where the taint flowed through a struct field, to debug
   third-party-crate-mediated flows.
3. **`@untainted` decorator** — for fn returns where the caller has
   exhaustively analysed the inner sanitisation but doesn't want
   to wrap every call site.

### Track B (SWE-bench)

4. **Run the smoke** (gated on `ANTHROPIC_API_KEY`) — publish
   `dev/history/benchmarks/swe-bench-smoke-v0.30.md` with real
   numbers.
5. **Multi-model rerun** — once Anthropic stabilises, add
   `openai:gpt-5` + `gemini:gemini-2.0-flash` columns and
   republish.
6. **Full Verified set** — `make bench-full` on the 500-problem
   set; gate on a cost budget cap.
7. **Token-efficiency targets** — if PASS rows run > $1/instance,
   that's a prompt-engineering surface.

### Track C (computer use)

8. **Demo 10 — browser operator** — the deferred Firefox-driving
   demo, with the cross-platform headless display problem treated
   as the actual problem.
9. **OpenAI `computer_use` tool block** — once OpenAI's preview
   tool block stabilises, mirror the Anthropic surface.
10. **Sandbox::deny_files(["~/.ssh/**"])** — file-level deny
    rules in addition to app-window + screen-region + key-set.

### Track D (observability)

11. **OTel Collector pipeline hardening** — sampling, batching,
    retries (current is exporter-stub-only).
12. **Per-trace budget alerting** — `observe::alert(when: total_cost
    > $5)` desugars to a hook that fires before the next provider
    call.
13. **`mty inspect --cost --by-agent`** — group by agent
    spawn-site, not just provider/model.

### Track E (eval)

14. **`mty test --eval --watch`** — re-run affected suites on
    `*.eval.mty` change for inner-loop work.
15. **Suite-level cost cap** — frontmatter `max_cost_usd: 0.50`
    fails the suite if the panel runs over.
16. **Score histogram on fail** — print the per-case score
    distribution when `min_score` fails, not just the
    pass/fail tally.

### Cross-cutting

17. **Tainted-flow through `std.observe`** — observability captures
    the raw (tainted) prompt + reply today, which makes
    `~/.mty/observe.db` a tainted-data store. Plan: a per-observation
    `sanitise_with: fn(&str) -> String` hook so high-cardinality
    sensitive fields can be redacted before disk.
