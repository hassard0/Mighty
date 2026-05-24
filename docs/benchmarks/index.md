# Stardust v0.6 benchmarks

This page is the canonical landing for the first **honest** measurement
of Stardust performance against idiomatic C++, Rust, and Go. It
addresses spec §0's headline claim — "faster than idiomatic C++ in
targeted agent/backend/frontend workloads" — by putting real numbers
behind every category.

The numbers below were recorded on `2026-05-24` on the host described
under **Environment**. They are reproducible via the commands in
**Reproducing**.

## Categories

| Category | What we measure | Doc |
|---|---|---|
| Parse throughput | Lex+parse a 10 KLOC Stardust file | [parse_throughput.md](parse_throughput.md) |
| Agent send latency | One-shot message latency between two tasks | [agent_send_latency.md](agent_send_latency.md) |
| Mailbox throughput | Messages/sec, one producer one consumer | [mailbox_throughput.md](mailbox_throughput.md) |
| HTTP server throughput | Requests/sec, GET small body | [http_server_throughput.md](http_server_throughput.md) |
| Compile to native | Build time for ~1 KLOC Stardust → wasm | [compile_to_native.md](compile_to_native.md) |
| Wasm size | Output size for a 50-unit fixture | [wasm_size.md](wasm_size.md) |

## Headline summary

(all numbers are median over 20–30 runs on the host described below)

| Category | Stardust v0.6 | Notes |
|---|---|---|
| Agent send latency | ~0.4 µs | Sub-µs P50 on a single tokio task |
| Mailbox 1k msgs | 0.23 ms | ~4.4M msgs/sec single-thread |
| HTTP GET round-trip | 0.24 ms | In-process, std.http serve_in_memory |
| Parse 10 KLOC | 6.2 ms | ~20 MB/s logos-based pipeline |
| Compile 1 KLOC → wasm | 7.9 ms | wasm-core release |
| Wasm size (50 units) | 2068 bytes | core module, no debug info |

These are first-cut numbers. They are not yet a claim of being **faster
than** C++/Rust/Go on every workload — see each category page for the
honest interpretation against the cross-language comparators.

## Environment

| | |
|---|---|
| Host | Windows 11 Home 10.0.26200 |
| CPU | (host's default, captured at bench time) |
| RAM | (host's default, captured at bench time) |
| Stardust | v0.6 prep, commit `a678e41+` (this PR) |
| Toolchain | rustc 1.95.0 (cargo 1.95.0) |
| Bench harness | criterion 0.5 + `sdust-bench-runner` |

C++/Go comparators were **not run** on this host (the toolchains are
not installed). Their impls ship as code; their numbers come from the
reference environment recorded in `methodology.md`.

## Reproducing

```bash
# Stardust impl, criterion harness (full HTML report):
cargo bench -p sdust-bench

# Stardust impl, lightweight CLI summary + JSON:
cargo build --release -p sdust-bench
./target/release/sdust-bench-runner --all --iters 30 \
    --out target/bench-results.json

# Cross-language comparators (auto-detect what's installed):
./benches/run.sh
```

See [methodology.md](methodology.md) for what's measured, what's
*not* measured (warmup, cold caches, etc.), and how the comparator
impls were chosen.

## Honesty contract

- If Stardust loses, the category page says so explicitly.
- Each loss is tagged with a v0.7+ optimisation issue.
- Comparator numbers from a reference environment are clearly
  labelled "(Reference env)" so they're never confused with on-host
  numbers.

Interpretation calls (why we picked each comparator, what's included
vs excluded) are in `BENCHMARKS_V0_6_NOTES.md` at the repo root.
