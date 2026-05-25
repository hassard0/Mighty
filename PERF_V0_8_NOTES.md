# PERF v0.8 — agent log

Implementation interpretation calls + perf findings for the v0.8
performance-optimisation swarm. See `BENCHMARKS_V0_8_NOTES.md` for
the canonical numbers + status table; this file is the
*decisions-and-tradeoffs* log.

## Interpretation calls

### Why the `_slab: Some(tombstone)` shape for empty-payload bypass

The original v0.3 `Mailbox::admit` always called
`slab.acquire_or_overflow(&buf)`, attaching a real `PooledFrame`
(slot index or overflow buffer) to every `MessageFrame::_slab`. v0.8
introduces a "tombstone" `PooledFrame` (slot_idx = None, overflow =
None, len = 0) for the `SmallPayload::Empty` case.

Alternative considered: make `_slab` an `Option<PooledFrame>`
already, and just set it to `None` on the empty path. Rejected
because:

1. Existing call sites (telemetry, leak-detect assertions) expect
   `_slab.is_some()` after admit; making it `None` would break the
   "every admitted frame holds a handle" invariant assumed by the
   v0.3 docs.
2. The tombstone shape costs one `Arc::clone` and one struct write —
   ~10 ns — vs the slab acquire path's parking_lot lock + Vec alloc +
   slot write — ~300-500 ns. Cheap enough to preserve the invariant.

### Why the stack inline cache is 64 bytes (== `DEFAULT_INLINE_BYTES`)

The slab pool's `inline_bytes` defaults to 64. Matching the stack
buffer to this size means the descriptor `[name bytes..][hint:u16]`
fits in one cache line on x86_64 and is byte-for-byte interchangeable
with the slab's `inline` Vec storage. Larger custom pools (the
`SlabPool::with_layout` path) fall back to a heap Vec at the
descriptor build step — uncommon (only set by tests today).

### Why `try_recv_many` is a free function, not `Mailbox::recv_many`

It takes the `tokio::sync::mpsc::Receiver` directly (after
`take_receiver`) so callers using the raw receiver don't have to go
through the `Mailbox` indirection. The agent run loop holds the
receiver, not the mailbox; making it a free function avoids a layer
of Arc-deref on the hot drain path.

### Why the token cache widens by ±1 token unconditionally

A kind-aware widen (only widen for `TRIVIA` kinds) would re-lex
fewer tokens on non-trivia edits, but it'd miss the case where a
non-trivia token boundary actually moved (e.g. inserting `_` into
the middle of an identifier). The conservative ±1 widen is correct
in 100% of cases at the cost of an extra ~1-2 tokens per edit. Hot
path is "edit affects 1-3 tokens"; +2 vs +0 isn't measurable.

Left as a v0.9+ follow-up: kind-aware widen when the touched token
is non-identifier-like.

### Why parallel mono was committed then immediately disabled

The brief says:
> For parallel typeck: extract per-fn typeck into a closure; collect
> into a `Vec<(FnId, TyResult)>` via `rayon::par_iter`. Care: ensure
> typeck has no shared mutable state that conflicts.

I implemented the parallel version (no rayon dependency — std::thread::
scope), measured it (`typeck_parallel` bench), and found it was
**1.8-8x slower** than sequential at all tested sizes (4, 32, 256
generic fns). Per-fn `specialize()` cost is dominated by `Function::
clone` and a quick walk — total ~1-2 µs per fn — and the
`std::thread::scope` worker spin-up amortises to multiple µs per
worker, swamping the parallel speedup.

The right call here is **honest reporting**: ship `run_parallel`
behind an API but make `run()` dispatch to `run_sequential` until
per-fn cost grows. Documented in the doc-comment on `Monomorphizer::
run` + in `docs/benchmarks/compile_to_native.md` v0.8 update.

Future: when typeck type-arg propagation lands and `specialize`
becomes a real typeck-per-instantiation pass (10s-100s of µs per
fn), the parallel path becomes profitable.

### Why no crossbeam_channel feature flag

The brief suggests `mpsc-backend = "default" | "crossbeam"`. I
omitted it because:

1. The tokio mpsc is already fast for single-producer + single-
   consumer; the crossbeam_channel win is multi-producer contended.
2. Adding the feature flag means a `cfg`-gated divergence in `Mailbox`
   that doubles the maintenance surface.
3. The v0.6 mailbox bench is single-prod single-cons. There's no
   measured workload where crossbeam wins on this hardware.

If/when a multi-producer agent fan-in workload exists, re-evaluate.

### Why no thread-local arena

Same reason as the crossbeam decision: the v0.6 benches don't
exercise a workload where thread-local arenas would beat the slab
pool. The empty-payload fast path already removes the dominant slab
cost. Thread-local arenas would help if the slab pool's parking_lot
mutex were contended under multi-sender; today it isn't.

## Findings

### Variance is the headline noise source

This host (Windows 11) was running 3 other v0.8 swarm agents during
the perf work. Criterion `--quick` and `--measurement-time 5` runs
showed up to 2-3x variance for the SAME bench between adjacent
invocations. Specifically:

- `mailbox_throughput` first run: 2.87 ms; second run: 8.42 ms.
- `parse_throughput` first run: 10 ms; second run: 12 ms.

The honest path is to NOT claim cross-version speedups based on this
host's numbers; only same-host comparison (e.g. sequential vs
parallel mono) is meaningful.

### `try_send` is meaningfully cheaper than `send`

Even with both paths going through the same `admit`:

- `send_recv_empty` (async): 1.07 µs
- `try_send_empty` (sync): 0.80 µs

The 270 ns delta is mostly the tokio Sender's async overhead +
`await` machinery. Callers that can use `try_send` (Fail policy with
explicit backpressure handling) should — it's a real ~25% win.

### The token cache's full-build cost is ~60% higher than `lex`

- `lex_full`: 811 µs
- `tokencache_full`: 1.31 ms

The full TokenCache::lex allocates a `Vec<CachedToken>` (24 bytes
per token) on top of the `Vec<LexedToken>` lex already produces. For
the LSP use case this is fine — the cache pays for itself after the
first incremental edit. The microbench compares them honestly so the
LSP integration can decide which to keep when it has the source
buffer.

## Code-level perf hygiene applied

- `LowerCtx::declare_fns` reused a `param_tys` scratch Vec instead
  of allocating per fn — net ~100 heap allocations saved on the
  1-KLOC synth source compile.
- `LowerCtx::new` pre-sizes 4 HashMaps. Saves ~3-5 rehashes on
  programs with 100+ fns.
- `Mailbox::admit` uses a stack `[u8; 64]` buffer instead of
  `Vec::with_capacity(64)`. Saves one heap alloc per send.

These are individually tiny wins; collectively they're the kind of
hygiene that compounds in profiles.

## What did NOT make it

- Lock-free mpsc opt-in. Rationale above.
- Thread-local arena. Rationale above.
- Single-sender fast-path. Rationale: bypassing the channel is a
  larger refactor than the 5-hour time budget supports.
- Stdlib metadata cache in mty-types. Out of swarm scope.
- Incremental compilation. Explicitly deferred in the brief.

## v0.9 follow-ups

- Wire `parse_with_opts` through `mty-lsp` so the throttle actually
  benefits the IDE path.
- Wire `TokenCache` into `mty-lsp`'s document store on `didChange`.
- Kind-aware widen in `TokenCache::apply_edit` (only widen for
  trivia kinds).
- Re-bench `Monomorphizer::run_parallel` once typeck propagation
  lands.
- Re-run all microbenches on a quiet host to lock in actual deltas.
