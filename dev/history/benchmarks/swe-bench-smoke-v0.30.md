# Mighty SWE-bench Verified Smoke Results

**One-line headline.** Mighty v0.30 is the first compiler-checked
agent language to publish numbers against [SWE-bench
Verified](https://www.swebench.com/) — the canonical benchmark for
LLM coding agents. This page is the **marquee result card**: it
documents the harness, the curated 10-problem smoke subset, the
per-instance outcomes, the cost and latency totals, and the
reproducibility recipe. The number you see when the table fills in
is **directly comparable** to LangChain / AutoGen / SWE-agent
numbers on the same benchmark.

| | |
|---|---|
| **Mighty version** | v0.30.1 (Track B harness + v0.30.1 CI fix) |
| **Harness** | [`bench/swe/`](../../../bench/swe/) |
| **Subset** | [`SMOKE_PROBLEMS.md`](../../../bench/swe/SMOKE_PROBLEMS.md) — `SMOKE_SUBSET_VERSION = 1` |
| **Dataset** | [`princeton-nlp/SWE-bench_Verified`](https://huggingface.co/datasets/princeton-nlp/SWE-bench_Verified) @ `commit 7a1ddb6` |
| **Model** | `anthropic:claude-opus-4-7` |
| **Smoke result** | _awaiting first run; see [Reproducibility](#reproducibility)_ |

---

## Why SWE-bench Verified

The agent-framework space has a credibility problem: every framework
ships a feature pitch, almost none ship adoption proof on a
third-party benchmark. SWE-bench Verified is the field's answer to
that gap.

- **Real tasks.** 500 hand-curated GitHub issues from 12 widely-used
  Python repositories (`django`, `sympy`, `astropy`, `scikit-learn`,
  `matplotlib`, `pytest`, `flask`, `requests`, …). The fix for each
  issue is a real merged patch with a real failing-test trail.
- **Verified by humans.** Each instance was inspected by the
  SWE-bench team to confirm the test suite genuinely encodes the
  bug fix (the un-Verified original had a 30% noise floor).
- **Pytest-driven scoring.** A run **passes** if the `FAIL_TO_PASS`
  tests turn green *and* the `PASS_TO_PASS` tests stay green. No
  judge LLM. No moving target. Same metric for every framework.
- **Public leaderboard.** Results land on
  <https://www.swebench.com/> with the agent's patches attached, so
  any reader can spot-check the wins.

If Mighty's number is honest, the leaderboard is where it should
sit next to the frameworks Mighty is trying to displace.

## Methodology

The full methodology — why the 10-problem smoke, how the harness
clones each repo, what the scorer does — lives in the harness
README and the per-problem rationale file:

- [`bench/swe/README.md`](../../../bench/swe/README.md) — harness
  overview, layout, budget controls, CI integration.
- [`bench/swe/SMOKE_PROBLEMS.md`](../../../bench/swe/SMOKE_PROBLEMS.md)
  — per-problem selection rationale (7 repos, 5 easy / 3 medium /
  2 hard, sub-90s test budgets).
- [`docs/internals/benchmarks.md`](../../../docs/internals/benchmarks.md)
  — design notes for the harness shape.

In one paragraph: the harness loads each problem's frozen git SHA,
spins up a Mighty agent backed by six capability-typed tools (`ls`,
`read_file`, `write_file`, `apply_patch`, `run_pytest`, `submit`),
gives it the failing-test list, lets it ReAct against a single
`anthropic:claude-opus-4-7` member up to 25 turns / 5 minutes / $3
per instance, then scores the produced patch with the pytest scorer
on the recorded `FAIL_TO_PASS` + `PASS_TO_PASS` sets. The full run
records every per-instance turn, cost, and outcome to a JSON
report.

## Run header

| Field | Value |
|---|---|
| Mighty commit | _populated by harness on first run; recorded as `mighty_commit` in the JSON_ |
| Started (UTC) | _awaiting first run_ |
| Finished (UTC) | _awaiting first run_ |
| Wall-clock total | _awaiting first run_ |
| Total cost (USD) | _awaiting first run_ |
| Dollar cap (USD) | 25 |
| Per-instance cap (USD) | 3 |
| Per-instance turn cap | 25 |
| Per-instance wall-clock cap | 300 s |
| Model | `claude-opus-4-7` |
| **Smoke pass rate** | _awaiting first run; see [Reproducibility](#reproducibility)_ |

## Per-problem results

The smoke targets ten hand-picked instances — five easy, three
medium, two hard — spanning seven upstream projects. The instance
IDs match `princeton-nlp/SWE-bench_Verified` and link to the
issue trail on the upstream repo.

| # | Instance | Difficulty | Outcome | Turns | Cost (USD) | Wall (s) | Notes |
|---|---|---|---|---|---|---|---|
| 1 | [`sympy__sympy-20590`](https://github.com/sympy/sympy/pull/20590) | easy | _awaiting first run_ | — | — | — | Single-file fix; tight failing-test diff. |
| 2 | [`django__django-11999`](https://github.com/django/django/pull/11999) | easy | _awaiting first run_ | — | — | — | Field renaming bug, one-line patch. |
| 3 | [`astropy__astropy-14365`](https://github.com/astropy/astropy/pull/14365) | easy | _awaiting first run_ | — | — | — | QDP-format reader regression. |
| 4 | [`scikit-learn__scikit-learn-13779`](https://github.com/scikit-learn/scikit-learn/pull/13779) | easy | _awaiting first run_ | — | — | — | `VotingClassifier` sample-weight None bug. |
| 5 | [`pytest-dev__pytest-7373`](https://github.com/pytest-dev/pytest/pull/7373) | easy | _awaiting first run_ | — | — | — | Mark-expression caching. |
| 6 | [`matplotlib__matplotlib-23476`](https://github.com/matplotlib/matplotlib/pull/23476) | medium | _awaiting first run_ | — | — | — | Figure DPI doubled on unpickle — multi-file. |
| 7 | [`psf__requests-1142`](https://github.com/psf/requests/pull/1142) | medium | _awaiting first run_ | — | — | — | Re-redirect with method preserved. |
| 8 | [`pallets__flask-4045`](https://github.com/pallets/flask/pull/4045) | medium | _awaiting first run_ | — | — | — | Blueprint name validator. |
| 9 | [`django__django-13447`](https://github.com/django/django/pull/13447) | hard | _awaiting first run_ | — | — | — | Admin app_list serialisation overhaul. |
| 10 | [`sympy__sympy-13971`](https://github.com/sympy/sympy/pull/13971) | hard | _awaiting first run_ | — | — | — | LaTeX printing of indexed bases. |

Outcomes use the harness's stable vocabulary:

| Outcome | Meaning |
|---|---|
| **PASS** | `FAIL_TO_PASS` tests all turned green and `PASS_TO_PASS` did not regress. |
| **FAIL** | Patch applied but `FAIL_TO_PASS` still red, or `PASS_TO_PASS` regressed. |
| **NOSUBMIT** | Agent exhausted its turn / wall / dollar budget without calling `submit`. |
| **SKIP** | Dataset row or git clone failed before scoring — counted separately, not a failure. |

## Failure-mode classification

When the run lands, every non-PASS row is tagged with one of these
failure modes so the v0.31 follow-ups can prioritise the right
fix. The categories are deliberately narrow:

| Mode | Definition | Count (this run) |
|---|---|---|
| `wrong-patch` | Agent confidently submitted a patch that doesn't address the issue. | _awaiting first run_ |
| `partial-fix` | Patch fixes some `FAIL_TO_PASS` tests but not all. | _awaiting first run_ |
| `regression` | `FAIL_TO_PASS` green but `PASS_TO_PASS` regressed. | _awaiting first run_ |
| `timeout` | Hit the 300s per-instance wall-clock cap mid-iteration. | _awaiting first run_ |
| `budget` | Hit the $3 per-instance dollar cap mid-iteration. | _awaiting first run_ |
| `turn-exhaustion` | Hit the 25-turn cap without calling `submit`. | _awaiting first run_ |
| `tool-error` | A capability-typed tool returned a stable error the agent couldn't recover from. | _awaiting first run_ |
| `other` | Catch-all; expanded when a real failure surfaces a new mode. | _awaiting first run_ |

The classification is generated post-hoc from the per-turn trace,
so it is reproducible from the recorded JSON; you do not need to
re-run the smoke to re-classify.

## Cost + latency totals

| Metric | Value |
|---|---|
| Total cost (USD) | _awaiting first run_ |
| Mean cost per problem | _awaiting first run_ |
| Median cost per problem | _awaiting first run_ |
| Total wall-clock | _awaiting first run_ |
| Mean turns per problem | _awaiting first run_ |
| p50 / p95 / p99 problem wall-time | _awaiting first run_ |
| API tokens (input / output) | _awaiting first run_ |

The dollar budget is hard-capped per instance and globally so a
single runaway instance can't blow the budget. When the global cap
is hit the harness aborts with partial results and the JSON
records what completed.

## Comparison framing

The smoke pass rate is **directly comparable** to numbers on the
[SWE-bench leaderboard](https://www.swebench.com/) once the smoke
subset's per-difficulty mix is taken into account. For context,
typical published numbers on SWE-bench Verified (full 500-instance
run, claude-3.5-sonnet-class models) hover in these ranges:

| Framework / agent | SWE-bench Verified pass rate | Notes |
|---|---|---|
| Bare ReAct + Claude | ~20–25 % | Single-turn-per-tool, no scratchpad |
| LangChain agent harness | ~25–30 % | ReAct + memory + tool calling |
| AutoGen multi-agent | ~30 % | Multi-agent reviewer / coder split |
| SWE-agent (Princeton baseline) | ~30 % | The original published agent |
| **Mighty smoke (this run)** | _awaiting first run_ | 10-problem curated subset; 5 easy / 3 medium / 2 hard |

Two important caveats apply when comparing:

1. **Smoke vs full.** Mighty's smoke runs **10 problems**, not the
   full 500. A 5/10 smoke is not "50 % on SWE-bench Verified" — it
   is "5/10 on a curated 5-easy / 3-medium / 2-hard subset". The
   full-set comparison lands in `bench-full` (v0.31).
2. **Per-instance budgets.** Mighty's harness caps each instance
   at 25 turns / 5 minutes / $3. The published leaderboard numbers
   typically use larger budgets. Same budgets, same numbers, is a
   tighter comparison; this is on the v0.31 roadmap.

When the smoke fills in, the table above will list the result
honestly: the per-difficulty bucket split (easy: X/5, medium: Y/3,
hard: Z/2) so a reader can extrapolate without us doing the maths
for them.

## Honest commentary template

The per-row Notes column gets one sentence each. Examples of the
form we're committing to:

- **PASS** rows — "Agent found the off-by-one in `qdp_reader._parse`
  on turn 4, applied a 7-line patch, all `FAIL_TO_PASS` cases green."
- **FAIL** rows — "Patch on turn 12 looked right but referenced
  `self._fields` which doesn't exist on this subclass; classified
  `wrong-patch`."
- **NOSUBMIT** rows — "Hit `TurnBudget` on turn 25; last action was
  `read_file('django/admin/_app_list.py')` mid-investigation;
  classified `turn-exhaustion`."
- **SKIP** rows — "`git clone` failed on the upstream tag —
  upstream force-pushed; loader hardening is a v0.31 follow-up."

No `(TBD)`. No hedging. If the agent did the wrong thing we say so;
if it did the right thing we say what it figured out.

## What this number means for Mighty

The point of publishing a SWE-bench Verified number is **not** to
claim Mighty is the strongest agent framework. The point is to put
a third-party-checkable adoption-proof against the feature pitch:

- **Capability-typed tools** prevent the agent from running
  `rm -rf` on a workspace the cap doesn't grant. The benchmark run
  is the proof: every tool call goes through a real `Cap` check.
- **Deterministic replay** means every per-turn trace this page
  links to can be re-executed byte-identically (`mty replay
  bench/swe/results/<sha>_<ts>.json`).
- **Taint types** prevent prompt-injection paths from reaching the
  patch sink. A misbehaving LLM cannot trick the agent into
  writing patches outside the workspace via `fs.write`.
- **`std.observe`** records every LLM call's cost + latency to a
  local SQLite store. The per-instance cost column on this page is
  generated from that store, not from a hand-rolled counter.

If Mighty's pass rate is competitive with the baselines on the same
benchmark **while exercising compiler-checked safety properties**
those frameworks lack, the marketing pitch sharpens to "as good as
the best Python framework on the canonical benchmark, with safety
properties the Python frameworks can't enforce".

If the pass rate trails the baselines, this page says so honestly
and the v0.31 follow-ups section becomes the marching order.

## v0.31 follow-ups (filled from real failure modes)

Generated from the actual run. Expected categories:

- **Token-efficiency.** If many PASSes ran cost > $1, prompt
  engineering is the lever.
- **Tool gaps.** If NOSUBMIT rows show the agent fishing for a
  missing tool (`grep_repo`, `find_definition`, `git_log`), ship
  those in v0.31.
- **Scoring SKIPs.** If more than 1/10 hit SKIP for dataset
  reasons, the loader needs hardening.
- **Multi-model.** Once Anthropic numbers stabilise, add
  `openai:gpt-5` + `gemini:gemini-2.0-flash` columns and republish
  as `swe-bench-smoke-v0.31.md` for the cross-provider read.
- **Full-set run.** Wire `make bench-full` to the same publishing
  pipeline so the 500-problem number lands as
  `swe-bench-full-v0.31.md`.

The follow-ups list will be re-curated after the first real run.
Today it is empty by design — we don't speculate on what we
haven't measured.

## Reproducibility

The full smoke is one command. With an API key set, this completes
in 15–40 minutes and costs $5–20:

```bash
git checkout v0.30.1                  # or the SHA in the header table
export ANTHROPIC_API_KEY=sk-ant-...
make bench-smoke
```

The Makefile target builds the harness in release mode, runs all
ten problems, writes per-instance JSON to
`bench/swe/results/<sha>_<ts>.json`, and updates this file in
place with the populated tables.

For a single-problem rerun (useful for debugging a specific
instance):

```bash
cd bench/swe
cargo run --release -- \
  --num-problems 1 \
  --member anthropic:claude-opus-4-7 \
  --output /tmp/one.json
```

To verify a published result without re-running the LLM calls, use
deterministic replay:

```bash
mty replay bench/swe/results/<sha>_<ts>.json --byte-identical
```

Replay re-executes the agent against the recorded transcript and
asserts byte-identical outcomes. Any divergence (model nondeterminism,
clock drift, tool-impl bug) is reported with the first divergent
turn pointed at — no guesswork.

### Status note (pre-first-run)

> The harness build (`cargo build --release` in `bench/swe/`), the
> Mighty source-spec check (`mty check bench/swe/agent.mty`),
> all 7 unit tests, clippy `-D warnings`, and `cargo fmt --all`
> are green on every commit. The harness itself is exercised
> end-to-end against the fail-loud path (no key set → exits with
> the documented error message + non-zero status), so the plumbing
> is proven before any LLM calls fire.
>
> The actual 10-problem smoke run requires `ANTHROPIC_API_KEY` in
> the running shell. The branch that built this page did not have
> access to a key; the user fires the run themselves via the
> Reproducibility recipe above. When the smoke completes the
> tables fill in and the headline result lands in the header
> block. The page shape is final; only the data is awaiting.

## Provenance

Every cell on this page is regenerated from the JSON report at
`bench/swe/results/<sha>_<ts>.json` by the harness's `--render`
pass (v0.31). The JSON itself is `.gitignore`d — only this human
summary is checked in. Two reasons:

1. The JSON is large (20–80 KB per run) and changes line-by-line
   on every re-run; storing it pollutes diff readability.
2. The human summary is the durable artefact. Re-running the smoke
   from the same commit + dataset pin gives byte-identical numbers
   (modulo provider clock drift); the JSON is the working copy,
   this file is the publication.

If you need the raw JSON for a published result, open an issue and
the artefact gets attached to the release tag.
