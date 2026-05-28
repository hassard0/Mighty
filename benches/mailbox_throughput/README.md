# `mailbox_throughput/` — one producer one consumer, drain N messages

## What we measure

One producer task fills a bounded mailbox; one consumer task drains
it. We time the whole drain and report median / p95 / p99 over many
iterations, plus a derived msgs/sec at the median.

This is the steady-state pipe that backs every Mighty agent that
streams work between stages. Spec §0 agent-first headline (throughput
side).

| Knob | Value |
|---|---|
| Producers / consumers | 1 / 1 |
| Mailbox capacity | matches batch size (no blocking on send) |
| Messages per iter | 1 000 (CLI runner) or 10 000 (criterion) |
| Payload | one `int`/`usize`-sized message |

## Layout

```
mailbox_throughput/
├── rust-tokio/        # tokio::sync::mpsc, bounded
│   ├── Cargo.toml
│   └── src/main.rs
├── go-channels/       # buffered chan (`make(chan int, 10000)`)
│   ├── go.mod
│   └── main.go
└── cpp-asio/          # lock-free SPSC ring (lower-bound)
    ├── Makefile
    └── main.cpp
```

Each impl is standalone — its own `Cargo.toml` / `go.mod` / `Makefile`,
no shared workspace.

## Building and running

**Rust (tokio):**

```bash
cd rust-tokio && cargo run --release -- 30
```

**Go (channels):**

```bash
cd go-channels && go run main.go --iters 30
```

**C++ (SPSC ring):**

```bash
cd cpp-asio && make run
```

Or, from the repo root, run all available toolchains at once:

```bash
./benches/run.sh   # auto-detects rust / go / c++
```

## Output shape

Each impl prints a single line of the form:

```
<lang>_mailbox_throughput: median=X.XXX ms  p95=X.XXX ms  p99=X.XXX ms
```

stable enough for the doc-rendering scripts to parse.

## What NOT to expect

- These are **research-grade comparators**, not production
  benchmarks. The C++ "comparator" is a single-thread SPSC ring with
  no synchronisation — a deliberate lower-bound number, strictly
  faster than any blocking channel.
- The numbers in [`docs/benchmarks/mailbox_throughput.md`](../../docs/benchmarks/mailbox_throughput.md)
  are a **v0.6 baseline** and have not been refreshed against current
  Mighty. Run the suite locally if you want trustworthy current
  numbers.
- The single-producer / single-consumer shape doesn't exercise
  contention. Multi-producer numbers would look quite different.

## Result page

Rendered numbers + interpretation:
[`../../docs/benchmarks/mailbox_throughput.md`](../../docs/benchmarks/mailbox_throughput.md).

The Mighty side of this comparison is driven by
`crates/mty-runtime/benches/mailbox_throughput.rs` (criterion) and the
`mty-bench-runner --category mailbox-throughput` CLI summary.
