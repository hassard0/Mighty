# Benchmarking — contributor guide

This page is for contributors who want to **add a new benchmark** or
**re-run an existing one** without re-deriving the methodology from
scratch.

For the published numbers themselves, see `docs/benchmarks/`. For
interpretation calls (why each comparator was chosen, what's missing),
see `BENCHMARKS_V0_6_NOTES.md` at the repo root.

## Layout

```
crates/mty-bench/
├── Cargo.toml                        # the bench crate
├── src/
│   ├── lib.rs                        # shared fixtures + http helpers
│   ├── fixtures.rs                   # synth_source, echo_sir_program
│   ├── http.rs                       # single_get / sequential_get
│   ├── metrics.rs                    # percentiles / mean
│   └── bin/
│       └── mty-bench-runner.rs     # CLI driver (criterion-less)
├── benches/
│   ├── parse_throughput.rs           # criterion harness
│   ├── agent_send_latency.rs
│   ├── mailbox_throughput.rs
│   ├── http_server_throughput.rs
│   ├── compile_to_native.rs
│   └── wasm_size.rs
└── tests/
    ├── fixture_load.rs               # smoke: 10 KLOC parses cleanly
    └── criterion_smoke.rs            # smoke: each bench runs ≥1 iter

benches/                              # cross-language comparators
├── parse_throughput/{mighty,rust,go,cpp}/
├── agent_send_latency/{...}/
├── mailbox_throughput/{...}/
├── http_server_throughput/{...}/
├── compile_to_native/                # generator script + READMEs
├── wasm_size/                        # README only
└── run.sh                            # toolchain-aware runner
```

## What goes where

| Type of impl | Location |
|---|---|
| Pure-Mighty benchmark | `crates/mty-bench/benches/<name>.rs` |
| Shared fixture / helper | `crates/mty-bench/src/<name>.rs` |
| Cross-language comparator | `benches/<category>/<lang>/` |
| CI-friendly script | `benches/run.sh` |

If you only need a tight microbenchmark of one crate's internals,
you can also put `benches/*.rs` inside that crate (e.g.
`crates/mty-runtime/benches/`). Use the top-level `benches/` only
for **cross-language comparative** benchmarks; per-crate benches are
fine for in-language tuning.

## Adding a new category

1. **Add a shared fixture** in `crates/mty-bench/src/fixtures.rs` if
   you need new test data.
2. **Add the criterion bench** at
   `crates/mty-bench/benches/<name>.rs` with `harness = false`
   in `Cargo.toml`. Use `BenchmarkId::new` to keep the report
   structure consistent.
3. **Add the runner integration** in
   `crates/mty-bench/src/bin/mty-bench-runner.rs` by extending
   the `Category` enum + adding a `run_<name>(iters)` function.
4. **Add cross-language comparators** in `benches/<name>/<lang>/` —
   at minimum a Rust comparator so the on-host number has a peer.
   Use the same source shape (same struct/fn count, same arithmetic)
   so the comparison is meaningful.
5. **Add the doc** at `docs/benchmarks/<name>.md` following the
   template (overview → numbers → interpretation → v0.7+ targets).
6. **Update `docs/benchmarks/index.md`** to list the new category.

## Running locally

```bash
# Full criterion suite (HTML report at target/criterion/report/index.html):
cargo bench -p mty-bench

# Single category:
cargo bench -p mty-bench --bench parse_throughput

# CLI runner (faster, no HTML):
cargo build --release -p mty-bench
./target/release/mty-bench-runner --category parse-throughput --iters 30
./target/release/mty-bench-runner --all --iters 30 --out target/bench.json

# Cross-language (auto-detects toolchains):
./benches/run.sh
```

## What statistics matter

For a publication-quality number, report `median / p95 / p99`. Mean
is misleading on skewed distributions — tokio scheduler jitter alone
can pull the mean by 2-5x relative to the median.

For relative comparisons across languages, prefer **median ratios**
("Mighty is 1.4x slower than Rust at median") over **mean ratios**
which are noisy.

For throughput (msgs/sec, bytes/sec), divide by the median latency
rather than the mean. This corresponds to "the rate you'd see on a
typical request" rather than "the rate amortised across worst-case
spikes".

## Honesty rules

- **Don't cherry-pick the best run.** Run 20-30 iterations and report
  the median.
- **Don't omit losing benchmarks.** If Mighty is 5x slower than
  Rust on a category, the doc says so + flags a v0.7+ issue.
- **Label numbers by environment.** Use "(This host)" vs "(Reference
  env)" — never let a reader mistake a reference-env number for an
  on-host measurement.

## CI

`.github/workflows/bench.yml` runs the **lightweight criterion suite**
on every push to `main`. It does *not* run the cross-language
comparators (the CI runner usually lacks Go/C++ toolchains). PR
comment posting + historical trend tracking are stub TODOs.

## Reproducibility checklist

- [ ] Fixture is deterministic (seeded, same output on rerun).
- [ ] Comparator runs the same shape of operation.
- [ ] Sample count documented in `methodology.md`.
- [ ] Environment recorded ("This host" vs "Reference env").
- [ ] Each category page links back to the comparator code path.
