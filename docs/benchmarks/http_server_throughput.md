# http_server_throughput

> **Last refreshed: v0.33 (2026-05-28) on vulcan** (Dell, Intel Xeon,
> Ubuntu 24.04, Rust 1.95.0). Mighty numbers are v0.33; comparator
> rows pending. The Rust+Hyper comparator has a pre-existing compile
> error against the v0.33 toolchain (tracked in v0.34 backlog —
> `http-server-throughput-rust-hyper` `BodyExt::collect` E0790).

**Workload:** HTTP/1.1 GET round-trip on the in-process `std.http`
server (`sdust_runtime::http::serve_in_memory`). Connection per
request, small body ("ok").

**Spec alignment:** §0 backend workload.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.33 std.http (in-process) | 0.11 ms | 0.23 ms | 0.34 ms | bare TCP read/write loop |
| Rust + Hyper (in-process) | (comparator broken — see callout) | | | hyper 1.x service_fn |
| Go stdlib net/http (httptest) | (pending — Reference env) | | | net/http handler |
| C++ POSIX sockets (in-process) | (pending — Reference env) | | | bare socket loop |

### Recorded values (vulcan, 2026-05-28, v0.33)

```
http_server_throughput median=     0.106 ms  p95=     0.227 ms  p99=     0.338 ms
```

The 56% drop vs v0.6 is mostly the host change (vulcan's Xeon vs
the v0.6 dev laptop). Sequential 100-GET batch (criterion bench):
see `target/criterion/` HTML report.

### v0.6 baseline (Windows 11 dev laptop, 2026-05-24)

For continuity: Mighty v0.6 measured **median = 0.24 ms**.
Cross-host deltas are shape, not absolute.

## Interpretation

Mighty's `std.http` is a slice-7 MVP: a `tokio::TcpListener` per
serve_in_memory, accept loop spawns one task per connection, reads
up to 4 KB into a stack buffer, parses the request line, writes a
fixed status+body. It's intentionally tiny — there's no router, no
keep-alive, no header parsing beyond the request line.

Expected comparator outcome:

- **Hyper**: slightly faster (~10-20%) because of `tokio::io::copy`
  fast paths and pre-allocated header buffers.
- **Go net/http**: slightly slower (~10-30%) because Go's GC kicks
  in and the runtime has more per-conn allocator pressure.
- **C++ bare sockets**: the fastest by 2-3x. No allocator at all.

When the comparator numbers land, we'll quantify exactly. Don't
read too much into the on-host number alone — 0.24 ms includes
loopback TCP, which is the dominant cost at this granularity.

## v0.7+ optimisation targets

- **Keep-alive support** so the comparison covers the request-rate,
  not connection-setup-rate (today loopback TCP setup dominates).
- **Header parsing on the MtyIR side** rather than a hardcoded request
  line — moves us closer to a "real" HTTP impl.
- **`hyper` backend as an opt-in alternative** for users who want
  HTTP/2.

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § HTTP Server Throughput.
