# Runtime v0.3 — interpretation notes

Working notes from the runtime swarm slice. Captures judgement calls
made under the autonomous-build mandate, deferred items, and the
post-v0.3 follow-on list.

## Closed amendments

- **A70** — Cooperative mid-turn cancellation (closes A41).
- **A71** — OTLP wire-format telemetry (closes A38).
- **A72** — Slab-pool mailbox frames (closes A40).
- **A73** — Batched per-turn deadline scheduler.

## Files added

- `crates/mty-runtime/src/cancel.rs` — `CancellationToken` + `CancelReason`.
- `crates/mty-runtime/src/slab_pool.rs` — `SlabPool` + `PooledFrame`.
- `crates/mty-runtime/src/delay_timers.rs` — `DelayScheduler`.
- `crates/mty-runtime/src/otlp.rs` — `OtlpHandle` (feature-gated).
- `crates/mty-runtime/tests/cancellation_mid_turn.rs`.
- `crates/mty-runtime/tests/mailbox_slab_pool.rs`.
- `crates/mty-runtime/tests/delay_queue_timers.rs`.
- `crates/mty-runtime/tests/otlp_export.rs`.
- `tests/conformance/budget_violation/{05_wall_cancels_mid_turn,06_cpu_step_budget}/`.
- `tests/conformance/mailbox_ordering/{04_slab_reuse_fifo,05_backpressure_block}/`.
- `docs/internals/telemetry-otlp.md`.

## Files modified

- `crates/mty-runtime/Cargo.toml` — opentelemetry-* + tokio-util deps, `otlp` feature.
- `crates/mty-runtime/src/lib.rs` — module list + re-exports.
- `crates/mty-runtime/src/mailbox.rs` — slab-backed admit path.
- `crates/mty-runtime/src/agent.rs` — `run_one_turn_async` + shared reply.
- `crates/mty-runtime/src/telemetry.rs` — `Otlp` sink variant.
- `crates/mty-runtime/src/runtime.rs` — `shutdown_token`, cancellation-aware agent loop.
- `docs/internals/runtime.md` — A70 architecture section.
- `docs/internals/mailboxes.md` — A72 slab section.
- `docs/spec/v0.1-amendments.md` — A70..A73 appended.

## Interpretation calls

1. **No edits to `mty-sir`.** The scope rule forbids it, so
   cooperative cancellation is implemented by wrapping each
   synchronous `run_handler_isolated` call in `spawn_blocking`,
   racing the join handle against a `CancellationToken`, and
   *detaching* the blocking thread on cancel. The worst-case wall
   time of the detached thread is bounded by the MtyIR step budget
   (1 M steps). A future v0.4 MtyIR change can replace this with
   real interpreter-side cancellation polling.

2. **Reply oneshot via shared slot.** To guarantee exactly-once
   notification of the `ask` caller even when the blocking shim and
   the cancel arm race, the frame's `reply` sender is moved into a
   `Arc<Mutex<Option<...>>>` before scheduling. Both sides take it;
   first wins. No double-send, no hang.

3. **Slab pool is per-mailbox, not global.** Spec §25.3 calls for
   "pre-allocated fixed-size MessageFrame slots reused via a
   free-list"; a single global pool would have unbounded cross-agent
   contention. Per-mailbox pools match the mailbox capacity 1:1 by
   default and keep all hot-path code lock-local.

4. **Inline payload encodes only metadata.** Slab slots hold a small
   descriptor (proto-message-name prefix + arg-size hint) — not a
   wire-format serialisation of the `Value` args. This keeps the
   per-`send` cost predictable (no value serialisation) while still
   exercising the inline-vs-overflow split for realistic memory
   pressure.

5. **OTLP is feature-gated, default-on.** `cargo build -p mty-runtime`
   pulls the exporter by default so `STARDUST_OTLP_ENDPOINT` Just
   Works. `--no-default-features` strips it for minimum-binary
   builds. Init failure (collector unreachable) silently falls
   through to the JSON sink and prints one diagnostic line — never
   breaks runtime construction.

6. **DelayScheduler ships but isn't wired as the default timer.**
   The per-turn cancellation path uses a single `tokio::spawn(sleep
   + cancel)` per turn — fine for one in-flight turn per agent.
   `DelayScheduler` is the batched building block; supervisors will
   adopt it in v0.4 when they track many children's per-turn budgets.

7. **Mailbox API is byte-for-byte compatible.** `MessageFrame`,
   `Mailbox::send`, `Mailbox::try_send`, `Mailbox::take_receiver`,
   `Mailbox::recv` all behave identically to slice 7 from outside the
   crate. The new `_slab: Option<PooledFrame>` field on
   `MessageFrame` is `pub(crate)`, so downstream code is unaffected.

8. **Conformance new cases.** The harness in
   `crates/mty-driver/tests/conformance_full.rs` drives cases
   through the MtyIR interp directly, *not* through `mty-runtime`.
   The new conformance cases (`05_wall_cancels_mid_turn`,
   `06_cpu_step_budget`, `04_slab_reuse_fifo`, `05_backpressure_block`)
   are shape-only at the harness layer: they compile + run as
   trivial programs and produce expected stdout, while the actual
   v0.3 invariants live at the runtime-test layer
   (`crates/mty-runtime/tests/`). This avoids modifying the
   harness (out of scope for this swarm).

## Open follow-on (post v0.3)

- **MtyIR-side cancellation polling.** Have `run_handler_isolated`
  check a passed-in cancel token every N steps so the runtime can
  truly interrupt mid-turn instead of detaching. Requires
  mty-sir changes.

- **CpuBudget reason wiring.** The reason variant exists but no
  current code path fires it. A future per-agent CPU-time aggregator
  would call `cancel(CancelReason::CpuBudget)` when the rolling sum
  exceeds the agent's `cpu` budget.

- **HTTP/protobuf transport selector.** `OtlpHandle::try_init`
  hardcodes gRPC. Add a second env var (e.g. `STARDUST_OTLP_PROTO`)
  selecting `tonic` vs `http-proto` from the user.

- **OTel resource attribute env-var overrides.** Honour
  `OTEL_RESOURCE_ATTRIBUTES` and `OTEL_SERVICE_NAME` per spec.

- **DelayScheduler as default per-turn timer.** When many agents
  run concurrently (>10), the per-turn `tokio::spawn(sleep + cancel)`
  overhead becomes measurable. Migrate to the batched scheduler
  once a supervisor v0.4 lands that already uses it.

- **Slab pool benchmark.** A criterion-driven `send`-latency
  benchmark would land here once the bench harness is set up (no
  bench harness exists for mty-runtime today).

- **`Mailbox::with_pool` plumbed through `RuntimeBuilder`.** Today
  the runtime always creates a fresh slab per mailbox; tests can
  share a pool but production callers can't override layout. A
  builder hook is straightforward.

## Acceptance status

- Build: `cargo build -p mty-runtime` passes (clean).
- Tests: `cargo test -p mty-runtime` — see latest run for live
  count; new tests added: cancellation_mid_turn (3),
  mailbox_slab_pool (8), delay_queue_timers (2), otlp_export (3) +
  inline `#[cfg(test)] mod tests` in cancel.rs (4), slab_pool.rs (4),
  delay_timers.rs (2).
- Clippy / fmt: clean for touched files (see CI).
- Examples: 20-example smoke still passes (cancellation upgrade
  preserves the slice-7 semantics for handlers that complete within
  budget — the per-turn timer simply never fires).
