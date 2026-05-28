# `bench/swe/` — Mighty SWE-bench Verified Harness

**Status.** v0.30 Track B — the first adoption-proof harness for the
Mighty agent framework. The ten-problem smoke is wired through
`make bench-smoke`; the full ~500-problem run is gated behind
`make bench-full`. Published results land in
[`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](../../dev/history/benchmarks/swe-bench-smoke-v0.30.md)
— that page is the marketing front door; this README is the
operating manual.

## Why this exists

Until v0.30 Mighty had a strong feature pitch (capability-typed
`@tool`, deterministic replay, `std.swarm`, native eval, taint
types) but **zero adoption proof on a third-party benchmark**.
[SWE-bench Verified](https://www.swebench.com/) is the canonical
benchmark for coding agents — 500 hand-curated GitHub issues from
12 widely-used Python projects, scored by running the upstream test
suite against the agent's patch. We built a Mighty agent that runs
it.

This crate is the harness; [`agent.mty`](agent.mty) is the Mighty
source-of-truth spec for the agent; [`src/agent.rs`](src/agent.rs)
is the Rust driver that mirrors that spec line for line. When the
v0.31 attribute-macro pass lifts `@tool` into auto-generated
companions, the Rust driver shrinks to a thin shim around the
lowered `.mty`.

## What gets exercised

| Mighty surface | How this harness uses it |
|---|---|
| `@tool(description, cap: ...)` | Six capability-typed tools (`ls`, `read_file`, `write_file`, `apply_patch`, `run_pytest`, `submit`); every cap is checked against the workspace root before each tool body runs. |
| `std.llm` Anthropic provider | The ReAct loop is one `AnthropicClient.complete` call per turn; the loop terminates on `submit` or budget exhaustion. |
| `std.observe` / `mty inspect --cost` | Every per-turn LLM call records to the local SQLite store; the published cost column is read back from it, not hand-counted. |
| `Tainted[T]` | LLM responses are minted `Tainted` by the provider; the patch sink (`apply_patch`) requires sanitisation, preventing prompt-injection paths into the working tree. |
| Deterministic replay | Every run records its per-turn trace; `mty replay --byte-identical` re-executes the run against the recorded transcript for verification. |

## Layout

```
bench/swe/
├── Cargo.toml           # standalone crate (not in the main workspace)
├── README.md            # this file
├── SMOKE_PROBLEMS.md    # the curated 10-problem subset + rationale
├── agent.mty            # Mighty source spec (v0.31 lowers this directly)
├── src/
│   ├── main.rs          # CLI entry point — `cargo run --release`
│   ├── agent.rs         # ReAct loop driver
│   ├── llm.rs           # tiny Anthropic Messages client
│   ├── tools.rs         # capability-typed tool impls
│   ├── problems.rs      # the fixed smoke subset
│   ├── dataset.rs       # SWE-bench Verified loader (HF datasets-server)
│   ├── workspace.rs     # per-instance git clone + reset
│   └── scorer.rs        # pytest-driven FAIL_TO_PASS / PASS_TO_PASS scorer
├── data/instances/      # cached dataset rows (.gitignored)
└── results/             # JSON per-run reports (.gitignored)
```

The crate is intentionally standalone (`Cargo.lock` of its own) so
it can build without the full Mighty workspace — useful for
adoption-testing the harness in isolation, useful for the CI matrix
that runs `cargo build` on the harness without the language-side
test suite.

## Running the smoke (10 problems)

The smoke targets a fixed 5-easy / 3-medium / 2-hard mix across 7
upstream repos. Wall-clock is 15–40 minutes; cost is $5–20 with the
default model. See
[`SMOKE_PROBLEMS.md`](SMOKE_PROBLEMS.md) for the per-problem
rationale.

```bash
export ANTHROPIC_API_KEY=sk-ant-...

# Easiest: from the repo root.
make bench-smoke

# Or, directly from this crate:
cd bench/swe
cargo run --release -- \
  --num-problems 10 \
  --member anthropic:claude-opus-4-7
```

When the run lands, the per-instance JSON is written to
`results/smoke_<UTC-ts>.json` and the human summary at
[`dev/history/benchmarks/swe-bench-smoke-v0.30.md`](../../dev/history/benchmarks/swe-bench-smoke-v0.30.md)
is updated in place with the populated tables. Re-runs from the
same commit + dataset pin produce byte-identical pass / fail
outcomes (modulo provider clock drift on cost / latency cells).

## Running the full Verified set

```bash
make bench-full
```

`bench-full` asks for explicit typed confirmation before launching
(~500 problems, ~$300–500, several hours wall-clock). Direct
invocation requires the second-line guard env var:

```bash
MTY_BENCH_FULL_CONFIRM=1 \
  cargo run --release -- --all --member anthropic:claude-opus-4-7
```

v0.30 still iterates the curated 10 even with `--all`; the bulk
dataset pull lands in v0.31, alongside the published-result page
`swe-bench-full-v0.31.md`.

## Re-running a single problem

```bash
cd bench/swe
cargo run --release -- \
  --num-problems 1 \
  --member anthropic:claude-opus-4-7 \
  --output /tmp/just_one.json
```

To pick *which* one, edit
[`src/problems.rs::SMOKE_PROBLEMS`](src/problems.rs) to put your
target instance first, or copy the row into a new test case. The
dataset row is cached on first fetch into `data/instances/`, so
subsequent runs are network-free.

## Replaying a finished run

Every recorded JSON report is a byte-identical replay seed:

```bash
mty replay bench/swe/results/smoke_<UTC-ts>.json --byte-identical
```

Any divergence (model nondeterminism, clock drift, tool-impl bug)
is reported with the first divergent turn pointed at. This is how
we publish numbers without inviting the "it works on my machine"
question.

## Budget controls

The harness defends against runaway spend at three levels:

| Knob | Default | Where | When it trips |
|---|---|---|---|
| Global cap (USD) | 25 | `--dollar-cap` | The whole run aborts; partial JSON still landed. |
| Per-instance cap (USD) | 3 | `--per-instance-cap` | Instance records `stop_reason: DollarBudget`; next instance starts cleanly. |
| Turns per instance | 25 | `--max-turns` | Instance records `stop_reason: TurnBudget`. |
| Seconds per instance | 300 | `--max-seconds` | Instance records `stop_reason: WallClock`. |

The defaults are intentionally tight for the smoke. Loosen for
`bench-full` runs once the smoke pass rate stabilises.

## CI integration

The smoke run does **not** run in the per-PR CI loop — LLM cost is
non-trivial and the runtime is too long for the 10-minute
build-test-merge cycle. The per-PR CI exercises only the harness's
plumbing:

```bash
cd bench/swe && cargo build --release    # compile-only
cd bench/swe && cargo test               # 7 unit tests (problems + sandbox)
```

Both run on every PR. The published-result file
(`dev/history/benchmarks/swe-bench-smoke-v0.30.md`) is checked in
so CI diffs it on every PR; intentional updates are auditable in
the commit history.

The smoke itself is fired manually on a tagged release boundary
(every v0.X minor), or on demand when the prompt / tools / harness
shape changes meaningfully.

## Reproducibility checklist

Every published run records:

- The Mighty git SHA (`mighty_commit` in the JSON; printed in the
  header table on the published page).
- The dataset revision pin (`SMOKE_SUBSET_VERSION` in
  `src/problems.rs`; bumped whenever the curated 10 change).
- The exact model string (`anthropic:claude-opus-4-7`).
- Per-instance token usage + cost + latency.
- The agent's produced patch (first 20 lines as preview; full
  patch in the JSON).

Re-running the same `make bench-smoke` from the same Mighty SHA +
the same dataset pin produces byte-identical PASS / FAIL outcomes.
Cost / latency drift modestly across runs because Anthropic's
billing clock and rate-limit shaping are not deterministic; we
publish those numbers anyway because the reader cares about them
operationally.

## v0.31 follow-ups

The harness is intentionally narrow for v0.30 — one model, fixed
subset, ReAct loop. The expected v0.31 additions:

- **Multi-model panels.** Wire `--member openai:gpt-5` and
  `--member gemini:gemini-2.0-flash`; publish a 3-column scorecard.
- **Full-set run.** Drop the curated-10 short-circuit on `--all`;
  pull the full 500-instance Verified set; publish
  `swe-bench-full-v0.31.md`.
- **Per-turn trace persistence.** `--trace-out path/` writes every
  turn to a typed event stream so the replay seed can rewind
  partway, not just to the start.
- **Custom-tool ablation.** Run the smoke twice — once with the
  full tool set, once with `apply_patch` swapped for a vanilla
  `write_file` — to quantify how much the patch tool's targeted
  semantics matter.

See the published-results page's "v0.31 follow-ups" section for
the live list (curated against the actual first-run failure
modes).
