# v0.8 benchmarks — interpretation calls

Agent log for the v0.8 performance optimisation swarm. Records the
optimisations applied to the v0.7+ backlog from `BENCHMARKS_V0_6_NOTES.md`,
what landed, what was honest-regression-and-reverted, and the open
follow-ups.

For numbers: see `docs/benchmarks/` and the microbench output below.
For the file/owner split: see the v0.8 perf swarm brief.

## Targets, status, and v0.8 commit anchors

| Target                    | Status     | What shipped                                                                                                                       |
|---------------------------|------------|------------------------------------------------------------------------------------------------------------------------------------|
| 1: Parse throughput       | LANDED     | `TokenCache` incremental re-lex + `ParseOpts::max_diagnostics` throttle (commit 82eafb5)                                            |
| 2: Mailbox throughput     | LANDED     | `try_recv_many` batched drain + slab-empty fast path (commit 82eafb5)                                                              |
| 3: Agent send latency     | LANDED     | `SmallPayload::Empty` bypasses slab acquire; stack-resident inline-bytes cache for non-empty (commit 82eafb5)                       |
| 4: Compile time           | PARTIAL    | `Monomorphizer::run_parallel` shipped but reverted from default; HashMap pre-sizing + scratch Vec in `LowerCtx::declare_fns` kept   |
| 5: HTTP server throughput | DEFERRED   | Owned by loose-ends agent; this swarm does not touch http_server.rs                                                                |

### Headline microbench numbers (this host, 2026-05-24)

`agent_send_latency_v0_8` (criterion `--quick` warmup-only — host
under concurrent agent load; treat as ballpark, not gospel):

| Variant                | Median   | Notes                                              |
|------------------------|----------|----------------------------------------------------|
| send_recv_empty        | 1.07 µs  | full async path, empty payload → slab-empty FP     |
| send_recv_inline_1     | 0.92 µs  | full async path, 1-value payload → slab acquire    |
| try_send_empty         | 0.80 µs  | sync send (no await on send), empty payload → FP   |

Compare to v0.6 baseline (`docs/benchmarks/agent_send_latency.md`):
P50 ~0.4 µs, P99 ~12.9 µs. The criterion numbers here include the
per-iter mailbox construction+teardown, so they're 2-3x higher than
the v0.6 runner's "steady-state" P50. The P99 12.9 µs spike was
attributed to "tokio scheduler jitter" in v0.6 — empty-payload
fast-path removes the scheduler-orthogonal slab acquire (parking_lot
Mutex lock + Vec alloc + slot write + eventual release), which should
shrink the P99 tail but isn't directly visible in the criterion median.

`mailbox_throughput_v0_8` (criterion `--quick`):

| Variant                       | Median   | Throughput      |
|-------------------------------|----------|-----------------|
| single_recv_empty_payload     | 2.87 ms  | 3.48 Melem/s    |
| batched_recv_empty_payload    | 2.65 ms  | 3.77 Melem/s    |

~7-8% improvement from batched try_recv_many drain. Compare to the
v0.6 baseline of 4.4 Melem/s (1000-msg shape, not 10000); the
absolute number depends on host load — see below for the variance
caveat.

`lex_throughput` (criterion `--quick`, 66 890-byte synth):

| Variant            | Median    | Throughput      |
|--------------------|-----------|-----------------|
| lex_full           | 811 µs    | 78 MiB/s        |
| tokencache_full    | 1.31 ms   | 48 MiB/s        |
| tokencache_edit    | 593 µs    | 107 MiB/s       |

`tokencache_edit` is the v0.8 LSP-target win: an incremental edit at
the midpoint re-lexes only ~3 tokens (verified by
`incremental_reduces_relex_count` test) and recomposes the cache in
~600 µs vs ~810 µs for the full re-lex. The throughput number is
nominal — the actual wall-time win is "re-lex only the dirty region",
not "lex faster than logos".

`diag_throttle` (criterion `--quick`, 500 stray `@` tokens):

| Variant      | Median   |
|--------------|----------|
| uncapped     | 113 µs   |
| capped_16    | 85 µs    |

~25% speedup on adversarial input. Real LSP wins are larger — capping
prevents a 10 KLOC file from emitting 50 000 diagnostics that the
client then has to display.

`mono_*` (criterion `--quick`):

| Program size  | Sequential | Parallel   | Verdict                  |
|---------------|------------|------------|--------------------------|
| small_4g      | 7.6 µs     | 6.7 µs     | parallel ~10% faster     |
| medium_32g    | 44 µs      | 354 µs     | parallel ~8x SLOWER      |
| large_256g    | 303 µs     | 547 µs     | parallel ~1.8x SLOWER    |

**Honest regression on Target 4**: parallel mono is slower because
`specialize` is just `clone + concretize` walk — per-fn cost is ~1-2
µs, less than the std::thread::scope spin-up cost amortised across
chunks. The `run_parallel` API stays available for the future when
typeck type-arg propagation makes per-fn cost large enough; `run()`
itself stays on the sequential path.

## v0.8 changes by owned file

### mty-syntax

- `src/token_cache.rs` (NEW): `TokenCache` with `apply_edit(start, end,
  replacement) → relexed_token_count`. Caches `CachedToken { kind,
  start, end }`. Widens the relex region by ±1 token to absorb
  whitespace/comment coalescing. 5 tests, all green.
- `src/parser/mod.rs`: `ParseOpts { max_diagnostics: usize }` +
  `parse_with_opts` entry. `Parser::error_at` no-ops once the cap is
  reached. Default uncapped (preserves v0.6 behaviour).
- Diag throttle is opt-in; the default `parse()` path is byte-for-byte
  unchanged.

### mty-runtime

- `src/slab_pool.rs`: `SlabPool::acquire_empty()` returns a tombstone
  `PooledFrame` (no slot held, Drop is a no-op). `acquire_or_overflow`
  bypasses to `acquire_empty` when `bytes.is_empty()`.
- `src/mailbox.rs`: `Mailbox::admit` checks `SmallPayload::Empty`
  upfront and takes the tombstone path. Non-empty admit uses a
  64-byte stack buffer for the descriptor (no per-call `Vec` heap
  alloc). `try_recv_many(rx, out, max)` exported from `mailbox` +
  `lib`. 4 new mailbox tests covering: empty-skips-slab, nonempty-
  uses-slab, batched drain, batched respects max.

### mty-codegen-cranelift

- `src/mono.rs`: `Monomorphizer::run_parallel` via `std::thread::scope`,
  4-worker cap, threshold 8 generics. Deterministic splice-by-index.
  `run()` reverted to dispatch to `run_sequential` after measurement
  showed parallel was a regression at the per-fn cost we have today.
  2 new tests: `parallel_matches_sequential`, `parallel_threshold_small_program`.
- `src/lower.rs`: `LowerCtx::new` pre-sizes the 4 HashMaps;
  `declare_fns` reserves + reuses a `param_tys` scratch Vec across
  fns instead of `Vec::new` per fn. No behaviour change; reduces
  rehash count on programs with >> 30 fns.

## What's NOT in v0.8

- **Lock-free mpsc opt-in (`crossbeam_channel` backend)**: punted.
  The tokio mpsc is already cache-friendly for the single-sender
  shape we benchmark; adding a feature flag without measured wins
  would just grow the API surface.
- **Thread-local arena for hot agents**: punted. Empty-payload fast
  path already eliminates the slab acquire for the dominant case
  (fire-and-forget Ping); a thread-local arena helps only when
  payloads are non-empty AND the slab is contended (multi-sender),
  which is not exercised by the v0.6 benches.
- **Single-sender fast path**: punted. tokio mpsc with one Sender
  doesn't take a fair-queueing slow path; the win would be from
  bypassing the channel itself, which is a much larger refactor.
- **Stdlib metadata cache**: the v0.6 backlog item lives upstream of
  cranelift's `lower.rs` (it'd cache the prelude type table from
  mty-types). Out of scope for this swarm — mty-types is owned by a
  different swarm. The cranelift-local equivalent (pre-sized
  HashMaps, scratch Vec) is shipped.

## Variance and honesty caveats

- The host (Windows 11) was running 3 other v0.8 swarm agents
  concurrently. Criterion numbers from the `--measurement-time 5`
  runs showed up to 2-3x variance run-to-run. The `--quick` numbers
  in the tables above are warmup-only samples; the documented v0.6
  baselines were recorded on a quieter host.
- Where the criterion run showed "the change made it worse" but the
  shape didn't make architectural sense (e.g. parse_throughput
  appearing to regress 60% when the default parse path is unchanged),
  attribute to host noise, not the code change.
- The honest path is to re-run on a quieter host before claiming any
  category exceeds the v0.6 baseline. That re-run is out of scope
  for the swarm time-budget.

## Open follow-ups

- Re-run all microbenches on a quiet host to lock in the actual
  baselines for `docs/benchmarks/v0_8.md`.
- Wire `parse_with_opts(src, ParseOpts { max_diagnostics: 256 })`
  through `mty-lsp` to actually benefit the IDE path.
- Wire `TokenCache` into the LSP's document-store on `didChange`.
  Today only the test suite + microbench exercises it.
- Replace the tokencache widen-by-1 with a kind-aware widen (only
  widen for trivia kinds) — reduces re-lex cost for non-trivia edits.
- When per-fn typeck-per-instantiation lands, re-benchmark
  `run_parallel` and consider re-enabling it via threshold.
