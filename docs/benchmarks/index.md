# Mighty cross-language microbenchmark results

> **Last refreshed: v0.6 baseline (2026-05-24).** The recorded numbers
> below were collected against Mighty v0.6 and have **not** been
> rerun against the current release (v0.31). The comparator code in
> `benches/` is current and ready to run on any host; the numbers
> themselves are stale. To collect current measurements, see
> [How to rerun](#how-to-rerun) below.

This page is the canonical landing for the **language-level
microbenchmarks** that put Mighty's performance in context against
idiomatic C++, Rust, and Go.

For the **agentic LLM benchmark** (SWE-bench Verified end-to-end
issue-resolution harness), see [`bench/swe/`](../../bench/swe/README.md)
instead — that's a different concern with a different cadence.

## Categories

| Category | What we measure | Doc |
|---|---|---|
| Parse throughput | Lex+parse a 10 KLOC Mighty file | [parse_throughput.md](parse_throughput.md) |
| Agent send latency | One-shot message latency between two tasks | [agent_send_latency.md](agent_send_latency.md) |
| Mailbox throughput | Messages/sec, one producer one consumer | [mailbox_throughput.md](mailbox_throughput.md) |
| HTTP server throughput | Requests/sec, GET small body | [http_server_throughput.md](http_server_throughput.md) |
| Compile to native | Build time for ~1 KLOC Mighty → wasm | [compile_to_native.md](compile_to_native.md) |
| Wasm size | Output size for a 50-unit fixture | [wasm_size.md](wasm_size.md) |

## Headline summary (v0.6 baseline)

(all numbers are median over 20–30 runs on the host described below;
**not refreshed for v0.31**)

| Category | Mighty v0.6 | Notes |
|---|---|---|
| Agent send latency | ~0.4 µs | Sub-µs P50 on a single tokio task |
| Mailbox 1k msgs | 0.23 ms | ~4.4M msgs/sec single-thread |
| HTTP GET round-trip | 0.24 ms | In-process, std.http serve_in_memory |
| Parse 10 KLOC | 6.2 ms | ~20 MB/s logos-based pipeline |
| Compile 1 KLOC → wasm | 7.9 ms | wasm-core release |
| Wasm size (50 units) | 2068 bytes | core module, no debug info |

These were first-cut numbers in v0.6. They are **not** a claim of
being faster than C++/Rust/Go on every workload — see each category
page for the honest interpretation against the cross-language
comparators.

## Environment (v0.6 baseline recording)

| | |
|---|---|
| Host | Windows 11 Home 10.0.26200 |
| CPU | (host's default, captured at bench time) |
| RAM | (host's default, captured at bench time) |
| Mighty | v0.6 prep, commit `a678e41+` |
| Toolchain | rustc 1.95.0 (cargo 1.95.0) |
| Bench harness | criterion 0.5 + `mty-bench-runner` |

C++/Go comparators were **not run** on this host (the toolchains were
not installed at v0.6 recording time). Their impls ship as code; the
methodology page describes the reference environment.

## How to rerun

The comparator code in `benches/` is **current** — it builds and
runs against today's toolchains. Only the recorded numbers on this
page are stale.

To run on your own hardware:

```bash
# Cross-language comparators (auto-detects available toolchains):
./benches/run.sh

# Rust comparators only (no Go or C++ toolchain required):
./benches/run.sh --rust

# Everything (requires rust + go + clang + emcc):
./benches/run.sh --all
```

And the Mighty side:

```bash
# Criterion harness (full HTML report under target/criterion/):
cargo bench -p mty-bench

# Lightweight CLI summary + JSON for downstream tooling:
cargo build --release -p mty-bench
./target/release/mty-bench-runner --all --iters 30 \
    --out target/bench-results.json
```

See [methodology.md](methodology.md) for what's measured, what's
*not* measured (warmup, cold caches, etc.), and how the comparator
impls were chosen.

The per-category READMEs under `benches/<category>/README.md`
describe each impl's build command in detail.

## Honesty contract

- If Mighty loses, the category page says so explicitly.
- Each loss was tagged with an optimisation issue at recording time.
- Comparator numbers from a reference environment are clearly
  labelled "(Reference env)" so they're never confused with on-host
  numbers.
- We do **not** retroactively edit recorded numbers to flatter a
  later release. The v0.6 baseline stays as v0.6 until somebody
  reruns the suite on a documented host and publishes a refresh.

Interpretation calls (why we picked each comparator, what's included
vs excluded) for the original baseline are in
`BENCHMARKS_V0_6_NOTES.md` at the repo root.

## v0.8 update — performance backlog status

| Target                  | Status      | Microbench location                                       |
|-------------------------|-------------|-----------------------------------------------------------|
| Parse throughput        | LANDED      | `crates/mty-syntax/benches/lex_throughput.rs`             |
| Mailbox throughput      | LANDED      | `crates/mty-runtime/benches/mailbox_throughput.rs`        |
| Agent send latency      | LANDED      | `crates/mty-runtime/benches/agent_send_latency.rs`        |
| Compile to native       | PARTIAL     | `crates/mty-codegen-cranelift/benches/typeck_parallel.rs` |
| HTTP server throughput  | OUT-OF-SCOPE | (owned by loose-ends agent v0.8 swarm)                   |
| Wasm size               | OUT-OF-SCOPE | (no perf-swarm optimisations in v0.8)                    |

v0.8 interpretation log: `BENCHMARKS_V0_8_NOTES.md`.

## Related benchmarks

- [`bench/swe/`](../../bench/swe/README.md) — Mighty SWE-bench
  Verified harness. End-to-end issue-resolution benchmark for the
  agent framework, completely separate from these microbenchmarks.
