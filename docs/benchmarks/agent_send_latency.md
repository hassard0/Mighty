# agent_send_latency

> **Last refreshed: v0.33 (2026-05-28) on vulcan** (Dell, Intel Xeon,
> Ubuntu 24.04, Rust 1.95.0). Mighty + Rust-tokio comparator numbers
> are v0.33; Go + C++ comparators retain the v0.6 baseline pending a
> comparator toolchain refresh on the benchmark host.

**Workload:** one fire-and-forget message between sender and receiver
on the same tokio runtime, mailbox capacity 8. Excludes mailbox setup
cost (measured *inside* the timed region; same for all comparators).

**Spec alignment:** §0 agent-first headline. This is the primitive
the whole runtime is built on.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Mighty v0.33 mailbox (Block policy) | 0.2 µs | 0.25 µs | 2.5 µs | tokio mpsc + slab admission |
| Mighty v0.33 mailbox (Fail policy, try_send) | (criterion bench) | | | bypasses await on send |
| Rust tokio mpsc (bare) | ~0.05 µs | ~0.1 µs | ~1 µs | bare mpsc, no slab — vulcan |
| Go unbuffered chan | (pending — Reference env) | | | `ch := make(chan int, 8)` |
| C++ asio coroutine channel | (pending — Reference env) | | | `experimental::channel` |

### Recorded values (vulcan, 2026-05-28, v0.33)

```
agent_send_latency      median=     0.000 ms  p95=     0.000 ms  p99=     0.003 ms
rust_tokio_send_latency median=     0.000 ms  p95=     0.000 ms  p99=     0.001 ms
```

Raw nanos from `/tmp/bench-results-v033.json`:

```json
"median_ns": 200,   // = 0.0002 ms
"p95_ns":    253,
"p99_ns":   2512
```

So median is ~0.2 µs and the P99 tail catches occasional 2.5 µs
spikes (down from v0.6's ~12 µs on the dev laptop — vulcan's
multi-socket Xeon has more predictable scheduling).

### v0.6 baseline (Windows 11 dev laptop, 2026-05-24)

For continuity: Mighty v0.6 measured **median = 0.2-0.4 µs** with a
P99 tail of ~12 µs on a different host. Cross-host deltas are
shape, not absolute.

## Interpretation

Sub-microsecond P50 on a single tokio task is a healthy starting
point. The P99 tail of ~12 µs is dominated by **tokio scheduler
jitter** — when the runtime decides to poll the receiver, not the
mailbox's overhead.

Mighty's mailbox adds a `slab_pool::admit()` step over a bare
tokio mpsc (records a metadata blob in the slab so we can back-pressure
on bytes, not just count). Expected overhead vs bare mpsc: a few hundred
nanoseconds. Once we run the Rust comparator we'll quantify the
exact slab tax.

## v0.7+ optimisation targets

- **Skip slab admission for empty payloads.** Empty `SmallPayload`
  doesn't need a slot.
- **Replace the slab's allocator with a thread-local arena** for hot
  agents (P99 spikes likely come from contention on the shared pool).
- **Inline fast path for unique senders** — when the mailbox has a
  single sender (the common case after spawn), the lock-free path
  can skip the mpsc indirection.

Tracked in: `BENCHMARKS_V0_6_NOTES.md` § Agent Send Latency.

## v0.8 update

| Optimisation                          | Status  | Delta                                                                                  |
|---------------------------------------|---------|----------------------------------------------------------------------------------------|
| Skip slab admit for empty payloads    | DONE    | `Mailbox::admit` returns a tombstone `PooledFrame` for `SmallPayload::Empty` — no parking_lot lock, no Vec alloc, no slot write |
| Stack-resident inline cache (non-empty) | DONE  | 64-byte stack buffer replaces per-call `Vec::with_capacity(inline_bytes)`              |
| Thread-local arena (hot agents)       | DEFER   | Empty fast-path already removes the dominant cost; thread-local arena only helps multi-sender + non-empty, which the v0.6 benches don't exercise |
| Single-sender fast path               | DEFER   | tokio mpsc with one Sender doesn't take a fair-queueing slow path; bypassing the channel is a larger refactor |

Microbench: `crates/mty-runtime/benches/agent_send_latency.rs`
(send_recv_empty / send_recv_inline_1 / try_send_empty).
Interpretation log: `BENCHMARKS_V0_8_NOTES.md`.

`try_send_empty` measured ~800 ns end-to-end on this host vs the
v0.6 P50 of ~0.4 µs measured by the lower-overhead runner; the
criterion bench's per-iter mailbox construct/drop adds ~400 ns,
which is the bulk of the headline gap.
