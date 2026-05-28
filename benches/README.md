# Mighty cross-language benchmark comparators

This directory holds the **cross-language microbenchmark comparators**
used by [`docs/benchmarks/*.md`](../docs/benchmarks/index.md) to put
Mighty's numbers in context against idiomatic Rust, Go, and C++.

Each category sub-directory contains one implementation per ecosystem
of the same workload shape (not the same API). The goal is to ask
"how fast is Mighty's primitive vs the host language's idiomatic
equivalent?", not "how fast is a port of Mighty's API in language X?".

## Scope vs `bench/swe/`

Two benchmark trees live in this repo and they serve different
purposes — don't confuse them:

| Tree | Purpose | What it measures |
|---|---|---|
| `benches/` (here) | **Language-level microbenchmarks** | Parser throughput, mailbox latency, HTTP round-trip, wasm size, etc. — host-language vs Mighty primitives. |
| [`bench/swe/`](../bench/swe/README.md) | **Agentic LLM benchmarks** | SWE-bench Verified harness — measures how the Mighty agent framework solves real GitHub issues end-to-end. |

If you're asking "how fast does Mighty parse?" you want `benches/`.
If you're asking "can a Mighty agent fix a real bug?" you want
`bench/swe/`.

## Categories

| Category | Mighty | Rust | Go | C++ |
|---|---|---|---|---|
| [`parse_throughput/`](parse_throughput/) | `crates/mty-bench` | `logos` + manual | `bufio.Scanner` | hand-written |
| [`agent_send_latency/`](agent_send_latency/) | tokio mailbox | tokio mpsc | unbuffered chan | asio coroutines |
| [`mailbox_throughput/`](mailbox_throughput/) | tokio mailbox | tokio mpsc | buffered chan | SPSC ring |
| [`http_server_throughput/`](http_server_throughput/) | `std.http` | hyper | `net/http` | cpp-httplib / sockets |
| [`compile_to_native/`](compile_to_native/) | `mty build` | `rustc` | `go build` | `clang++ -O2` |
| [`wasm_size/`](wasm_size/) | `mty build --target wasi` | `cargo build --target wasm32` | TinyGo | Emscripten |

Each implementation depends on its host toolchain. The
[`run.sh`](run.sh) helper auto-detects which toolchains are installed
and runs only those, so a developer with just Rust + Python can still
exercise the Rust and Mighty paths.

## Running

```bash
./benches/run.sh           # whatever toolchains are available
./benches/run.sh --rust    # rust comparators only
./benches/run.sh --go      # go comparators only
./benches/run.sh --cpp     # c++ comparators only
./benches/run.sh --all     # require everything (rust + go + clang + emcc)
```

The Mighty implementations themselves are **not** invoked through
`run.sh`. They live in `crates/mty-bench` and are driven by:

```bash
cargo bench -p mty-bench                                # criterion HTML report
cargo build --release -p mty-bench
./target/release/mty-bench-runner --all --iters 30      # CLI summary + JSON
```

The numbers in [`docs/benchmarks/*.md`](../docs/benchmarks/index.md)
come from the cross-product of both.

## Per-category READMEs

Each subdirectory has its own README with the build commands for each
language impl:

- [`agent_send_latency/README.md`](agent_send_latency/README.md)
- [`compile_to_native/README.md`](compile_to_native/README.md)
- [`http_server_throughput/README.md`](http_server_throughput/README.md)
- [`mailbox_throughput/README.md`](mailbox_throughput/README.md)
- [`parse_throughput/README.md`](parse_throughput/README.md)
- [`wasm_size/README.md`](wasm_size/README.md)

## Honesty contract

These are **research-grade comparators**, not production benchmarks.
The numbers recorded in `docs/benchmarks/*.md` were collected as a
v0.6 baseline — see the callouts at the top of each result page. If
you want trustworthy current numbers, run the suite on your own
hardware via `./benches/run.sh`.

If a comparator outperforms Mighty by 30%+ on a category, the
corresponding `docs/benchmarks/<category>.md` says so explicitly and
flags the optimisation target. Mighty is not claiming parity across
the board.

## Rendered results

The rendered, narrated results live under
[`docs/benchmarks/index.md`](../docs/benchmarks/index.md).
