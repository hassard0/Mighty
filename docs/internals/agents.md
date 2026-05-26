# Agents — the first-class concurrency primitive (internals)

**Crate:** `mty-runtime` (with surface support in `mty-types`, `mty-hir`,
`mty-sir`, `mty-codegen-*`)
**Spec:** §22 Agents, §23 Protocols & Messages, §25 Runtime Architecture
**Reference:** [`docs/internals/runtime.md`](runtime.md),
[`docs/internals/mailboxes.md`](mailboxes.md),
[`docs/internals/supervisors.md`](supervisors.md)

This document is a cross-cutting summary of how an `agent` declaration
in surface Mighty becomes a live tokio-backed task with mailboxes,
budgets, and supervision. For deep dives on each piece, follow the
"see also" links below.

## Surface form

```mighty
agent Counter {
  state { count: i32 = 0 }

  msg Increment(n: i32)
  msg Get() -> i32

  on Increment(n) { self.count += n }
  on Get() -> i32 { self.count }
}
```

An `agent` declaration introduces three things into the type system:

1. **A nominal type** `Counter` whose value space is the set of live
   instances (addressed by `AgentHandle<Counter>` and — after v0.18 —
   by `AgentAddr` when the cluster mesh is opted in).
2. **A protocol** — the set of `msg` arms declares the messages the
   agent accepts. Each message is itself a typed value; sending a
   non-protocol message is MT-coded at compile time
   (MT2014, MT2015 family).
3. **State** — a struct-shaped record whose fields are the agent's
   private mutable state. The state shape is checked for `Sendable`
   (see `docs/internals/sendable.md`) where it crosses the
   spawn-arg boundary.

## Lowering pipeline

| Stage              | Where                                       | Outcome                                                                                        |
|--------------------|---------------------------------------------|------------------------------------------------------------------------------------------------|
| Parse              | `crates/mty-syntax/src/parser/items.rs`     | `AGENT_DECL` CST node with `STATE_DECL`, `MSG_DECL`s, `ON_HANDLER`s                            |
| AST view           | `crates/mty-ast/src/generated.rs`           | `AgentDecl`, `StateDecl`, `MsgDecl`, `OnHandler` accessors                                     |
| HIR lowering       | `crates/mty-hir/src/lower/agents.rs`        | `HirAgent { id, name, state, msgs, handlers }` in the arena                                    |
| Typeck             | `crates/mty-types/src/check/agents.rs`      | `AgentTy`, per-msg arg/return types, Sendable check on spawn args, effect rows on handlers     |
| Borrowck           | `crates/mty-borrow/src/agents.rs`           | Handler bodies are checked with `self: &mut Self`; `state` field places tracked NLL            |
| MtyIR lowering     | `crates/mty-ir/src/lower/agents.rs`         | One function per handler + a dispatch table; spawn lowers to `Rvalue::SpawnAgent`              |
| Codegen (native)   | `crates/mty-codegen-cranelift/src/agents.rs`| Generates the dispatch function pointer + thunks for each `on` arm                             |
| Codegen (wasm)     | `crates/mty-codegen-wasm/src/agents.rs`     | Component-model export shape; per-handler `func`s                                              |
| Runtime spawn      | `crates/mty-runtime/src/agent.rs`           | `AgentDescriptor` + `AgentRegistry::register` + `spawn_agent_loop`                             |

## Runtime shape

Once an agent is live, the runtime owns:

| Field                       | Type                                  | Purpose                                                                       |
|-----------------------------|---------------------------------------|-------------------------------------------------------------------------------|
| `id`                        | `AgentId` (u64)                       | Process-unique handle                                                         |
| `name`                      | `Arc<str>`                            | Surface type name (e.g. `"Counter"`)                                          |
| `mailbox`                   | `Mailbox` (bounded MPSC)              | Inbound messages, capacity defaults to 1024 (overridable via budget)          |
| `state`                     | `parking_lot::Mutex<Value>`           | Per-agent mutable state, held only for the duration of a turn                 |
| `budget`                    | `BudgetTracker`                       | CPU, memory, ticks, wall-clock                                                |
| `supervisor_parent`         | `Option<AgentId>`                     | Parent in the supervision tree                                                |
| `introspect_state`          | `Arc<AgentIntrospectState>` (v0.16)   | Live mailbox depth, in-flight handler, last-N message names                   |
| `cluster_addr`              | `Option<AgentAddr>` (v0.18, opt-in)   | Routable cross-node address when a `ClusterRouter` is installed               |

Each agent runs in its own tokio task. The per-turn loop:

```text
loop {
  msg = mailbox.recv().await
  budget.check()
  telemetry.emit(TurnStart { agent, msg })
  replay::record_message_handled(agent, msg, elapsed_us)    // v0.18, opt-in
  state' = run_handler_isolated(prog, handler, state, args, host)
  state.write(state')
  telemetry.emit(TurnEnd  { agent, msg, duration_us })
  if reply: reply_channel.send(handler_return)
}
```

See [`docs/internals/runtime.md`](runtime.md) for the full per-turn
sequence + trap handling.

## Spawn anatomy

```rust
let api = runtime.spawn_agent("Api", vec![arg0, arg1]).await?;
```

| Step | Action                                                                 |
|------|------------------------------------------------------------------------|
| 1    | `AgentRegistry::register(name)` → fresh `AgentId`                      |
| 2    | Mailbox MPSC pair created, bounded                                     |
| 3    | `BudgetTracker::new(budget_spec)` from agent metadata or defaults      |
| 4    | `AgentIntrospectState::default()` for snapshot reads                   |
| 5    | `replay::with_recorder(|rec| rec.record_spawn(id, name, parent))`      |
| 6    | tokio task spawned: `spawn_agent_loop(desc, mailbox_rx, host, ...)`    |
| 7    | Caller receives `AgentHandle { id, sender }`                           |

The `spawn_agent_loop` function is the trap boundary — a panic inside
the handler turns into a `RuntimeError::Trap(MT5001)` which the
supervisor (if any) sees as a `ChildFailure::Trap`. See
[`docs/internals/supervisors.md`](supervisors.md) for restart strategies.

## Send / Ask

* **`runtime.send(handle, "Msg", args).await`** is fire-and-forget.
  The frame is pushed onto the mailbox; the caller does not block on
  the agent's handler. Records as `MessageSent` in replay traces.
* **`runtime.ask(handle, "Msg", args, deadline).await`** is
  request-reply. The frame carries a `tokio::sync::oneshot::Sender`
  that the agent's handler signals at end-of-turn. The caller awaits
  the oneshot; deadline triggers a `RuntimeError::DeadlineExceeded`
  (MT5006).

Send and Ask are the only entry points; direct field access on `self`
inside a handler is the only legal mutation path. `Sendable` (see
`docs/internals/sendable.md`) enforces that args crossing the
spawn/send boundary are deep-clonable across the actor isolation.

## Visibility surfaces

| Surface                      | Where                                                | Off-by-default? |
|------------------------------|------------------------------------------------------|-----------------|
| `mty inspect`                | `crates/mty-runtime/src/{introspect, control_socket}` (v0.16) | Yes (env)       |
| OTel agent spans             | `crates/mty-runtime/src/telemetry/spans.rs` (v0.16)  | Yes (env)       |
| Deterministic replay         | `crates/mty-runtime/src/replay/*` (v0.17/18)         | Yes (env)       |
| Slice-7 JSON-line telemetry  | `crates/mty-runtime/src/telemetry/sink.rs`           | Yes (env)       |
| Cluster mesh                 | `crates/mty-runtime/src/cluster/*` (v0.18)           | Yes (config)    |

Each surface follows the v0.16 contract: zero hot-path overhead when
the relevant env / config is absent, additive activation under a
documented env var.

## Capability + budget enforcement

Agent handlers run under the same capability table the spawn-time
context provided. The runtime does not grant new capabilities mid-
agent; surface-level cap-narrow constructs (`with cap.fs.read_only`)
lower to a child agent with a derived cap table.

Budget enforcement runs at three points:

1. **Pre-turn**: `BudgetTracker::check()` rejects the turn if any
   limit is already exceeded (MT5009).
2. **Mid-turn**: the interpreter / codegen-emitted check stubs
   (cooperative cancellation) call `BudgetTracker::record_cpu` and
   honour the cancel flag at safe points.
3. **Post-turn**: wall-clock budget is checked after the turn
   returns; deadline exhaustion emits both `BudgetExhausted` (replay)
   and `BudgetExceeded` (supervisor trap).

See [`docs/internals/budgets.md`](budgets.md) for the full breach
taxonomy.

## Roadmap (Tier 4+)

* **Tier 4.1 (v0.18, SHIPPED)** — single-cluster mesh + `AgentAddr`
  with `node` axis. Transport layer landed; `Runtime::send` consults
  the router in v0.19. See [`docs/internals/cluster.md`](cluster.md).
* **Tier 4.2** — cluster-aware supervisors (one-for-one, all-for-one,
  rest-for-one across nodes).
* **Tier 4.3** — lossless live agent migration (`migrate(addr,
  target_node)`).
* **Tier 5** — per-message work-stealing scheduler.

## See also

* [`docs/internals/runtime.md`](runtime.md) — runtime API + turn loop
* [`docs/internals/scheduler.md`](scheduler.md) — multi-core scheduler
* [`docs/internals/mailboxes.md`](mailboxes.md) — bounded MPSC details
* [`docs/internals/supervisors.md`](supervisors.md) — restart strategies
* [`docs/internals/budgets.md`](budgets.md) — per-handler limits
* [`docs/internals/introspect.md`](introspect.md) — `mty inspect`
* [`docs/internals/telemetry-spans.md`](telemetry-spans.md) — OTel
* [`docs/internals/replay.md`](replay.md) — deterministic replay
* [`docs/internals/cluster.md`](cluster.md) — distributed agents
* [`docs/internals/sendable.md`](sendable.md) — cross-agent arg soundness
* [`docs/internals/agent-features-roadmap.md`](agent-features-roadmap.md) — Tier roadmap
