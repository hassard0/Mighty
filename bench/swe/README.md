# `bench/swe/` — Mighty SWE-bench Verified harness

**Status:** v0.30 Track B — first adoption-proof harness for the
Mighty agent framework. Ships the 10-problem smoke; the full
~500-problem run is gated behind `make bench-full`.

## Why this exists

Mighty already has a strong feature pitch (capability-typed `@tool`,
deterministic replay, `std.swarm`, native eval) but until v0.30 we
had **zero adoption proof**. SWE-bench Verified is the canonical
benchmark for coding agents — so we built a Mighty agent that
runs it.

This crate is the harness; `agent.mty` is the Mighty source spec;
`src/agent.rs` is the Rust driver (v0.30) that mirrors that spec
line for line.

## Layout

```
bench/swe/
├── Cargo.toml          # standalone crate, NOT in the main workspace
├── README.md           # this file
├── SMOKE_PROBLEMS.md   # the curated 10
├── agent.mty           # Mighty source spec (v0.31 will lower this directly)
├── src/
│   ├── main.rs         # CLI entry point — `cargo run --release`
│   ├── agent.rs        # ReAct loop driver
│   ├── llm.rs          # tiny Anthropic Messages client
│   ├── tools.rs        # capability-typed tool impls
│   ├── problems.rs     # the fixed smoke subset
│   ├── dataset.rs      # SWE-bench Verified loader (HF datasets-server)
│   ├── workspace.rs    # per-instance git clone + reset
│   └── scorer.rs       # pytest-driven FAIL_TO_PASS scorer
├── data/instances/     # cached dataset rows (gitignored)
└── results/            # JSON per-run reports (gitignored)
```

Published smoke results land in `dev/history/benchmarks/swe-bench-smoke-v0.30.md`.

## Running the smoke (10 problems, ~$5-20)

```bash
export ANTHROPIC_API_KEY=sk-ant-...
# from repo root:
make bench-smoke
# or directly:
cd bench/swe && cargo run --release -- \
  --num-problems 10 \
  --member anthropic:claude-opus-4-7
```

Expected wall-clock: 15-40 minutes (each instance is a multi-turn
ReAct loop; some hit the 5-minute per-instance cap).

## Running the full set (~500 problems, ~$300-500)

```bash
make bench-full         # gates on a typed confirmation
```

The Makefile target asks for explicit confirmation before launching.
Direct invocation requires the second-line guard:

```bash
MTY_BENCH_FULL_CONFIRM=1 cargo run --release -- --all --member anthropic:claude-opus-4-7
```

(v0.30 still iterates the curated 10 even with `--all`; the bulk
dataset pull lands in v0.31.)

## Re-running a single problem locally

```bash
cd bench/swe && cargo run --release -- \
  --num-problems 1 \
  --member anthropic:claude-opus-4-7 \
  --output /tmp/just_one.json
```

To pick *which* one, edit `src/problems.rs::SMOKE_PROBLEMS` to put
your target first, or copy the row into a new test case.

## CI integration

The smoke run does **not** run in CI by default — the LLM cost is
non-trivial and the runtime is too long for the per-PR loop. CI
exercises the harness's plumbing via:

```bash
cd bench/swe && cargo build --release    # compile-only
cd bench/swe && cargo test               # unit tests for problems/tools
```

The published-result file (`dev/history/benchmarks/swe-bench-smoke-v0.30.md`)
is checked in; CI diffs it on every PR so changes are auditable.

## Budget controls

| Knob | Default | Where |
|---|---|---|
| Global cap (USD) | 25 | `--dollar-cap` |
| Per-instance cap | 3 | `--per-instance-cap` |
| Turns per instance | 25 | `--max-turns` |
| Seconds per instance | 300 | `--max-seconds` |

When the **global cap** is hit, the harness aborts with partial
results — the JSON report still records what completed.

When a **per-instance cap** is hit, that instance is recorded with
`stop_reason: DollarBudget` and the next instance starts cleanly.

## Reproducibility

Every run records:

* The Mighty git SHA (`mighty_commit`).
* The dataset revision pin (`SMOKE_SUBSET_VERSION`).
* The exact model string.
* Per-instance token usage + cost.
* The agent's produced patch (first 20 lines as preview).

The full per-turn trace is kept in memory and could be persisted in
a v0.31 follow-up (`--trace-out`).
