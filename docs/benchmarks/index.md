# Mighty cross-language microbenchmark results

> **Last refreshed: v0.38 (2026-05-29).** Numbers below were rerun on
> the vulcan benchmark host (Dell, Intel Xeon, 16 cores, Ubuntu 24.04,
> Rust 1.95.0, PGO release-pgo profile). Mighty + Rust + C++
> comparator numbers are v0.38; Go comparator retains the v0.6
> baseline because the Go toolchain is not installed on vulcan
> (tracked as a v0.39 follow-up). To collect current measurements
> yourself, see [How to rerun](#how-to-rerun) below.

This page is the canonical landing for the **language-level
microbenchmarks** that put Mighty's performance in context against
idiomatic C++, Rust, and Go.

For the **agentic LLM benchmark** (SWE-bench Verified end-to-end
issue-resolution harness), see [`bench/swe/`](https://github.com/hassard0/Mighty/blob/main/bench/swe/README.md)
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

## Headline summary (v0.38)

(all numbers are median over 30 runs on vulcan; ~ symbol indicates
the value moved within noise vs the v0.33 refresh)

| Category | Mighty v0.38 | Δ vs v0.33 | Notes |
|---|---|---|---|
| Agent send latency | 0.19 µs | -8% | sub-µs P50; tokio mpsc + slab admit |
| Mailbox 1k msgs | 0.22 ms | -6% | ~4.5M msgs/sec single-thread |
| HTTP GET round-trip | 0.10 ms | -5% | in-process, std.http serve_in_memory |
| Parse 10 KLOC | 5.58 ms | -1% | ~24 MB/s logos-based pipeline |
| Compile 1 KLOC → wasm | 7.95 ms | -1% | wasm-core release-pgo path |
| Wasm size (50 units) | 2698 bytes | ~ | byte-exact reproduction; deterministic emit |

Apples-to-apples with v0.33 on the same host (vulcan), same
toolchain (rustc 1.95.0). The release-pgo profile (fat LTO + PGO
from a 30-example training corpus) trims a few ns off agent_send +
mailbox per-msg cost and ~5% off the http accept loop; parse +
compile sit inside the 1-2% noise band; wasm_size is byte-exact
(the wasm-core emitter is deterministic). They are **not** a claim
of being faster than C++/Rust/Go on every workload — see each
category page for the honest interpretation against the
cross-language comparators.

## Environment (v0.38 refresh)

| | |
|---|---|
| Host | vulcan (Dell server, Ubuntu 24.04) |
| CPU | Intel Xeon, 16 cores (NUMA, multi-socket) |
| GPU | 4× NVIDIA V100-SXM2 16GB NVLinked (not on the bench path) |
| Mighty | v0.38, commit `8c443be` |
| Toolchain | rustc 1.95.0 (cargo 1.95.0), PGO release-pgo profile (fat LTO, 1 codegen unit) |
| Bench harness | criterion 0.5 + `mty-bench-runner --all --iters 30` |

The Rust comparator was rerun on this host for v0.38 (parse,
agent-send-latency, mailbox; the hyper http server still has its
pre-existing comparator compile error tracked since v0.34). The C++
comparators were also rerun for v0.38 (parse, agent-send via cv
fallback, mailbox SPSC, http POSIX-sockets); g++ 13.3.0 is the
toolchain. The Go comparators **were not run** on vulcan because Go
is not installed there — their impls ship as code, the v0.6-baseline
pending rows on each category page are retained as such.

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

- [`bench/swe/`](https://github.com/hassard0/Mighty/blob/main/bench/swe/README.md) — Mighty SWE-bench
  Verified harness. End-to-end issue-resolution benchmark for the
  agent framework, completely separate from these microbenchmarks.
