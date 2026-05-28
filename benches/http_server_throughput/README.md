# `http_server_throughput/` — HTTP/1.1 GET round-trip

## What we measure

Sequential HTTP/1.1 `GET` round-trips against an in-process server
on the loopback interface, connection-per-request, with a tiny
response body (`"ok"`). We time each round-trip and report
median / p95 / p99.

This matches Mighty's `std.http` slice-7 MVP shape (no keep-alive,
no router, no header parsing beyond the request line). Spec §0 backend
workload.

| Knob | Value |
|---|---|
| Protocol | HTTP/1.1, connection per request |
| Body | `"ok"` (2 bytes) |
| Iterations (CLI runner) | 30 (single-round-trip per iter) |
| Transport | loopback TCP |

## Layout

```
http_server_throughput/
├── rust-hyper/        # hyper 1.x + service_fn + tokio
│   ├── Cargo.toml
│   └── src/main.rs
├── go-stdhttp/        # net/http + httptest server
│   ├── go.mod
│   └── main.go
└── cpp-cppserver/     # POSIX sockets, single-threaded
    ├── Makefile
    └── main.cpp
```

Each impl is standalone — its own `Cargo.toml` / `go.mod` / `Makefile`,
no shared workspace.

## Building and running

**Rust (hyper):**

```bash
cd rust-hyper && cargo run --release -- 30
```

**Go (net/http):**

```bash
cd go-stdhttp && go run main.go --iters 30
```

**C++ (POSIX sockets):**

```bash
cd cpp-cppserver && make run
```

Or, from the repo root, run all available toolchains at once:

```bash
./benches/run.sh   # auto-detects rust / go / c++
```

## Output shape

Each impl prints a single line of the form:

```
<lang>_http_server_throughput: median=X.XXX ms  p95=X.XXX ms  p99=X.XXX ms
```

stable enough for the doc-rendering scripts to parse.

## What NOT to expect

- These are **research-grade comparators**, not production
  benchmarks. The connection-per-request shape is dominated by
  loopback TCP setup at this granularity — a real keep-alive load
  test would tell a different story.
- The numbers in [`docs/benchmarks/http_server_throughput.md`](../../docs/benchmarks/http_server_throughput.md)
  are a **v0.6 baseline** and have not been refreshed against current
  Mighty. Run the suite locally if you want trustworthy current
  numbers.
- The C++ "comparator" is a hand-written socket loop, not a real
  HTTP server. It's a lower-bound number, not an apples-to-apples
  comparison against a production HTTP stack.

## Result page

Rendered numbers + interpretation:
[`../../docs/benchmarks/http_server_throughput.md`](../../docs/benchmarks/http_server_throughput.md).

The Mighty side of this comparison runs through
`sdust_runtime::http::serve_in_memory` and is driven by
`mty-bench-runner --category http-server-throughput`.
