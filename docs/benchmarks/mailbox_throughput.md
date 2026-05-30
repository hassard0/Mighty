# mailbox_throughput

> **Last refreshed: v0.38 (2026-05-29) on vulcan** (Dell, Intel Xeon,
> 16 cores, Ubuntu 24.04, Rust 1.95.0, PGO release-pgo profile).
> Mighty + Rust-tokio + C++ SPSC comparator numbers are v0.38
> (rerun on this host); Go comparator retains the v0.6 baseline
> (Go toolchain not installed on vulcan — tracked as a v0.39
> follow-up).

**Workload:** one producer task, one consumer task, drain 1 000 (CLI)
or 10 000 (criterion) `MessageFrame`s through a bounded mailbox.

**Spec alignment:** §0 agent-first headline (steady-state pipe).

## Numbers

| Impl | Median | p95 | p99 | Msgs/sec (median) | Notes |
|---|---|---|---|---|---|
| Mighty v0.38 mailbox (1k msgs) | 0.22 ms | 0.23 ms | 0.23 ms | ~4.5M/sec | tokio mpsc + slab |
| Mighty v0.38 mailbox (10k msgs, criterion) | (criterion bench) | | | | |
| Rust tokio mpsc (10k msgs) | 1.09 ms | 1.18 ms | 1.19 ms | ~9.2M/sec | bare mpsc, no slab — vulcan |
| C++ SPSC lock-free ring (10k msgs) | 0.16 ms | 0.25 ms | 0.38 ms | ~63.7M/sec | bare SPSC ring; fastest by a wide margin as expected |
| Go buffered chan (10k msgs) | (pending — Go not installed on vulcan) | | | | |

### Recorded values (vulcan, 2026-05-29, v0.38)

```
mailbox_throughput              median=     0.221 ms  p95=     0.229 ms  p99=     0.234 ms  (1k msgs/iter)
rust_tokio_mailbox_throughput   median=     1.088 ms  p95=     1.179 ms  p99=     1.188 ms  (10k msgs/iter)
cpp_spsc_mailbox_throughput     median=     0.157 ms  p95=     0.250 ms  p99=     0.377 ms  (10k msgs/iter)
```

With 1 000 msgs/iter: **median ≈ 4.5M msgs/sec single-threaded** —
about 49% of bare Rust tokio mpsc on the same host (per-msg
normalised: 0.221 ms / 1k vs 1.088 ms / 10k = 221 ns vs 109 ns
per message). The slab admission step adds ~112 ns per message
over a bare mpsc — a ~16% improvement over the v0.33 refresh's
~134 ns slab tax, which we attribute to release-pgo's fat LTO +
PGO inlining the admit fast-path more aggressively. The C++ SPSC
lock-free ring is ~14× faster per message than Mighty's mailbox —
that's the expected shape (no allocator pressure at all,
single-producer-single-consumer assumption).

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
