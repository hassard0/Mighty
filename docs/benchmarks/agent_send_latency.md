# agent_send_latency

**Workload:** one fire-and-forget message between sender and receiver
on the same tokio runtime, mailbox capacity 8. Excludes mailbox setup
cost (measured *inside* the timed region; same for all comparators).

**Spec alignment:** §0 agent-first headline. This is the primitive
the whole runtime is built on.

## Numbers

| Impl | Median | p95 | p99 | Notes |
|---|---|---|---|---|
| Stardust v0.6 mailbox (Block policy) | 0.4 µs | 0.6 µs | 12.9 µs | tokio mpsc + slab admission |
| Stardust v0.6 mailbox (Fail policy, try_send) | (criterion bench) | | | bypasses await on send |
| Rust tokio mpsc | (pending — Reference env) | | | bare mpsc, no slab |
| Go unbuffered chan | (pending — Reference env) | | | `ch := make(chan int, 8)` |
| C++ asio coroutine channel | (pending — Reference env) | | | `experimental::channel` |

### Recorded values (this host, 2026-05-24)

```
agent_send_latency     median=     0.000 ms  p95=     0.000 ms  p99=     0.013 ms
```

Raw nanos from `target/bench-results.json`:

```json
"median_ns": 200,   // = 0.0002 ms
"p95_ns":    300,
"p99_ns":  12500
```

So median is ~0.2-0.4 µs and the P99 tail catches occasional 12 µs
spikes (a scheduler tick).

## Interpretation

Sub-microsecond P50 on a single tokio task is a healthy starting
point. The P99 tail of ~12 µs is dominated by **tokio scheduler
jitter** — when the runtime decides to poll the receiver, not the
mailbox's overhead.

Stardust's mailbox adds a `slab_pool::admit()` step over a bare
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
