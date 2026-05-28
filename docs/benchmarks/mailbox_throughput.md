# mailbox_throughput

> **Last refreshed: v0.33 (2026-05-28) on vulcan** (Dell, Intel Xeon,
> Ubuntu 24.04, Rust 1.95.0). Mighty + Rust-tokio comparator numbers
> are v0.33; Go + C++ comparators retain the v0.6 baseline pending a
> comparator toolchain refresh on the benchmark host.

**Workload:** one producer task, one consumer task, drain 1 000 (CLI)
or 10 000 (criterion) `MessageFrame`s through a bounded mailbox.

**Spec alignment:** §0 agent-first headline (steady-state pipe).

## Numbers

| Impl | Median | p95 | p99 | Msgs/sec (median) | Notes |
|---|---|---|---|---|---|
| Mighty v0.33 mailbox (1k msgs) | 0.24 ms | 0.24 ms | 0.25 ms | ~4.2M/sec | tokio mpsc + slab |
| Mighty v0.33 mailbox (10k msgs, criterion) | (criterion bench) | | | | |
| Rust tokio mpsc (10k msgs) | 1.28 ms | 1.31 ms | 1.33 ms | ~7.8M/sec | bare mpsc, no slab — vulcan |
| Go buffered chan (10k msgs) | (pending — Reference env) | | | | |
| C++ SPSC lock-free ring (10k msgs) | (pending — Reference env) | | | | will be the fastest by a wide margin |

### Recorded values (vulcan, 2026-05-28, v0.33)

```
mailbox_throughput        median=     0.235 ms  p95=     0.244 ms  p99=     0.251 ms  (1k msgs/iter)
rust_tokio_mailbox        median=     1.282 ms  p95=     1.308 ms  p99=     1.331 ms  (10k msgs/iter)
```

With 1 000 msgs/iter: **median ≈ 4.2M msgs/sec single-threaded** —
about 54% of bare Rust tokio mpsc on the same host, which is the
slab admission overhead the prior baseline anticipated.

### v0.6 baseline (Windows 11 dev laptop, 2026-05-24)

For continuity: Mighty v0.6 measured **median = 0.23 ms** for 1k
msgs (~4.4M msgs/sec). Cross-host deltas are shape, not absolute.

## Interpretation

The Mighty mailbox is **tokio mpsc + a 64-byte slab admission
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

## v0.8 update

| Optimisation               | Status | Delta                                                                          |
|----------------------------|--------|--------------------------------------------------------------------------------|
| `try_recv_many`            | DONE   | exported from `mty_runtime::mailbox`; batched_recv ~7-8% faster than single_recv on the 10k empty-payload bench |
| Slab inline cache          | DONE   | 64-byte stack buffer for descriptor admit (non-empty payload path)             |
| Lock-free mpsc opt-in      | DEFER  | crossbeam_channel feature flag would grow API surface without measured wins on the single-producer single-consumer shape today |

Microbench: `crates/mty-runtime/benches/mailbox_throughput.rs`
(single_recv_empty_payload vs batched_recv_empty_payload).
Interpretation log: `BENCHMARKS_V0_8_NOTES.md`.

The criterion `--quick` numbers (3.5M elem/s vs 3.8M elem/s) are
lower than the v0.6 baseline of 4.4M msgs/sec because the v0.6
shape used 1000-msg batches via a CLI runner with less per-batch
overhead. Same-host before/after measurement is the meaningful
comparison; cross-version comparison needs a quiet-host re-run
(deferred — see v0.8 notes).
