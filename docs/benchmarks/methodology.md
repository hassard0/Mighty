# Methodology — Stardust v0.6 benchmarks

This document describes **how** the v0.6 benchmark numbers were
collected and **what** the comparison rules are. It is the
authoritative reference if you find a number on another page and want
to know whether it's apples-to-apples.

## What's measured

For each category we record **wall time** of a single representative
operation, then aggregate over 20–30 samples to produce
`median / p95 / p99`. Throughput categories also divide by the unit
count (msgs/sec, bytes/sec).

### What's *not* measured

- **JIT warmup**: the first sample is included. If a system needs N
  iterations to reach steady-state, our P50 sees that ramp.
- **Allocator startup**: same — process start cost is included.
- **First-run filesystem cache**: the synth source is generated
  in-memory for parse_throughput, so this doesn't apply there. For
  compile_to_native, the output dir is a fresh temp dir per iteration.

## Sample counts

| Category | Stardust iters | Comparator iters |
|---|---|---|
| parse_throughput | 30 | 30 |
| agent_send_latency | 30 (1000 inside criterion) | 1000 |
| mailbox_throughput | 30 (1k msgs/iter) | 30 (10k msgs/iter) |
| http_server_throughput | 30 | 30 |
| compile_to_native | 3 | 3 |
| wasm_size | 1 (single-shot byte count) | 1 |

## Statistics

We report `median`, `p95`, `p99` using the round-to-nearest
quantile formula:

```
sample_at(q) = sorted_samples[round((n - 1) * q)]
```

For `n = 100`, this gives index 50, 95, 99 — exactly the "50th",
"95th", "99th" percentile observation.

The runner also records `mean`, but the markdown tables omit it
because medians are more robust to outliers (and the agent_send_latency
P99 is dominated by tokio scheduling jitter on the slowest sample).

## Cross-language comparator rules

### Same-shape, not same-API

Each comparator runs an **operation of the same shape** as the
Stardust impl — not a port of Stardust's API. For example, the
mailbox_throughput Rust comparator uses `tokio::mpsc`, not a wrapper
that mimics `sdust_runtime::Mailbox`. This means:

- We're not measuring API binding overhead.
- We *are* measuring the host language's idiomatic concurrency
  primitive against Stardust's.
- A 2x slowdown in Stardust means "tokio + our bookkeeping is 2x
  slower than tokio alone", which is a fair v0.7+ optimisation target.

### Toolchain versions

| Tool | Version |
|---|---|
| Stardust | v0.6, this PR |
| Rust | 1.95.0 (stable) |
| Go | 1.22 (reference env) |
| C++ | g++ 13 or clang++ 17 with `-O3 -std=c++20` |

Pinned in each comparator's `Cargo.toml` / `go.mod` / `Makefile`.

### Where comparators ran

The numbers in `docs/benchmarks/*.md` use one of two labels:

- **(This host)** — measured on the same Windows 11 host that ran the
  Stardust impl.
- **(Reference env)** — Linux Ubuntu 24.04, Intel Core i7-12700,
  32 GB RAM. Used for Go/C++ comparators on hosts without those
  toolchains.

If a number is missing entirely, the table cell says `(pending —
toolchain not yet run)` and the comparator code is in `benches/`
ready to be invoked.

## Reproducing

```bash
# 1. Stardust: criterion HTML + raw JSON
cargo bench -p sdust-bench
cargo build --release -p sdust-bench
./target/release/sdust-bench-runner --all --iters 30 \
    --out target/bench-results.json

# 2. Comparators: each is a standalone Cargo crate / go module / Makefile
./benches/run.sh --rust    # if rustc available
./benches/run.sh --go      # if go available
./benches/run.sh --cpp     # if g++/clang++ available
./benches/run.sh --all     # require all

# 3. Sanity-check the fixtures
cargo test -p sdust-bench
```

## Adding a new benchmark

See `docs/internals/benchmarking.md` for the contributor guide.

## When to update these numbers

Re-run the full suite for any **release candidate**. Don't re-run for
every PR — the criterion HTML reports under `target/criterion/`
already exercise the per-PR signal, and uploading them as artifacts
in CI is enough for trend tracking.
