# `agent_send_latency/` — one fire-and-forget message between two tasks

## What we measure

One **fire-and-forget message** between a sender and a receiver on
the same async runtime, with a mailbox capacity of `8`. We time the
send + recv pair over many iterations and report median / p95 / p99
nanoseconds.

This is the primitive every Mighty agent uses to talk to another
agent. Spec §0 calls it out as the agent-first headline.

| Knob | Value |
|---|---|
| Mailbox capacity | 8 |
| Payload | one `int`/`usize`-sized message |
| Concurrency | sender + receiver in the same runtime |
| Iterations (CLI runner) | 1000 |

## Layout

```
agent_send_latency/
├── rust-tokio/        # tokio::sync::mpsc (bounded, capacity 8)
│   ├── Cargo.toml
│   └── src/main.rs
├── go-channels/       # buffered chan (`make(chan int, 8)`)
│   ├── go.mod
│   └── main.go
└── cpp-asio/          # asio::experimental::channel + coroutines
    ├── Makefile
    └── main.cpp
```

Each impl is standalone — its own `Cargo.toml` / `go.mod` / `Makefile`,
no shared workspace.

## Building and running

**Rust (tokio):**

```bash
cd rust-tokio && cargo run --release -- 1000
```

**Go (channels):**

```bash
cd go-channels && go run main.go --iters 1000
```

**C++ (asio):**

```bash
cd cpp-asio
# without asio installed, the Makefile falls back to a
# condition-variable shape (same workload, no coroutines):
make run
# with asio:
make ASIO_INCLUDE=/path/to/asio/include run
```

Or, from the repo root, run all available toolchains at once:

```bash
./benches/run.sh   # auto-detects rust / go / c++
```

## Output shape

Each impl prints a single line of the form:

```
<lang>_agent_send_latency: median=X.XXX ms  p95=X.XXX ms  p99=X.XXX ms
```

stable enough for the doc-rendering scripts to parse.

## What NOT to expect

- These are **research-grade comparators**, not production
  benchmarks. They measure one specific shape (single sender, single
  receiver, capacity 8) on whatever host you happen to run them on.
- The numbers in [`docs/benchmarks/agent_send_latency.md`](../../docs/benchmarks/agent_send_latency.md)
  are a **v0.6 baseline** and have not been refreshed against current
  Mighty. Run the suite locally if you want trustworthy current
  numbers.
- The cross-language gap depends heavily on the runtime's scheduler
  jitter. A P99 spike of ~10 µs is normal for tokio; it doesn't
  indicate a Mighty bug.

## Result page

Rendered numbers + interpretation:
[`../../docs/benchmarks/agent_send_latency.md`](../../docs/benchmarks/agent_send_latency.md).

The Mighty side of this comparison is driven by
`crates/mty-runtime/benches/agent_send_latency.rs` (criterion) and the
`mty-bench-runner --category agent-send-latency` CLI summary.
