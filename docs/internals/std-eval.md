# `std.eval` — replay-driven LLM eval harness (internals, v0.28 → v0.29)

**Module:** `mty_stdlib::eval` (submodules `suite`, `case`, `runner`,
`compare`, `replay_glue`)
**Roadmap:** Post-v1.0 — `docs/internals/agent-features-roadmap.md`
**Mighty surface:** `use std.eval.{Suite, Case, Member, Compare}`

This document explains the internal architecture of the typed LLM-eval
driver shipped in v0.28 Track G. The driver wires the v0.21 byte-
identical replay machinery (`mty_runtime::replay::ReplayDriver`) into
a `Suite` × `Case` × `Member` matrix so an agent's behaviour can be
regression-tested against many model variants the same way unit
tests regress code.

## Module shape

```
crates/mty-stdlib/src/eval/
  ├── mod.rs        — top-level Suite/Case/Member/Compare re-exports
  │                   + `EvalError` enum
  ├── suite.rs      — `Suite` builder (`.case`/`.run_with`/`.compare`)
  ├── case.rs       — `Case` + `CaseKind::{Input, Trace}` + `CaseRun`
  ├── runner.rs     — `Runner::run_matrix` + `Runner::stamp_verdicts`
  ├── compare.rs    — `Compare` strategy + `Report` + `Verdict` +
  │                   tool-call extractor
  └── replay_glue.rs — `decode_trace_baseline` + v0.29 hook backlog
```

`Suite` is the user-facing type. Everything else is implementation
detail — `Runner` is `pub` only so integration tests in
`crates/mty-stdlib/tests/` can drive the matrix directly when needed.

## Surface

The four-noun shape mirrors `std.swarm` (one prompt, N members, one
verdict) so callers transitioning from "make a consensus call" to
"regression-test agent behaviour" only swap the verb at the end of
the builder chain:

```rust
use mty_stdlib::eval::{Case, Compare, Member, Suite};

let report = Suite::new("research-agent")
    .case(Case::from_input("What's the population of France?"))
    .case(Case::from_trace("traces/research-001.mty-trace"))
    .run_with(Member::anthropic("claude-opus-4-7"))
    .run_with(Member::openai("gpt-5"))
    .compare(Compare::semantic_similarity(0.85))
    .await?;
```

The same shape in Mighty source:

```mty
use std.eval.{Suite, Case, Member, Compare}

let report = Suite.new("research-agent")
  .case(Case.from_trace("traces/research-001.mty-trace"))
  .case(Case.from_input("What's the population of France?"))
  .run_with(Member.anthropic("claude-opus-4-7"))
  .run_with(Member.openai("gpt-5"))
  .compare(Compare.semantic_similarity(threshold: 0.85))
  .await
```

## Case sources

| Source | Constructor | Baseline column |
|---|---|---|
| Raw prompt | `Case::from_input(s)` | first non-errored member reply |
| Recorded trace | `Case::from_trace(path)` | recorded assistant reply |

The trace source decodes a lightweight v0.28 JSON-lines wire format
(one event per line, fields: `type`, `content`). Unknown event types
are silently skipped so the decoder stays forward-compatible with the
v0.29 structured trace wire format (see backlog below). Reading the
trace is deferred to `Case::resolve()` — `Suite::new(...).case(...)`
stays synchronous so the suite builder can be constructed from
non-async contexts.

## Comparison strategies

Three `Compare` variants today:

### `Compare::Equal`

Strict byte-equal after `trim().to_lowercase()`. The strictest
comparator; useful for tool-name verdicts where the agent emits a
single token, useless for free-form prose.

### `Compare::SemanticSimilarity { threshold, embedder }`

Cosine similarity over the embedder's vectors. Two replies are
equivalent iff `cos(a, b) >= threshold`. The default embedder is the
stub FNV-hash embedder from `std.memory` — bit-stable across runs +
deterministic, so eval reports reproduce across CI lanes. Callers who
want real semantic distance pass `Compare::semantic_similarity_with(t,
Arc::new(OpenAIEmbedder::new(...)))`.

The threshold is clamped into `[0.0, 1.0]` on construction — out-of-
range values would always-match or never-match and that's almost
never what the caller meant.

### `Compare::ToolCallSetEqual`

Extracts `@tool` invocations from each reply and compares the *set*
of tool names. Order-independent. Two extraction shapes today:

1. **Function-call literal:** `tool_name(arg1, ...)` anywhere in the
   reply text. Identifiers must be lower-snake-case + at least 3
   chars to keep the false-positive rate low — random prose rarely
   contains `foo(`.
2. **XML marker:** `<tool_use name="tool_name">` — the form the
   Anthropic streaming adapter emits when an assistant block was a
   `ToolUse`.

A reply with *no* tool calls compares trivially equal to any other
no-tool reply (both produce the empty set). This keeps the
comparator from spuriously failing when the model declines to use a
tool.

## Verdict + Report

The dispatch matrix is stamped cell-by-cell into a `Vec<Vec<Verdict>>`
shape:

```
        member-0   member-1   member-2
case-0   Match     Diverge    Match
case-1   Match     Match      Error
```

Verdict ∈ `Match`, `Diverge`, `Error`, `SingleMember`. The
`SingleMember` arm fires when a single-member suite has no trace
baseline — the comparator has nothing to compare against, so we stamp
that-shape rather than auto-claim a match.

The `Report` shape carries:

- `cells: Vec<Vec<Verdict>>` — the verdict matrix.
- `divergences: Vec<Divergence>` — every (case, member) cell whose
  verdict was `Diverge` or `Error`, with the baseline + actual reply
  + a free-form reason (e.g. `cosine 0.42 below threshold 0.85`).
- `total_cost_cents: u64` — sum of every cell's `cost_cents`.
- `passed()` — `true` iff every cell is `Match` or `SingleMember`.

`Report::render()` produces a multi-line human-readable diff for
CLI output.

## Dispatch + budget

```text
   Suite::compare(comparator)
        │
        ▼
   Runner::resolve_cases(&self.cases)  ◀── reads trace files off disk
        │
        ▼
   Runner::run_matrix(&cases, &members, &budget)
        │      members run in parallel inside each case row
        ▼
   Runner::stamp_verdicts(name, cases, members, matrix, comparator)
        │
        ▼
   Report { cells, divergences, total_cost_cents }
```

The driver dispatches members *in parallel* within a single case
(via `tokio::spawn`) but processes cases *sequentially*. Members
share a single `SharedDollarBudget` so an eval can be capped at a
fixed dollar ceiling (`Suite::with_budget(2.50)`); once the budget
trips, pending dispatches return `LlmError::BudgetExhausted` and the
runner stamps `Verdict::Error` on those cells. The suite still
returns a `Report` for the cells that did run — the eval isn't
abort-on-first-error.

When *every* (case, member) cell errored we surface
`EvalError::AllCellsFailed` at the suite level rather than returning
an empty-but-passing report; CI is much better served by a loud
error than by a quiet `passed() == true` with zero verdicts.

## Replay-runtime hooks

v0.28 Track G integrates the v0.21 replay machinery (the
`ReplayDriver` + the byte-identical wire format) through a thin
glue layer in `replay_glue.rs`. Two operations:

1. **`decode_trace_baseline(path)`** — read the recorded prompt +
   assistant reply out of the JSON-lines trace shim so a
   `Case::from_trace` has a baseline column.
2. **`run_trace_with_member(prompt, member, budget)`** — dispatch
   the recorded prompt against a fresh `Member`. v0.28 path: a
   straight `member.ask(prompt, budget)` call.

## Native replay (v0.29)

v0.29 Track F upgraded the integration from the JSON-lines shim to
the real `mty_runtime::replay` machinery. Four backlog items shipped:

| Backlog item | What landed |
|---|---|
| `ReplayDriver::with_provider(member)` | New method on `ReplayDriver` plus a `TurnProvider` trait — swaps the recorded LLM provider for a fresh one mid-replay. The v0.29 surface is the **LLM-only** path (walk every `TraceEvent::LlmCall`, redispatch against the live provider, collect per-turn diffs). Full re-execution `with_provider` (inside `replay_all`) is queued for v0.30 — see "v0.30 follow-ups" below. |
| `TraceFile::iter_llm_calls()` | Borrowed iterator over every recorded `TraceEvent::LlmCall` event. `std.eval` uses this to fast-path "just rerun the LLM turns" without spinning a fresh `Runtime`. |
| Trace wire v3 | New `TraceEvent::LlmCall` variant captures one LLM turn structurally: `agent`, `turn_id`, `prompt`, `system`, `tools`, `reply`, `tool_uses`, `cost_cents`. `TRACE_WIRE_VERSION` bumped 2 → 3. Additive: v2 traces still decode cleanly (`iter_llm_calls()` returns empty). |
| `mty replay --diff` | CLI gained `--diff` + `--turn <id>` flags. `--diff` alone renders a sweep over every recorded LLM turn ("turn #N : MATCH/DIVERGE"); `--turn <id>` renders the full structural diff for one turn (`LlmTurnDiff::render`). The eval driver's divergence reporter points users at this shell command. |

### New native-path surfaces

The glue layer's `replay_glue.rs` exposes the v0.29 native bridges:

| Symbol | Shape |
|---|---|
| `decode_trace_baseline_native(path)` | Read a v3 binary `.mty-trace`; return the first recorded LLM turn as the baseline. |
| `decode_baseline_auto(path)` | Sniff the 8-byte `MTYTRACE` magic; route to native or JSON-lines. `Case::from_trace` calls this so legacy + native fixtures coexist. |
| `read_binary_trace(path)` | Load the full `TraceFile`; caller iterates `iter_llm_calls()`. |
| `MemberTurnProvider` | Adapter — implements `mty_runtime::replay::TurnProvider` for a `Member`, so `Suite::compare()` can hand a panel member to `ReplayDriver::with_provider`. |

### Auto-routing

`Case::from_trace(path)` now auto-routes:

- File starts with `MTYTRACE` magic → v3 binary decoder
  (`decode_trace_baseline_native`).
- Anything else → v0.28 JSON-lines shim (`decode_trace_baseline`).

Existing eval fixtures (hand-written JSON-lines) keep working
unchanged; new recordings produced by `MTY_RECORD_TRACE` flow
through the native path without any per-call-site change.

### `MemberTurnProvider` async serialisation

`TurnProvider::provide` is sync at the trait surface (`mty-runtime`
doesn't take an async dep in its public trait), but `Member::ask`
is async. The adapter handles the async-from-sync bridge:

1. Multi-thread tokio runtime → `tokio::task::block_in_place` +
   `Handle::block_on`.
2. Current-thread / no runtime → spawn a fresh single-thread
   runtime on a dedicated OS thread, channel back the reply.

Eval drivers running under `#[tokio::main]` get the cheap path
automatically; standalone callers (CLI tools, test fixtures) pay
one short-lived OS thread per turn.

### v0.30 follow-ups

Three deep-runtime items were stubbed at the v0.29 surface but
deferred — `mty_runtime` would need a deeper refactor to land them
without scope creep:

1. **`Member::ask` returning `tool_uses` structurally** — today
   `MemberReply` only surfaces the text. The provider wraps with
   `tool_uses: vec![]`. v0.30 lifts the LLM adapter's structured
   reply through the swarm layer.
2. **`ReplayDriver::replay_all` + `with_provider` together** — full
   re-execution that intercepts every recorded LLM call inside the
   running `Runtime`. Requires a runtime-side LLM-provider
   injection point (today the runtime is opaque to `mty-stdlib::llm`).
3. **Recorder integration into `Member::ask`** — `Member::ask`
   doesn't itself call `record_llm_call` today; the recording side
   is wired in std.eval's record-mode driver only. v0.30 lifts it
   into the `LlmProvider` trait so any agent run with
   `MTY_RECORD_TRACE` captures LLM turns automatically.

See `mty_stdlib::eval::replay_glue::V029_BACKLOG` — every entry
now starts with the `[shipped v0.29]` marker; the constant is kept
for the audit trail.

## Why a fluent builder over a `struct` literal

A `Suite { cases: vec![...], members: vec![...], ... }` literal
shape would work in Rust but trips on two boundaries:

1. **Mighty source surface.** Mighty doesn't have struct literals
   for opaque ADTs — `Suite.new(...)` + chained methods is the
   shape the prelude already permits for `std.swarm.Member`,
   `std.memory.VectorStore`, `std.llm.AnthropicClient`. The fluent
   builder mirrors that.
2. **Forward-compat.** Adding a `with_concurrency_limit(n)` knob to
   the suite is a one-line `pub fn` addition; adding it to a
   struct literal would force every existing call site to either
   bind the new field or use `..Default::default()`.

## Test coverage

* `crates/mty-stdlib/src/eval/suite.rs` (15 tests): builder shape,
  budget conversion, every comparator strategy via `compare()`,
  trace-baseline divergence + match, all-members-errored path,
  multi-case multi-member matrix, single-member suite.
* `crates/mty-stdlib/src/eval/case.rs` (8 tests): input + trace
  resolution, name derivation, unicode boundary truncation,
  missing-file errors.
* `crates/mty-stdlib/src/eval/compare.rs` (17 tests): every
  strategy, threshold clamping, tool-call extraction (XML + bare
  call), report `passed()` + `render()` + `failure_count()`,
  cosine math edge cases.
* `crates/mty-stdlib/src/eval/runner.rs` (6 tests): matrix
  dispatch, error-cell capture, verdict stamping, semantic-divergence
  explanation includes cosine score.
* `crates/mty-stdlib/src/eval/replay_glue.rs` (15 tests): baseline
  decode, malformed JSON, missing file, missing user prompt, mock
  dispatch round-trip, v0.29 backlog shipped-marker, native v3
  binary trace decoder, auto-routing (binary + JSON-lines),
  `read_binary_trace`, `MemberTurnProvider` against mock + error
  members.
* `crates/mty-stdlib/src/eval/mod.rs` (2 tests): empty-suite +
  no-members error paths.

68 tests total in `mty-stdlib`. Native replay machinery adds 18
tests in `mty-runtime/src/replay/*` (8 new for `iter_llm_calls` +
`with_provider` + `diff_llm_turn` + wire-v3 round-trips, plus the
v0.29 recorder hook). CLI ships 5 new tests for `mty replay --diff`.
All pass via `cargo test --workspace`.

## See also

* `docs/internals/replay.md` — the v0.21 byte-identical replay
  machinery the eval driver builds on.
* `docs/reference/stdlib/swarm.md` — `std.swarm`, sibling
  multi-LLM primitive sharing the same `Member` enum.
* `docs/reference/stdlib/llm.md` — the typed LLM provider surface
  `Member` wraps.
* `docs/reference/stdlib/memory.md` — the `Embedder` trait the
  semantic-similarity comparator uses.
* `examples/31_eval_agent.mty` — minimal Mighty-source example
  (v0.28 JSON-lines shim).
* `examples/32_eval_native.mty` — v0.29 native-replay-backed
  example (binary `.mty-trace` via `MTY_RECORD_TRACE`).
* `dev/history/notes/STD_EVAL_V0_28_NOTES.md` — design rationale
  (track-G ship notes; populated by the integrator).
