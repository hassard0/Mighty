# Mighty v0.6 benchmarks — comparative implementations

This directory holds the **cross-language** comparator implementations
used by `docs/benchmarks/*.md` to put Mighty's numbers in context.
Each category sub-directory contains one impl per ecosystem:

| Category | Mighty | Rust | Go | C++ |
|---|---|---|---|---|
| `parse_throughput/` | uses `crates/mty-bench` | pest+manual | bufio.Scanner | re-flex |
| `agent_send_latency/` | tokio mailbox | tokio mpsc | unbuffered chan | asio coroutines |
| `mailbox_throughput/` | tokio mailbox | crossbeam channel | buffered chan | asio strand |
| `http_server_throughput/` | std.http | hyper | net/http | cpp-httplib |
| `compile_to_native/` | mty build | rustc | go build | clang -O2 |
| `wasm_size/` | mty build --target wasi | cargo build --target wasm32 | tinygo | Emscripten |

**Each impl has its own toolchain dependency.** The host that ran the
recorded numbers in `docs/benchmarks/*.md` had only Rust + Python
installed; the Go and C++ impls ship as code + documented reference
environment (`docs/benchmarks/methodology.md`).

## Running

Use the helper script from the repo root:

```bash
./benches/run.sh           # runs whatever toolchains are available
./benches/run.sh --rust    # rust comparators only
./benches/run.sh --all     # everything (requires go + clang + emcc)
```

The Mighty impls are *not* invoked via this script — they're driven by
`cargo bench -p mty-bench` (criterion harness) and
`./target/release/mty-bench-runner --all` (CLI summary). The numbers
recorded under `docs/benchmarks/` come from the cross-product of both.

## Honesty contract

If a comparator outperforms Mighty by 30%+ on a category, the
corresponding `docs/benchmarks/<category>.md` says so explicitly and
flags a v0.7+ optimisation target. Mighty v0.6 is *not* claiming
parity across the board — see `BENCHMARKS_V0_6_NOTES.md` for the full
interpretation.
