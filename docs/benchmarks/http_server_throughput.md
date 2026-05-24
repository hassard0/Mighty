# http_server_throughput

**Workload:** HTTP/1.1 GET round-trip on the in-process `std.http`
server (`sdust_runtime::http::serve_in_memory`). Connection per
request, small body ("ok").

**Spec alignment:** §0 backend workload.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.6 std.http (in-process) | 0.24 ms | 0.32 ms | 0.43 ms | bare TCP read/write loop |
| Rust + Hyper (in-process) | (pending — Reference env) | | | hyper 1.x service_fn |
| Go stdlib net/http (httptest) | (pending — Reference env) | | | net/http handler |
| C++ POSIX sockets (in-process) | (pending — Reference env) | | | bare socket loop |

### Recorded values (this host, 2026-05-24)

```
http_server_throughput median=     0.235 ms  p95=     0.319 ms  p99=     0.425 ms
```

Sequential 100-GET batch (criterion bench): see `target/criterion/`
HTML report.

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
