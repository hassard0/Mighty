# Benchmarks (internals)

This page covers Mighty's **runtime and adoption benchmarks** — the
big-picture "is this language fast / capable enough to use" measurements,
not the per-crate microbenchmarks. For the microbench layout see
[`benchmarking.md`](benchmarking.md).

There are two families:

| Family | What it measures | Run cadence | Cost |
|---|---|---|---|
| Runtime perf (`crates/mty-bench/`) | parser throughput, agent send latency, mailbox throughput, HTTP server perf, codegen output size | Per-release | Free |
| **Adoption (`bench/swe/`)** | end-to-end agent ability on a public, third-party coding benchmark | Per-major release + ad-hoc | LLM credits |

This file is about the **adoption family** (new in v0.30).

## Why SWE-bench Verified

[SWE-bench Verified](https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified)
is the canonical real-world coding-agent benchmark: 500 hand-curated
issues from real Python OSS repos, each with a buggy commit + the set
of failing tests the fix must turn green.

We picked it over the alternatives because:

* **It's the one comparators are publishing on.** OpenAI Codex, Aider,
  SWE-Agent, Devin, etc. all report SWE-bench Verified numbers. Any
  other choice would force readers to mentally re-baseline.
* **It's adversarial in the right way.** The agent has to read code
  it didn't write, navigate large unfamiliar codebases, and write a
  patch that doesn't regress *anything*. That's exactly Mighty's
  capability-typed `@tool` + `std.swarm` thesis under stress.
* **It's verified.** The "Verified" subset filtered out instances
  where the failing-test selector was ambiguous or the upstream
  fix touched files outside the natural scope — so a pass is a
  high-confidence pass.

We did NOT pick the classic SWE-bench because it's known to contain
~30% noisy instances; the Verified subset is the right adoption-proof
target.

## The 10-problem smoke vs. full-run split

The full SWE-bench Verified set (~500 problems) costs roughly $300-500
in LLM credits per run on Claude Opus 4.7. That's too much to spend
on every iteration during development.

So we ship two scales:

* **Smoke (10 problems, ~$5-20):** runs in 15-40 minutes; the
  default `make bench-smoke` invocation. Fixed, hand-picked
  subset (see `bench/swe/SMOKE_PROBLEMS.md`) so runs are
  directly comparable across Mighty versions.
* **Full (~500 problems, ~$300-500):** `make bench-full`, gated
  behind a typed confirmation + a `MTY_BENCH_FULL_CONFIRM=1`
  env-var as second-line guard. Run manually before each major
  release.

The smoke is intentionally biased toward diversity (7 different
upstream repos, mix of 5 easy / 3 medium / 2 hard) rather than
the 10 easiest — that would over-state our adoption-readiness.

## Architecture

```
bench/swe/
├── src/
│   ├── main.rs       CLI + per-instance loop + budget checks
│   ├── agent.rs      ReAct loop driver (mirror of agent.mty)
│   ├── llm.rs        tiny Anthropic Messages client (NOT mty-stdlib::llm)
│   ├── tools.rs      capability-typed tool impls
│   ├── problems.rs   the fixed smoke subset
│   ├── dataset.rs    SWE-bench Verified loader
│   ├── workspace.rs  per-instance git clone + reset
│   └── scorer.rs     pytest-driven FAIL_TO_PASS scorer
└── agent.mty         Mighty source-of-truth for the agent
```

The harness is a **standalone cargo crate** (`exclude = ["bench/swe"]`
in the top-level `Cargo.toml`) so its heavy deps (`reqwest`,
`tokio-rustls`) don't slow down `mty-cli` builds for everyone.

We deliberately do NOT build the harness on top of `mty-stdlib::llm` —
that crate's retry/streaming/budget plumbing is the wrong abstraction
for a benchmark runner that needs deterministic single-shot calls.
The `agent.mty` file pins the Mighty surface form so the v0.31
attribute-macro pass can lower it directly when ready.

## Per-instance lifecycle

```
1. dataset::load_instance(smoke)
       -> HF datasets-server fetch (cached on disk)
2. workspace::GitWorkspace::clone_at(repo, base_commit)
       -> shallow git clone into bench/swe/.swe-work/checkouts/
3. agent::run_agent(client, ws, problem_statement, failing_tests)
       Loop:
         - send (system, history, tools) to Claude
         - execute every tool_use under workspace capability set
         - stop on submit / max_turns / wall_clock / dollar_budget
4. scorer::score(ws, instance, submitted)
       - reset workspace to base commit
       - apply agent's diff + dataset's test_patch
       - run pytest <FAIL_TO_PASS...>
       - record PASS/FAIL/SKIP
5. write per-instance row to results/<sha>_<ts>.json
```

## Budget controls

| Knob | Default | Notes |
|---|---|---|
| `--dollar-cap` | 25 USD | Global; aborts with partial results when hit. |
| `--per-instance-cap` | 3 USD | Records `stop_reason: DollarBudget` per instance. |
| `--max-turns` | 25 | Per-instance ReAct turn cap. |
| `--max-seconds` | 300 | Per-instance wall-clock cap. |

The smoke run's $25 ceiling matches the user-authorised spend; the
full run's `--dollar-cap 500` is set by the Makefile target.

## Reproducibility

Every result JSON records:

* `mighty_commit` — short SHA the harness was built from.
* `subset_version` — bumps when the curated smoke list changes.
* `model` — exact model string sent to the provider.
* Per-instance `cost_usd`, `input_tokens`, `output_tokens`.
* `agent_patch_preview` — first 20 lines of the agent's diff.

To rerun against the same dataset snapshot, set
`MTY_SWE_OFFLINE=1` and ensure `data/instances/` is populated.

## How to compare two runs

Both runs land in `bench/swe/results/*.json`. Read the
`pass_count / num_problems` ratio + the per-instance `outcome` field.
The publish-quality version lives in
`dev/history/benchmarks/swe-bench-smoke-v0.30.md` (and successor
files for v0.31+).

## v0.31 follow-ups

* Add `openai:gpt-5` and `gemini:gemini-2.0-flash` as `--member`
  targets so we can publish a multi-model row.
* Land the bulk dataset puller so `--all` runs all ~500.
* Surface the per-turn trace as a `--trace-out FILE` flag so
  failure modes are debuggable post-hoc.
* Wire the v0.31 attribute-macro pass so `bench/swe/agent.mty`
  drives the loop instead of `src/agent.rs`.
* Promote the harness to its own `mty-bench-swe` crate published
  on crates.io alongside `mty-cli`.
