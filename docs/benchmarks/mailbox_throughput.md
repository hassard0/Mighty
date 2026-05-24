# mailbox_throughput

**Workload:** one producer task, one consumer task, drain 1 000 (CLI)
or 10 000 (criterion) `MessageFrame`s through a bounded mailbox.

**Spec alignment:** §0 agent-first headline (steady-state pipe).

## Numbers

| Impl | Median | p95 | p99 | Msgs/sec (median) | Notes |
|---|---|---|---|---|---|
| Stardust v0.6 mailbox (1k msgs) | 0.23 ms | 0.44 ms | 0.48 ms | ~4.4M/sec | tokio mpsc + slab |
| Stardust v0.6 mailbox (10k msgs, criterion) | (criterion bench) | | | | |
| Rust tokio mpsc (10k msgs) | (pending — Reference env) | | | | |
| Go buffered chan (10k msgs) | (pending — Reference env) | | | | |
| C++ SPSC lock-free ring (10k msgs) | (pending — Reference env) | | | | will be the fastest by a wide margin |

### Recorded values (this host, 2026-05-24)

```
mailbox_throughput     median=     0.227 ms  p95=     0.436 ms  p99=     0.483 ms
```

With 1 000 msgs/iter: **median = 4.4M msgs/sec single-threaded**.

## Interpretation

The Stardust mailbox is **tokio mpsc + a 64-byte slab admission
step**. We expect:

- Within 2x of bare tokio mpsc.
- 3-5x slower than a C++ SPSC lock-free ring (which has no allocator
  pressure at all).
- Roughly tied with Go's buffered channel (Go channels have similar
  per-msg overhead to tokio mpsc).

These are first-cut expectations; the comparator numbers (when
recorded on a host with all toolchains) will confirm or refute them.

## v0.7+ optimisation targets

- **Batched recv** — `try_recv_many` to amortise the await.
- **Lock-free mpsc** as an opt-in for high-throughput agents (today
  every mailbox uses the bounded `tokio::sync::mpsc`, which holds a
  futex).
- **Slab inline cache** — if every message in a burst is the same
  size, we can avoid re-acquiring slots.

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Mailbox Throughput.
