# SWE-bench Verified — Mighty v0.30 Smoke Results

**Mighty version:** v0.30 (Track B)
**Harness:** `bench/swe/` @ commit-of-record (see header table)
**Subset:** `SMOKE_SUBSET_VERSION = 1` (the 10 hand-picked problems in
[`bench/swe/SMOKE_PROBLEMS.md`](../../../bench/swe/SMOKE_PROBLEMS.md))
**Model:** `anthropic:claude-opus-4-7`

This is the **first** Mighty adoption-proof number. See
[`docs/internals/benchmarks.md`](../../../docs/internals/benchmarks.md)
for the methodology and the why-SWE-bench-Verified rationale.

## Run header

| Field | Value |
|---|---|
| Mighty commit | _populated by harness — `mighty_commit` in the JSON_ |
| Started (UTC) | _populated by harness_ |
| Finished (UTC) | _populated by harness_ |
| Wall-clock total | _populated by harness_ |
| Total cost (USD) | _populated by harness_ |
| Dollar cap (USD) | 25 |
| Per-instance cap (USD) | 3 |
| Per-instance turn cap | 25 |
| Per-instance wall-clock cap | 300s |
| Model | claude-opus-4-7 |
| **Smoke result** | **N / 10 passed** |

## Status at publication

> **NOT YET EXECUTED on this branch.**
>
> The harness build (`cargo build --release` in `bench/swe/`),
> Mighty source-spec check (`mty check bench/swe/agent.mty`),
> all 7 unit tests, clippy `-D warnings`, and `cargo fmt --all`
> are green. The harness itself was exercised end-to-end against
> the fail-loud path (no key set → exits with the documented
> error message + non-zero status).
>
> The actual 10-problem smoke run requires `ANTHROPIC_API_KEY`
> in the running shell. The Track B session that built this
> branch did not have access to a key; the user will fire the
> run themselves via:
>
> ```bash
> export ANTHROPIC_API_KEY=sk-ant-...
> make bench-smoke
> ```
>
> When the smoke completes, this file is updated in place with
> the real per-instance results table + cost + commentary. The
> JSON report lands in `bench/swe/results/<sha>_<ts>.json` (gitignored)
> and the human summary in this file (committed).
>
> The harness's own logic is exercised by `cargo test`
> (7 unit tests covering the smoke list shape and the capability
> sandbox), and by the fail-loud path verification.

## Results table (populated after first real run)

| # | Instance ID | Difficulty | Outcome | Turns | Cost | Wall | Notes |
|---|---|---|---|---|---|---|---|
| 1 | sympy__sympy-20590 | easy | _pending_ | – | – | – | – |
| 2 | django__django-11999 | easy | _pending_ | – | – | – | – |
| 3 | astropy__astropy-14365 | easy | _pending_ | – | – | – | – |
| 4 | scikit-learn__scikit-learn-13779 | easy | _pending_ | – | – | – | – |
| 5 | pytest-dev__pytest-7373 | easy | _pending_ | – | – | – | – |
| 6 | matplotlib__matplotlib-23476 | medium | _pending_ | – | – | – | – |
| 7 | psf__requests-1142 | medium | _pending_ | – | – | – | – |
| 8 | pallets__flask-4045 | medium | _pending_ | – | – | – | – |
| 9 | django__django-13447 | hard | _pending_ | – | – | – | – |
| 10 | sympy__sympy-13971 | hard | _pending_ | – | – | – | – |

Outcomes use the harness's vocabulary:

* **PASS** — `FAIL_TO_PASS` tests all turned green; `PASS_TO_PASS` did not regress.
* **FAIL** — patch applied but `FAIL_TO_PASS` still red, or `PASS_TO_PASS` regressed.
* **NOSUBMIT** — agent exhausted its turn/wall/dollar budget without calling `submit`.
* **SKIP** — git clone failed, dataset row unavailable, or the scoring patches didn't apply
  cleanly. Skipped instances are tallied separately and do NOT count as failures.

## Honest commentary template (filled per-instance after the run)

The "Notes" column on each row gets:

* **PASS** rows: one-sentence "what the agent figured out".
* **FAIL** rows: classify the failure mode — `wrong-patch`,
  `partial-fix`, `regression`, `applied-but-tests-still-red`,
  `infinite-loop`, `over-edited`. One sentence on the symptom.
* **NOSUBMIT** rows: tag with `stop_reason` from the JSON
  (`TurnBudget` / `WallClock` / `DollarBudget` / `ApiError(...)`)
  and one sentence on what the agent was last doing.
* **SKIP** rows: one sentence on why scoring couldn't run
  (typically a dataset / git issue, not an agent failure).

## v0.31 follow-ups (filled from real failure modes)

Generated from the actual run. Expected categories:

* **Token-efficiency** — if many PASSes ran cost > $1, that's
  a prompt-engineering target.
* **Tool gaps** — if NOSUBMIT rows show the agent fishing for
  a missing tool (`grep`, `find_definition`, `git_log`),
  ship those in v0.31.
* **Scoring SKIPs** — if more than 1/10 hit SKIP for dataset
  reasons, the loader needs hardening.
* **Multi-model** — once Anthropic numbers stabilise, add the
  `openai:gpt-5` + `gemini:gemini-2.0-flash` columns and
  republish as `swe-bench-smoke-v0.31.md`.

## How to reproduce

```bash
git checkout v030-track-b              # or the SHA in the header table
export ANTHROPIC_API_KEY=sk-ant-...
make bench-smoke
# results land in:
#   - bench/swe/results/<sha>_<ts>.json  (raw, gitignored)
#   - this file                          (human summary, committed)
```

For a single-problem rerun (debugging):

```bash
cd bench/swe
cargo run --release -- \
  --num-problems 1 \
  --member anthropic:claude-opus-4-7 \
  --output /tmp/one.json
```
