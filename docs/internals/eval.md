# `mty test --eval` — file format, CLI reference, CI template (v0.30)

**Crate:** `mty-cli` (`src/cmd/test.rs`) + `mty-stdlib::eval::runner` (`run_for_cli`)
**Related:** `docs/internals/std-eval.md` (the `Suite`/`Case`/`Member`/`Compare` Rust API)
**Roadmap:** v0.30 Track E (this doc); v0.31 follow-ups listed at the bottom.

The `std.eval` driver (shipped v0.28 Track G, replay-substrate v0.29
Track F) became a daily-used CLI verb in v0.30 via `mty test --eval`.
This page documents the file format the discovery layer parses, the
CLI flags, and a copy-paste GitHub Actions workflow.

## Why a CLI surface

`std.eval` started life as a Rust-only API: callers construct a
`Suite`, chain in `Case`s and `Member`s, and `.compare(...)`. That
shape is great for embedded use ("rerun this eval inside my agent
test") but creates a chicken-and-egg problem for routine use: every
eval needs a host program to instantiate the builder.

v0.30 closes that gap. An `.eval.mty` file IS the host program. The
`//!` frontmatter at the top of the file specifies the panel and the
cases; `mty test --eval` discovers every file ending `.eval.mty`,
parses the frontmatter, builds the matching `Suite`, dispatches it,
and prints a `cargo test`-style report. The (still-supported) `pub fn
eval()` body inside the file is the source-of-truth for humans
reading the file; the CLI runner takes the frontmatter as the
machine-readable contract.

## File format

```mty
//! eval: research-agent
//! threshold: semantic_similarity >= 0.85
//! members:
//!   - anthropic:claude-opus-4-7
//!   - openai:gpt-5
//! cases:
//!   - from_trace: traces/research-001.mty-trace
//!   - from_input: "What's the population of France?"

use std.eval.{Suite, Case, Member, Compare}

pub fn eval() -> Report {
  Suite::new("research-agent")
    .case(Case::from_trace("traces/research-001.mty-trace"))
    .case(Case::from_input("What's the population of France?"))
    .run_with(Member::anthropic("claude-opus-4-7"))
    .run_with(Member::openai("gpt-5"))
    .compare(Compare::semantic_similarity(threshold: 0.85))
}
```

### Frontmatter grammar

Each line begins with `//!` (a doc-comment marker), optionally
followed by whitespace. The block ends at the first non-`//!`,
non-blank line. Keys recognised in v0.30:

| Key | Shape | Required | Default |
|---|---|---|---|
| `eval` | scalar suite name | no | filename stem (without `.eval.mty`) |
| `threshold` | `<comparator> >= <0.0–1.0>` | no | `semantic_similarity >= 0.85` |
| `members` | list of `- provider:model` | **yes** | — |
| `cases` | list of `- from_input: "..."` / `- from_trace: path` | **yes** | — |

Comparator names accepted in `threshold`:

- `equal` / `byte_equal` / `exact` — strict byte-equal after trim + lowercase
- `semantic_similarity` / `cosine` / `similarity` — stub embedder cosine
- `tool_call_set_equal` / `tool_call` / `tools` — order-independent tool-name set equality

Provider names accepted in `members`:

- `mock` — `Member::mock(model, "mock-reply", 1)`. Always available.
- `anthropic` / `openai` / `gemini` / `bedrock` — the real swarm providers. Missing API keys are surfaced as a clean `mock_error` (with the env-var name in the message) rather than panicking the runner.
- Any other identifier — falls through to a labelled mock so the suite still runs.

### Case sources

- `from_input: "<prompt>"` — a raw prompt; the comparator runs member-vs-member (no recorded baseline).
- `from_trace: <path>` — a `.mty-trace` file produced by `MTY_RECORD_TRACE=…`. The runner resolves the first user-prompt turn + the recorded assistant reply (the "baseline") and compares each fresh member reply against it.

### Round-tripping through `mty fmt`

Frontmatter lines must be LF-terminated with no trailing whitespace.
The parser is tolerant of internal-blank lines + arbitrary spacing
between `//!` and the key, so a roundtrip through `mty fmt` (which
normalises trailing whitespace) is idempotent.

## CLI reference

```
mty test [OPTIONS]
```

| Flag | Effect |
|---|---|
| (no flag) | Unit-test mode: discover `tests/*.test.mty` + bare `tests/*.mty`, run via the v0.2 `std.test` runner. |
| `--eval` | Eval mode: discover `**/*.eval.mty`, run via the frontmatter-driven `Suite` builder. |
| `--manifest-dir <PATH>` | Override the project root. Default = cwd. |
| `--strict` | Eval mode: fail the run if any cell errored (default). |
| `--no-strict` | Eval mode: ignore error cells when judging pass/fail. Useful for offline / no-API-key dev. |
| `--replay-only` | Eval mode: skip live LLM dispatch; run only against recorded traces (deterministic-replay equivalence assertion). |
| `--ci` | Eval mode: read `members` + `threshold` from the `[eval.ci]` table in `mighty.toml` instead of the per-file frontmatter. |
| `--format pretty\|json` | Output format. JSON emits one object per suite, then a `{"type":"summary"}` object — line-delimited. |

### Manifest sections

```toml
# mighty.toml

[eval]
# Where to look for *.eval.mty files. Defaults to walking the whole
# project tree. Set this to scope discovery to one subdirectory.
paths = ["tests/eval/"]

[eval.ci]
# Used with `mty test --eval --ci`. Per-file `members:` + `threshold:`
# are ignored in favour of these values. Useful when CI wants to
# pin a fixed provider set without editing every eval file.
members = ["anthropic:claude-opus-4-7", "openai:gpt-5"]
threshold = "semantic_similarity >= 0.85"
```

## Output shape

Pretty (default):

```
running 2 eval suites
tests/eval/research_agent.eval.mty:
  [claude-opus-4-7        ] case 0 (trace) PASS  score=0.92 thresh=0.85  cost=$0.04 lat=1.8s
  [gpt-5                  ] case 0 (trace) PASS  score=0.89 thresh=0.85  cost=$0.02 lat=1.1s
  [claude-opus-4-7        ] case 1 (input) PASS  score=0.95 thresh=0.85  cost=$0.03 lat=1.4s
  [gpt-5                  ] case 1 (input) FAIL  cosine 0.78 below threshold 0.85
tests/eval/swarm_review.eval.mty:
  ... passing ...
eval result: 1 failed, 7 passed. cost=$0.34
```

JSON (`--format json`):

```jsonl
{"type":"suite","file":"tests/eval/research_agent.eval.mty","suite":"research-agent","passed":false,"failures":1,"cost_cents":34,"lines":[…]}
{"type":"suite","file":"tests/eval/swarm_review.eval.mty","suite":"swarm-review","passed":true,"failures":0,"cost_cents":0,"lines":[…]}
{"type":"summary","mode":"eval","passed":1,"failed":1,"total":2,"cost_cents":34}
```

One JSON object per line so `jq -c '.'` / `jq 'select(.type == "suite")'`
work without buffering.

## CI template — GitHub Actions

Copy into `.github/workflows/eval.yml`. The workflow runs the
`--replay-only` smoke check on every PR (free + deterministic) and a
nightly real-dispatch run gated by repo-level secrets.

```yaml
name: eval

on:
  pull_request:
  schedule:
    # Nightly real-LLM dispatch at 04:00 UTC.
    - cron: "0 4 * * *"

jobs:
  smoke:
    name: smoke (replay-only)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p mty-cli --release
      - name: mty test --eval --replay-only
        run: |
          ./target/release/mty test --eval --replay-only --format json \
            | tee eval-replay.jsonl
      - uses: actions/upload-artifact@v4
        with:
          name: eval-replay
          path: eval-replay.jsonl

  live:
    name: live providers (nightly)
    if: github.event_name == 'schedule'
    runs-on: ubuntu-latest
    env:
      ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
      OPENAI_API_KEY:    ${{ secrets.OPENAI_API_KEY }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p mty-cli --release
      - name: mty test --eval --ci
        run: |
          ./target/release/mty test --eval --ci --format json \
            | tee eval-live.jsonl
      - uses: actions/upload-artifact@v4
        with:
          name: eval-live
          path: eval-live.jsonl
      - name: Fail on any divergence
        run: |
          jq -e 'select(.type=="summary") | .failed == 0' eval-live.jsonl
```

## v0.31 follow-ups

The CLI surface shipped in v0.30 Track E covers the daily-use loop.
Two upgrades are tracked for v0.31:

1. **Provider-aware `--replay-only` filter.** Today the flag is a
   pass-through (the runner dispatches every member as usual; the
   PASS comes from the trace baseline being equivalent to itself
   under the chosen comparator). The proper filter would drop live
   members + run only the trace-baseline equivalence check. Blocked
   on `Suite` exposing its case/member vectors for introspection.
2. **Run `pub fn eval()` directly.** v0.30 takes the frontmatter as
   the source of truth + ignores the function body. v0.31 will run
   the body through the SIR interpreter + cross-check that the
   resulting `Suite` matches the frontmatter (catches frontmatter
   drift). Requires the SIR interpreter to handle `std.eval.*`
   constructors as callables — partially shipped in v0.27 (the typed
   surface lives in stdlib).
3. **Per-cell latency.** The streamed CLI line carries a placeholder
   `lat=` field today; the runner needs to capture per-cell wall-clock
   timing inside `Runner::run_matrix` for it to be real.

See `dev/history/notes/MTY_TEST_EVAL_V0_30_NOTES.md` for the design
discussion that produced this shape.
