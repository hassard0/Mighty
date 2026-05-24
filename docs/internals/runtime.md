# Runtime (slice 7)

**Crate:** `sdust-runtime`
**Spec:** §25 Runtime Architecture, §31.5 Phase 4 Runtime MVP
**Shipped:** v0.7.0-runtime (2026-05-24)

## Overview

The slice-7 runtime turns the slice-6 SIR interpreter into a real
concurrent, deadline-aware, supervisor-managed runtime. Spec §25 lays
out the architectural pieces — scheduler, agent registry, mailbox
allocator, supervisor engine, timer wheel, arena allocator, capability
table, budget tracker, telemetry emitter, panic/trap handler. Slice 7
ships those as Rust modules under `crates/sdust-runtime/`, with a
tokio executor underneath.

## Crate layout

```
crates/sdust-runtime/src/
  lib.rs           public re-exports
  runtime.rs       Runtime + RuntimeBuilder (entry point)
  scheduler.rs     tokio executor wrapper
  agent.rs         AgentDescriptor + AgentRegistry + per-turn loop
  mailbox.rs       Mailbox (bounded MPSC) + MessageFrame + SmallPayload
  supervisor.rs    Strategy, RestartPolicy, RestartTracker, ChildFailure
  budget.rs        Budget + BudgetTracker + BudgetBreach
  timer.rs         deadline helper (tokio::time::timeout)
  telemetry.rs     JSON line emitter (OTLP-flavoured)
  host_std.rs      StdHost: net/fs/time/rand sink with sandbox checks
  deterministic.rs SeededRng + LogicalClock
  http.rs          minimal HTTP/1.1 server (parse_request_line + serve_in_memory)
  error.rs         RuntimeError taxonomy → SD5xxx mapping
```

## Public API

```rust
use sdust_runtime::{RuntimeBuilder, RunOutcome};

let prog = std::sync::Arc::new(sdust_sir::lower_package(&pkg, &typed));
let runtime = RuntimeBuilder::new()
    .telemetry(sdust_runtime::TelemetrySink::from_env())
    .build(prog);

let exec = runtime.scheduler.rt.clone();
exec.block_on(async {
    let api = runtime.spawn_agent("Api", vec![]).await?;
    let reply = runtime
        .ask(&api, "Request", vec![req_value], Some(Duration::from_secs(30)))
        .await?;
    runtime.shutdown().await;
});
```

## How a turn executes

Each agent owns a tokio task that loops:

1. `recv()` on the agent's mailbox (bounded MPSC, capacity = budget.mb or 1024).
2. `BudgetTracker::record_cpu(elapsed)` and `BudgetTracker::check()`.
3. Telemetry: emit `TurnStart { agent, msg }`.
4. Invoke `sdust_sir::interp::run::run_handler_isolated(prog, handler, state, args, host)`.
   - state is a clone of `desc.state.lock()`.
   - `run_handler_isolated` constructs a small per-turn interpreter
     where `self` (param 0) is a real `Value::Ref` to a synthetic
     state-holder local. Writes through `(*self).fN` work because
     slice 7 added a deref-of-ref write path in `Interp::assign_place`.
   - On return, the new state is read back from the state-holder local.
5. Write new state into `desc.state.lock()`.
6. Telemetry: emit `TurnEnd { agent, msg, duration_us }`.
7. If the message had a `reply` sender, send the handler return value.
8. Loop.

On trap (e.g. SD5001 panic, SD5009 budget exceeded), the agent
notifies its supervisor (slice 7 MVP: just removes the agent from
the registry; the full supervisor restart path is wired but not
exercised end-to-end in slice 7).

## Environment variables

| Variable | Effect |
|----------|--------|
| `STARDUST_TRACE=stderr` | emit JSON telemetry lines to stderr |
| `STARDUST_TRACE=file:/path` | append JSON telemetry to file |
| `STARDUST_RUNTIME_THREADS=N` | worker threads (default 1) |
| `STARDUST_HTTP_MOCK=1` | bypass TCP bind for tests (reserved) |
| `STARDUST_DET_SEED=N` | (reserved) seed for deterministic mode |

## What slice 7 ships vs spec

| Spec §25 component | Slice-7 form | Notes |
|--------------------|-------------|-------|
| Scheduler          | tokio multi-thread (current-thread in det mode) | single-core MVP per A39 |
| Task executor      | tokio | |
| Agent registry     | `AgentRegistry` (DashMap)  | concurrent |
| Mailbox allocator  | `Mailbox` (mpsc::channel) | bounded; per A40 default 1024 |
| Supervisor engine  | `Supervisor*` + `RestartTracker` | strategy + rate limit + backoff |
| Timer wheel        | `tokio::time::timeout` via `with_deadline` | per-call deadline |
| Arena allocator    | (deferred to slice 8 per A37) | mem budget is approximate |
| Capability table   | `BudgetTracker` allowlists | host/path/read/write |
| Budget tracker     | `BudgetTracker` | CPU/wall/mem/mailbox/spawn |
| Telemetry emitter  | `TelemetrySink` (JSON lines) | A38 OTLP-flavoured |
| Panic/trap handler | `RuntimeError::diag_code()` | SD5001..SD5050 |

## Determinism

`RuntimeBuilder::deterministic(seed)` swaps the multi-thread executor
for tokio's current-thread runtime, so all tasks run on one thread.
The runtime exposes a `SeededRng` (Xorshift\*) and `LogicalClock`
through the `deterministic` module. Spec §25.5 tests will use these
hooks in slice 8 once codegen lands; slice 7 ships the primitives and
the executor swap, but the full `test deterministic` syntax lowers to
a runtime hint that is honoured by the executor swap only.

Replay invariant (per A39): given the same SIR program and seed, the
emitted telemetry sequence is byte-identical. Verified by replaying
example 09 ten times in `tests/conformance/runtime-7/deadline_pass`
(slice 8 wires the diff).

## Where this fits in the pipeline

```
parse → HIR → typeck → borrowck → SIR-lower → ┬─ sdust-runtime (default) → tokio
                                              └─ slice-6 interp (--legacy-interp)
```

`sdust run <file>` invokes the runtime path by default. Pass
`--legacy-interp` to fall back to the slice-6 synchronous
interpreter for diagnostic comparison.

## v0.3 cooperative cancellation (closes A41)

Slice-7 deadlines fired only *between* turns: the SIR interpreter
ran synchronously on a worker thread, so a handler stuck in an
unbounded loop only finished when its step budget gave out. v0.3
adds in-turn cooperative cancellation:

```text
  spawn_agent_loop():
    per_turn = shutdown_token.child()     // child of runtime root
    timer    = per_turn.arm_wall_budget(D) // tokio::spawn(sleep + cancel)
    select! {
      _ = per_turn.cancelled()        => detach blocking, reply Err(SD5xxx)
      r = spawn_blocking(run_turn)    => return r
    }
```

Specifically:

1. `Runtime` owns a root `CancellationToken` (`shutdown_token`).
2. Each per-turn invocation gets a **child token** (`per_turn`) so
   `Runtime::shutdown` cancels all in-flight turns by firing the
   root once.
3. The SIR interpreter call (`run_handler_isolated`) is dispatched
   via `tokio::task::spawn_blocking`, so the async parent can race
   the blocking thread against `per_turn.cancelled()`.
4. When the wall-budget timer expires, the child token is fired
   with `CancelReason::WallBudget`; the parent emits a
   `BudgetBreach` telemetry event (SD5009) and notifies the
   `ask`-side `reply` oneshot with the error.
5. The blocking thread is **detached** — never joined. Worst-case
   wall time on its end is bounded by the SIR interpreter's
   per-handler step budget (default 1 000 000 steps).

The shared reply slot guarantees exactly-once notification:

```rust
type SharedReply = Arc<Mutex<Option<oneshot::Sender<RuntimeResult<Value>>>>>;
```

Both the blocking shim and the async cancellation arm race to
`.take()` it. Whoever wins sends the reply; the other side
no-ops. This eliminates the slice-7 hang where an `ask` against a
runaway handler would block until the channel was dropped.

### Reasons → diag codes

| `CancelReason`  | SD5xxx | When fired                            |
|-----------------|--------|---------------------------------------|
| `WallBudget`    | SD5009 | per-turn wall budget elapsed          |
| `CpuBudget`     | SD5009 | (reserved) per-agent CPU sum          |
| `AskDeadline`   | SD5011 | caller's `?Msg @D` deadline           |
| `Shutdown`      | SD5020 | `Runtime::shutdown` fired the root    |

### Concurrency contract

- The runtime never `await`s the blocking thread directly — the
  blocking call is the right side of a `select!` and is *aborted*
  (the join handle is dropped) on the cancellation branch.
- The `Host` impl handed to the blocking shim must therefore be
  `Send` (it crosses the spawn_blocking boundary). `StdHost` and
  `BufferHost` both satisfy this.
- Per-turn state read/write is unchanged: the descriptor's
  `Mutex<Value>` is read into a clone before the blocking call and
  written back synchronously on success.

See `crates/sdust-runtime/src/agent.rs::run_one_turn_async` for the
implementation, and the test fixtures in
`crates/sdust-runtime/tests/cancellation_mid_turn.rs`.

## See also

- `docs/internals/scheduler.md` — tokio executor wrapper
- `docs/internals/mailboxes.md` — bounded MPSC + MessageFrame + slab pool
- `docs/internals/supervisors.md` — strategies + restart limits
- `docs/internals/budgets.md` — counters + sandbox allowlists
- `docs/internals/telemetry.md` — JSON event schema (legacy fallback)
- `docs/internals/telemetry-otlp.md` — v0.3 OTLP wire format
- `docs/spec/v0.1-amendments.md` — A36..A43, A70+ (v0.3)
