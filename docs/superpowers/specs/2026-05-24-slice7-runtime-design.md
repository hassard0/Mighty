# Slice 7 — Runtime MVP (spec §25 + §31.5)

**Date:** 2026-05-24
**Predecessor:** `v0.6.0-sir` (`068af1a`) — MtyIR + synchronous interpreter
**Target tag:** `v0.7.0-runtime`

## Goal

Make Mighty's interpreter a real, concurrent, deadline-aware,
supervisor-managed runtime. Slice 6 ships a single-thread tree-walker
with metadata-only agents, budgets, and sandboxes. Slice 7 wires those
metadata pieces into an actual asynchronous executor so:

- `spawn AgentName(args)` produces a long-lived actor with a mailbox
  and its own arena;
- `agent!Msg(args)` enqueues a fire-and-forget message;
- `agent?Msg(args) @2s` enqueues a request with a deadline-bounded
  reply;
- supervisors detect failed children and restart per strategy
  (`one_for_one`, `one_for_all`, `rest_for_one`, `escalate`) with rate
  limits (`restart up_to N in DUR`) and backoff (`backoff D1..D2`);
- budgets (CPU, wall, mem, mailbox-depth, spawned-tasks) and sandboxes
  (path / host allowlists) are enforced at runtime, raising the
  reserved MT5009 / MT5010 traps;
- a deterministic mode replays controlled interleavings against the
  same MtyIR program;
- `std.http.serve(":8080", api)` opens a real TCP listener and
  dispatches HTTP requests to the bound `Api` agent;
- a telemetry emitter writes OpenTelemetry-shaped spans (JSON) to
  stderr or `$STARDUST_TRACE_FILE` for each agent turn, send/ask, and
  supervisor restart.

Out of scope (deferred to slice 8 or post-v0.1):

- Multi-core work-stealing (single shared executor for slice 7).
- Native or Wasm codegen.
- Real cooperative cancellation semantics during a long-running turn
  (slice 7 honours deadlines between turns; cooperative inside a turn
  comes with codegen in slice 8).
- Distributed agents across machines.
- Field-level borrow tracking improvements.

## Architecture

A new crate `mty-runtime` sits between `mty-sir` and the OS:

```
mty-driver --> mty-runtime --> tokio (executor + timers)
                       |
                       +-- mty-sir::interp (per-turn evaluator)
                       |
                       +-- std host (real net/fs/time/rand)
```

The slice-6 interpreter becomes the **per-turn evaluator**: each agent
turn calls `interp::run_fn_by_name(prog, handler_name, args, host)`
inside a tokio task. The runtime owns scheduling, mailboxes, timers,
supervisor logic, and the live `Host` implementation.

### Crate layout

```
crates/mty-runtime/
  src/
    lib.rs              # public API
    runtime.rs          # Runtime, RuntimeBuilder
    scheduler.rs        # tokio executor wrapper + deterministic mode
    agent.rs            # AgentHandle, AgentDescriptor, AgentRegistry
    mailbox.rs          # Mailbox slabs, MessageFrame, ReplyHandle
    supervisor.rs       # Supervisor engine, strategies, backoff
    budget.rs           # BudgetTracker, BudgetWord
    timer.rs            # deadline wheel (tokio::time wrapper)
    telemetry.rs        # JSON span emitter (RUST_LOG-style)
    host_std.rs         # Net/Fs/Time/Rand effect impls
    deterministic.rs    # alt scheduler + seeded RNG
    test_harness.rs     # programmatic API for integration tests
    error.rs            # SD5xxx → RuntimeError mapping
  tests/
    mailbox_basic.rs
    agent_lifecycle.rs
    supervisor_strategies.rs
    budget_enforcement.rs
    timer_deadline.rs
    deterministic_replay.rs
    http_serve.rs
    sandbox_enforcement.rs
```

### Public API sketch

```rust
pub struct Runtime { /* … */ }

pub struct RuntimeBuilder {
    deterministic_seed: Option<u64>,
    telemetry_sink: TelemetrySink,
    default_budget: Budget,
    default_step_budget: u64,
    // …
}

impl RuntimeBuilder {
    pub fn new() -> Self;
    pub fn deterministic(self, seed: u64) -> Self;
    pub fn telemetry(self, sink: TelemetrySink) -> Self;
    pub fn build(self, prog: Arc<Program>) -> Runtime;
}

impl Runtime {
    pub fn spawn_agent(&self, name: &str, args: Vec<Value>) -> Result<AgentHandle>;
    pub fn send(&self, target: AgentHandle, msg: &str, args: Vec<Value>);
    pub fn ask(&self, target: AgentHandle, msg: &str, args: Vec<Value>,
               deadline: Option<Duration>) -> JoinHandle<Result<Value>>;
    pub fn run_main(self) -> RunResult;
    pub fn shutdown(self) -> RunResult;
}
```

The `mty run <file>` command builds a `Runtime` from the lowered
`Program`, spawns `main` on it, and pumps the executor until either
`main` returns or all agents quiesce.

### Per-agent layout (spec §25.2 descriptor)

```rust
pub struct AgentDescriptor {
    pub agent_id: u64,
    pub state: AgentState,        // owned mutable backing struct
    pub mailbox: Arc<Mailbox>,    // MPSC of MessageFrame
    pub arena: ArenaStack,        // per-turn arenas
    pub capabilities: CapBitmap,  // narrowed during spawn
    pub budget: BudgetWord,       // atomic counters
    pub supervisor: Option<SupervisorId>,
    pub sir_id: AgentSirId,
}
```

`AgentState` wraps a `Value::Struct` produced by the slice-6 ctor
function so the per-turn evaluator can read/write it through normal
MtyIR projections.

### Mailbox slabs (spec §25.3)

Slice 7 ships a bounded MPSC channel per agent, depth controlled by
the agent's budget (default 1024). The channel carries `MessageFrame`
values:

```rust
pub struct MessageFrame {
    pub proto_msg: String,       // e.g. "Ping", "Query"
    pub payload: SmallPayload,   // inline up to 4 Values; spills to Vec
    pub reply: Option<oneshot::Sender<Result<Value>>>,
    pub deadline: Option<Instant>,
}

enum SmallPayload {
    Inline([MaybeUninit<Value>; 4], usize),
    Spilled(Vec<Value>),
}
```

On a full mailbox, `send` either blocks (default), drops with a
warning, or fails the sender per the budget policy `mb_policy`
metadata (defaulting to `block`).

### Scheduling (spec §25.4)

Slice 7 uses tokio's multi-thread scheduler under the hood but caps
worker threads at 1 by default (`STARDUST_RUNTIME_THREADS=N` override,
documented). Each agent runs as a tokio task that loops:

1. `recv()` on the mailbox (bounded; backpressure handled upstream).
2. Tick the `BudgetTracker.before_turn()`; trap if exceeded.
3. Push a fresh arena onto `arena`.
4. Call the per-turn evaluator with the agent's state + message args.
5. On completion: pop the arena, send reply if `MessageFrame.reply` is
   `Some(_)`, emit a turn telemetry span, update budget counters, loop.
6. On trap: notify the supervisor (if any) via an
   `mpsc::UnboundedSender<ChildFailure>`.

The supervisor task watches `ChildFailure` events and applies the
configured strategy. Restart attempts honour `restart up_to N in DUR`
(deny after N attempts within window) and `backoff D1..D2` (uniform
jitter between `D1` and `D2`).

### Deadlines + timers (spec §25.4)

`Ask { deadline_ms: Some(d), .. }` wraps the response oneshot in
`tokio::time::timeout(Duration::from_millis(d), recv)`. On expiry the
reply resolves to `Result::Err(DeadlineExceeded)`; the MtyIR
interpreter materialises this as the appropriate `Result::Err`
variant when the typed error union is known, otherwise as a generic
`RuntimeError::Deadline` that the caller can match with `?`.

Generic `task scope @5s` (spec §14.1) and standalone deadline
literals attach a `tokio::time::sleep` cancellation guard to the
spawned task.

### Budget enforcement (spec §16.2)

A `BudgetTracker` holds atomic counters for:

| Counter        | Source                                      | Action on breach |
|----------------|---------------------------------------------|------------------|
| `cpu_ns`       | accumulated turn duration                    | `MT5009`         |
| `wall_ns`      | wall clock since budget start                | `MT5009`         |
| `mem_bytes`    | per-agent arena byte counter (approximate)   | `MT5009`         |
| `mailbox_max`  | enforced on `try_send`                       | `MT5009`         |
| `spawned`      | child count                                  | `MT5009`         |
| `paths`        | sandbox path allowlist                       | `MT5010`         |
| `hosts`        | sandbox host allowlist                       | `MT5010`         |

Slice-7 approximation for `mem`: each `Value` carries a synthetic byte
cost (1 byte per primitive, 24 per allocation header, etc.); the
arena accumulates these on push/pop. A real allocator integration
ships in slice 8.

The supervisor receives breaches as `ChildFailure::Budget(kind)` and
follows the same strategy table as ordinary panics. Top-level breach
(no supervisor) traps the whole run.

### Telemetry

The runtime emits JSON lines on stderr by default (one event per
agent turn, send/ask call, supervisor decision):

```json
{"ts": 1716552123000, "kind": "turn_start", "agent": "Searcher", "msg": "Query"}
{"ts": 1716552123005, "kind": "send", "from": "main", "to": "Api", "msg": "Request"}
{"ts": 1716552123007, "kind": "ask", "from": "Api", "to": "Searcher", "msg": "Query", "deadline_ms": 2000}
{"ts": 1716552123042, "kind": "turn_end", "agent": "Searcher", "msg": "Query", "duration_us": 35000}
{"ts": 1716552123042, "kind": "restart", "supervisor": "SearchFlow", "child": "planner", "attempt": 1}
```

The schema is OpenTelemetry-flavoured but not strict; the goal is
useful structured logs slice 7 can consume without committing to the
full OTLP wire format. `STARDUST_TRACE=off|stderr|file:PATH` controls
the sink.

### Deterministic mode (spec §25.5)

`RuntimeBuilder::deterministic(seed)` swaps the executor for a
single-thread cooperative scheduler:

- All tasks run on one tokio current-thread runtime.
- Time advances by an injected `Clock` (no system clock reads).
- Mailbox draining order is FIFO + sorted by (deadline_ms, frame_id)
  to break ties deterministically.
- RNG is seeded by `seed` and exposed via the `Host::rng_u64()` hook.

This backs the `test deterministic "name" { runtime.det { ... } }`
syntax: slice 7 parses + lowers this to a `Stmt::DeterministicScope`
hint that the runtime honours when `mty run` sees it inside the
`main` body. Standalone test files run via the deterministic mode by
default.

### `std.http` minimal server

A new `EffectOp` family routes `http.serve(addr, agent)` and
`http.ok(body)` calls to the runtime. For slice 7:

- `http.serve(addr, agent)` binds a `tokio::net::TcpListener` on
  `addr`, parses HTTP/1.1 requests (a tiny in-tree parser, no extra
  deps), turns each into a `Request { method, path, body }` value, and
  asks the agent: `agent?Request(req) @30s`. The reply value is
  serialised back as an HTTP response (`Json(body)` → 200 with
  `Content-Type: application/json`; `Bytes(b)` → 200; explicit
  `(status, body)` tuple supported).
- `http.ok(body)` returns `(200, body)`.

If the `STARDUST_HTTP_MOCK=1` env var is set (used by tests), the
runtime registers an in-memory queue instead of binding a TCP socket
so `http_serve.rs` doesn't need to pick a free port.

A36 records this as the slice-7 stdlib surface.

## Plan-time interpretations + amendments

- **A36** — `std.http.serve` MVP shape (above).
- **A37** — Slice-7 budget approximation (1 B / primitive, etc.).
- **A38** — Telemetry JSON schema is OTLP-flavoured, not OTLP-strict.
- **A39** — Deterministic mode = single-thread + seeded clock + FIFO
  mailbox ordering + seeded RNG.
- **A40** — Mailbox default depth 1024 (`mb`), default policy `block`.
- **A41** — `task scope @D` cancels via `tokio::time::sleep` guard;
  slice-7 cancellation kicks in **at the next await point** (no
  preemption inside a synchronous MtyIR turn). The next-await behaviour
  is fine because slice-7 turns are bounded by step budget (default
  1 M steps); a turn cannot run forever.
- **A42** — `restart up_to N in DUR` denies after N restarts within a
  sliding `DUR` window; on denial the supervisor escalates one level.
- **A43** — Top-level `sandbox` items (A27) execute as a child Runtime
  with the declared budget caps applied; violations trap with MT5010.

## Testing strategy

- **Unit tests in `mty-runtime`** (target ~60 new tests):
  - Mailbox FIFO, bounded behaviour, drop-on-full policy, send-block.
  - Timer wheel — deadlines fire at the expected logical time in
    deterministic mode.
  - Budget tracker — each counter and each policy decision.
  - Supervisor strategies — table-driven across `one_for_one`,
    `one_for_all`, `rest_for_one`, `escalate`.
  - Restart rate-limit window (5 restarts in 1 s denies the 6th).
  - Backoff jitter is within range.
  - Telemetry emits the documented shapes for each kind.
- **Integration tests** wiring MtyIR programs through the runtime:
  - Agent echo (example 07) — spawn, send, observe state.
  - Counter (example 08) — three sends, ask state, expect 3.
  - Send + ask + deadline (example 09) — succeeds + times out.
  - Supervisor (example 10) — child fails → restart attempts traced.
  - Budget block (example 11) — CPU breach traps with MT5009.
  - Sandbox (example 18) — path-allowlist breach traps with MT5010.
  - Backend service (example 19) — start, hit `:8080`, observe reply.
- **Conformance corpus** under `tests/conformance/runtime-7/`:
  - 8 new cases covering scheduling, supervisor, budgets, deadlines,
    deterministic replay.
- **Determinism property**: run example 09 ten times with the same
  seed; observed event log must be byte-identical.

## Concrete deliverables

1. New crate `mty-runtime` (~3 000 lines target, comparable to
   `mty-sir`).
2. `mty-driver` and `mty-cli` rewired so `mty run` uses the
   runtime (slice-6 synchronous fallback retained behind
   `--legacy-interp` for diagnostic comparison).
3. Examples 07/08/09/10/11/18/19 run end-to-end (vs lower-only).
4. 290 baseline tests still pass; ~60 new tests added (target ~350).
5. MT5009 / MT5010 / MT5011..MT5050 expanded with runtime-specific
   sub-codes (`MT5011 deadline_exceeded`, `MT5012 mailbox_full`,
   `MT5013 supervisor_escalated`, `MT5014 restart_limit_exceeded`,
   `MT5015 capability_outside_sandbox`, `MT5050 extern_fn_unimpl`
   unchanged).
6. Documentation: `docs/internals/runtime.md`,
   `docs/internals/scheduler.md`, `docs/internals/mailboxes.md`,
   `docs/internals/supervisors.md`, `docs/internals/budgets.md`,
   `docs/internals/telemetry.md`, `docs/reference/cli/mty-run.md`
   updated with runtime flags, tour pages for agents/supervisors/
   budgets updated with "now actually runs!" markers.
7. Amendments A36..A43 added to `docs/spec/v0.1-amendments.md`.
8. `SLICE7.md` summary committed.
9. Tag `v0.7.0-runtime` pushed to `origin/main`.

## Dependencies added

- `tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "net", "io-util", "sync"] }`
- `parking_lot = "0.12"` (mutexes for shared registry; faster + non-poisoning)
- `dashmap = "5"` (concurrent agent registry)
- (No new dev-deps; we keep using `insta` for snapshot tests.)

`tokio` is added at the workspace level under `[workspace.dependencies]`.

## Risk register

| Risk | Mitigation |
|------|------------|
| Tokio leaks into too many crates and bloats build times | Keep `tokio` confined to `mty-runtime`; `mty-driver` only re-exports `Runtime`. |
| Real HTTP serve is fragile in CI (port binding, firewall) | Provide `STARDUST_HTTP_MOCK=1` to bypass TCP in tests. |
| Determinism mode drifts from spec §25.5 | Pin a "replay" test that runs example 09 ten times under one seed and diffs the telemetry stream. |
| Per-turn arena byte accounting is approximate | Documented as A37; slice 8 will integrate a real arena allocator. |
| Supervisor restart loops chew CPU under failing child | Restart rate limit (A42) caps it; backoff jitter prevents thundering herd. |
| Tokio + Windows TCP edge cases | `http.serve` is gated by env var; default `mty run examples/19_backend_service.sd` prints a hint and exits 0 if `STARDUST_HTTP_REAL=1` is not set. |

## Success criteria

- `cargo test --workspace` passes (~350 tests, no failures).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `mty run examples/07_agent_echo.sd` exercises spawn + send.
- `mty run examples/10_supervisor.sd` reports a restart sequence
  for a child that intentionally fails (example 10 lacks a body that
  causes failure today; we add a `main()` to make it executable and
  introduce a `fail_after_n` helper in the prelude so the run is
  observable).
- `mty run examples/11_budget_block.sd` traps with MT5009 when
  `cpu 150ms` is exceeded by an infinite-loop helper.
- `STARDUST_HTTP_REAL=1 mty run examples/19_backend_service.sd`
  starts listening on `:8080` and serves at least one mock request
  (verified by a curl-style test in `tests/http_serve.rs`).
- Determinism replay: 10 runs of example 09 under
  `STARDUST_DET_SEED=1` produce byte-identical telemetry.
- A36..A43 amendments documented.
- SLICE7.md describes the slice; SLICE6.md gets a deferral-cleanup
  note pointing at A36..A43.
- Tag `v0.7.0-runtime` exists at HEAD with all changes pushed.
