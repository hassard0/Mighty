# v0.6 benchmarks — interpretation calls

This is the **agent log** for the v0.6 honest-benchmarks swarm. It
records the calls we made, the comparators we picked, what's missing,
and the v0.7+ optimisation backlog.

For numbers, see `docs/benchmarks/`.
For methodology, see `docs/benchmarks/methodology.md`.
For the contributor guide, see `docs/internals/benchmarking.md`.

## Calls

### Why these six categories

Spec §0 lists "agent / backend / frontend / tooling" as the four
target workloads. We collapsed those into six measurable categories
because:

- "Agent" is two operations (`send latency` ↔ `mailbox throughput`).
- "Backend" maps cleanly to `http_server_throughput`.
- "Frontend" maps to `wasm_size` (lean output is the headline) and
  `agent_send_latency` (SPA event-loop responsiveness).
- "Tooling" maps to `parse_throughput` (LSP / fmt) and
  `compile_to_native` (edit-compile cycle).

Six categories is a tractable v0.6 deliverable. v0.7 should add at
least one **end-to-end** benchmark (e.g. "spawn 1000 agents, send 1k
messages each, measure total wall time") so the per-primitive numbers
have an aggregate to anchor.

### Why these comparators

| Category | Comparator choice | Why |
|---|---|---|
| parse_throughput | Rust/Go/C++ hand-written lexers | "Mighty's lexer + CST" vs "idiomatic lexer". Rust's logos comparator is a near-identical control. |
| agent_send_latency | tokio mpsc / Go chan / asio coro | All three are the "best in class" inter-task primitive for their language. |
| mailbox_throughput | same as above | One producer + one consumer = simplest fair shape. |
| http_server_throughput | hyper / net/http / cpp-httplib | The default HTTP stack each ecosystem reaches for. |
| compile_to_native | rustc / go / clang | The compilers themselves, not just their codegen libs. |
| wasm_size | wasm32-rust / TinyGo / Emscripten | The wasm-targeting toolchains for each. |

We did **not** pick:

- Akka / Erlang (too far from Mighty's tokio-based shape).
- Nginx / Envoy (too much config required to be apples-to-apples).
- LLVM directly (rustc already wraps it; same for clang).

### Why we used the wasm-core backend for `compile_to_native`

The native cranelift backend requires an external linker. On Windows
that's `link.exe` from MSVC, which isn't always installed. The
wasm-core backend goes through the same `parse → lower → typeck →
borrowck → MtyIR → emit` pipeline minus the link step, so it's the
most portable measure of the compiler's hot path.

For a true "native" number, swap the runner's `BuildTarget::Wasm`
for `BuildTarget::Native` on a host with `link.exe` / `ld`.

### Why on-host comparator runs are pending

The Windows 11 host this swarm ran on has:

- ✅ Rust 1.95.0
- ✅ Python 3.11
- ❌ Go
- ❌ g++ / clang++
- ❌ TinyGo / Emscripten

Plus disk space hit 100% during the swarm's run. So **all comparator
impls ship as code** in `benches/<category>/<lang>/`. The "Reference
env" numbers will be filled in by a later run on a host with all
toolchains installed.

This is documented as `(pending — Reference env)` in every category
table, not as a fabricated number.

### Why ~30 iters

Criterion's default is 100; we used 30 for the CLI runner because:

- The runner is a quick-look tool; for publication-quality use
  `cargo bench` directly (which gets 100+ iters via criterion).
- 30 is enough to stabilise the median (the P99 still has noise
  from scheduler jitter; documented).

## What's missing

### Not measured in v0.6

- **JIT warmup**: included in every sample. For hot-path agents
  this overstates the per-message cost.
- **Allocator first-fit cost**: same — we don't pre-warm the heap.
- **Cold-cache parse**: the synth source is in-memory. A real
  on-disk + first-syscall parse would be 1-3x slower.
- **Concurrent agents**: every benchmark uses 1 sender + 1 receiver
  + 1 server. Multi-agent contention is a separate workload.
- **Multi-core scaling**: the runtime defaults to single-threaded
  in benches. v0.7 should add a parallel mailbox bench.

### Comparator gaps

- The C++ asio comparator falls back to a `condition_variable` shape
  if `<asio.hpp>` isn't on the include path. The fallback is slower
  than asio's coroutines; documented in the source.
- The C++ HTTP comparator builds POSIX-sockets-only. On Windows it
  prints a "build on POSIX" message and exits. A winsock variant is
  TODO.

## v0.7+ optimisation backlog

Each item references the category doc that motivates it.

### parse_throughput

- [ ] Token caching between LSP and incremental re-parse
- [ ] Arena green nodes (drop the per-node `Arc`)
- [ ] Single-pass diag throttle (cap at N errors)

### agent_send_latency

- [ ] Skip slab admission for empty payloads
- [ ] Thread-local arena for hot agents' slabs
- [ ] Inline fast path for single-sender mailboxes

### mailbox_throughput

- [ ] Batched `try_recv_many`
- [ ] Lock-free mpsc opt-in (today: bounded `tokio::sync::mpsc`)
- [ ] Slab inline cache for uniform-size message bursts

### http_server_throughput

- [ ] Keep-alive support
- [ ] Header parsing on the MtyIR side (today: hardcoded request line)
- [ ] Opt-in `hyper` backend for HTTP/2

### compile_to_native

- [ ] Parallel type-checker
- [ ] Pre-built stdlib metadata cache
- [ ] Incremental compilation in the IDE path

### wasm_size

- [ ] Function deduplication (CSE-style)
- [ ] Constant-folding pass before emission
- [ ] gzip custom sections

## Acceptance checklist

- [x] `cargo bench -p mty-bench` runs cleanly
- [x] `cargo test -p mty-bench` passes (8 tests)
- [x] `cargo build -p mty-cli` not broken
- [x] `cargo clippy -p mty-bench --all-targets -- -D warnings` clean
  (for mty-bench itself; lib-crate warnings come from the scheduler
  swarm's WIP and are out of our scope)
- [x] All 6 categories have at least the Mighty impl + 1 comparator
  shipped as code
- [x] Documented numbers in `docs/benchmarks/*.md` are real (from a
  real run on this host)

## Files added

```
crates/mty-bench/
  Cargo.toml
  src/{lib,fixtures,http,metrics}.rs
  src/bin/mty-bench-runner.rs
  benches/{parse_throughput,agent_send_latency,mailbox_throughput,
           http_server_throughput,compile_to_native,wasm_size}.rs
  tests/{fixture_load,criterion_smoke}.rs

benches/
  README.md
  run.sh
  .gitignore
  parse_throughput/{mighty,rust,go,cpp}/...
  agent_send_latency/{rust-tokio,go-channels,cpp-asio}/...
  mailbox_throughput/{rust-tokio,go-channels,cpp-asio}/...
  http_server_throughput/{rust-hyper,go-stdhttp,cpp-cppserver}/...
  compile_to_native/{generate_sources.sh,README.md}
  wasm_size/README.md

docs/benchmarks/
  index.md
  methodology.md
  parse_throughput.md
  agent_send_latency.md
  mailbox_throughput.md
  http_server_throughput.md
  compile_to_native.md
  wasm_size.md

docs/internals/benchmarking.md

.github/workflows/bench.yml      (added)

BENCHMARKS_V0_6_NOTES.md         (this file)
```
