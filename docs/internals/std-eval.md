# `std.eval` — replay-driven LLM eval harness (internals, v0.28)

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
   assistant reply out of the trace file so a `Case::from_trace`
   has a baseline column.
2. **`run_trace_with_member(prompt, member, budget)`** — dispatch
   the recorded prompt against a fresh `Member`. Today this is a
   straight `member.ask(prompt, budget)` call.

The full byte-identical replay-driver integration (where the eval
driver feeds the recorded trace into a fresh `Runtime` + only
diverts the LLM provider calls to the new member) is queued for
v0.29:

| Backlog item | What lands |
|---|---|
| `Replay::with_provider(member)` | Constructor on `ReplayDriver` that swaps the recorded `LlmProvider` mid-replay. |
| `RecordedTrace::iter_llm_calls()` | Accessor so `std.eval` can rerun just the LLM turns without spinning a fresh `Runtime`. |
| Trace wire v3 | Captures LLM request+response shapes structurally (prompt + system + tools + reply + tool_uses). |
| `mty replay --diff` integration | Eval divergence reporter points back at the exact recorded turn. |

The eval driver works against today's v0.21 replay surface by
reading the lightweight JSON-lines trace shape; upgrading to the v3
wire format is a drop-in once the integrator lands the hooks above.
See `mty_stdlib::eval::replay_glue::V029_BACKLOG` for the canonical
list (kept in sync with the commit-body backlog).

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
* `crates/mty-stdlib/src/eval/replay_glue.rs` (7 tests): baseline
  decode, malformed JSON, missing file, missing user prompt, mock
  dispatch round-trip, v0.29 backlog non-empty.
* `crates/mty-stdlib/src/eval/mod.rs` (2 tests): empty-suite +
  no-members error paths.

60 tests total. All pass via `cargo test -p mty-stdlib --lib eval`.

## See also

* `docs/internals/replay.md` — the v0.21 byte-identical replay
  machinery the eval driver builds on.
* `docs/reference/stdlib/swarm.md` — `std.swarm`, sibling
  multi-LLM primitive sharing the same `Member` enum.
* `docs/reference/stdlib/llm.md` — the typed LLM provider surface
  `Member` wraps.
* `docs/reference/stdlib/memory.md` — the `Embedder` trait the
  semantic-similarity comparator uses.
* `examples/31_eval_agent.mty` — minimal Mighty-source example.
* `dev/history/notes/STD_EVAL_V0_28_NOTES.md` — design rationale
  (track-G ship notes; populated by the integrator).
