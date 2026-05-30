# http_server_throughput

> **Last refreshed: v0.38 (2026-05-29) on vulcan** (Dell, Intel Xeon,
> 16 cores, Ubuntu 24.04, Rust 1.95.0, PGO release-pgo profile).
> Mighty + C++ POSIX-sockets comparator numbers are v0.38 (rerun on
> this host). The Rust+Hyper comparator still hits the pre-existing
> compile error (`BodyExt::collect` E0790, tracked since v0.34); fixing
> the comparator + wiring a Go toolchain on vulcan is in the v0.39
> follow-up bucket.

**Workload:** HTTP/1.1 GET round-trip on the in-process `std.http`
server (`sdust_runtime::http::serve_in_memory`). Connection per
request, small body ("ok").

**Spec alignment:** §0 backend workload.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.38 std.http (in-process) | 0.10 ms | 0.26 ms | 0.32 ms | bare TCP read/write loop |
| Rust + Hyper (in-process) | (comparator broken — see callout) | | | hyper 1.x service_fn |
| C++ POSIX sockets (in-process) | 0.10 ms | 0.13 ms | 0.28 ms | bare socket loop |
| Go stdlib net/http (httptest) | (pending — Go not installed on vulcan) | | | net/http handler |

### Recorded values (vulcan, 2026-05-29, v0.38)

```
http_server_throughput    median=     0.101 ms  p95=     0.259 ms  p99=     0.319 ms
cpp_sockets_http_server   median=     0.098 ms  p95=     0.134 ms  p99=     0.280 ms
```

The Mighty std.http median is within **3%** of the C++ POSIX-sockets
comparator at the median — both are dominated by loopback TCP
setup. The C++ impl has tighter p95/p99 because it doesn't spawn a
new tokio task per connection (Mighty's accept loop pays a tokio
task spawn per request, which adds tail-latency jitter). Holds
within noise of the v0.33 refresh (0.106 ms → 0.101 ms median —
a ~5% improvement from release-pgo's fat LTO + PGO on the accept
loop). Sequential 100-GET batch (criterion bench): see
`target/criterion/` HTML report.

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
