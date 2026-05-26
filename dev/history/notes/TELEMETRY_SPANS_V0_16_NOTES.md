# Telemetry — v0.16 OpenTelemetry agent spans

**Release:** v0.16
**Roadmap:** Tier 1.2 + Tier 1.3 (`docs/internals/agent-features-roadmap.md`)
**Reference:** `docs/reference/telemetry.md`
**Internals:** `docs/internals/telemetry-spans.md`

## What shipped

A new `telemetry/` submodule under `crates/mty-runtime/src/`:

- `telemetry/mod.rs` — `init_from_env()`, `shutdown()`, lazy global
  `TracerProvider` slot keyed off `MTY_OTLP_ENDPOINT`.
- `telemetry/spans.rs` — RAII span guards (`SpawnGuard`,
  `HandlerGuard`) + helpers (`span_spawn`, `span_send`, `span_ask`,
  `span_handler`, `record_restart`, `record_budget_exhausted`) + a
  `tokio::task_local!` holding the active handler-span context.
- `telemetry/events.rs` — `agent_event(name, &[(k, v)])` that routes
  to the active handler span (when one is set) or to a stdout JSON
  line (otherwise).
- `telemetry/sink.rs` — the existing slice-7 / v0.3 JSON-line + OTLP
  event emitter, **moved** from `telemetry.rs` and re-exported through
  the new `mod.rs` so every existing import (`use
  mty_runtime::telemetry::{TelemetryEvent, TelemetrySink}`) keeps
  working without changes.

Re-exports from the crate root (`lib.rs`):

```rust
pub use telemetry::{
    agent_event, init_from_env as init_telemetry_from_env,
    record_budget_exhausted, record_restart, shutdown as shutdown_telemetry,
    span_ask, span_handler, span_send, span_spawn, HandlerGuard,
    SpanContext, SpawnGuard,
};
```

The existing `pub use telemetry::{TelemetryEvent, TelemetrySink}` is
unchanged.

## Span names + attributes

| Span                | Kind     | Attributes                              |
|---------------------|----------|-----------------------------------------|
| `agent.spawn`       | Internal | `agent.type`                            |
| `agent.send`        | Internal | `protocol.msg`                          |
| `agent.ask`         | Internal | `protocol.msg`                          |
| `agent.handler`     | Internal | `agent.type`, `agent.handler`           |
| `supervise.restart` | Internal | `reason` (event on parent span, or standalone) |
| `budget.exhausted`  | Internal | `reason` (event on parent span, or standalone) |

Attribute namespace decision: we use the bare `agent.*` /
`protocol.*` namespace rather than `mighty.agent.*` /
`mighty.protocol.*`. Rationale: these are *generic* agent-system
attributes — a downstream cross-runtime dashboard (mighty + other
actor runtimes) benefits from a shared namespace. The v0.3 OTLP
bridge in `crate::otlp` uses `mighty.*` for its event spans; that
stays unchanged because it's a different layer (event-shaped, not
span-shaped).

Resource attributes (constant per process):
`service.name = "mighty-runtime"`, `service.version = <CARGO_PKG_VERSION>`.

## `MTY_OTLP_ENDPOINT` activation tested

- `init_from_env()` with no env var → no-op, no provider installed.
  Verified by `init_with_no_env_is_noop` in
  `crates/mty-runtime/tests/telemetry.rs`.
- `init_from_env()` with `MTY_OTLP_ENDPOINT=http://127.0.0.1:14317`
  (port not listening) → provider installs, exporter defers
  connection. No panic. Verified by `init_with_env_attempts_otlp`.
- `MTY_OTLP_PROTOCOL=http` selects the HTTP/protobuf exporter
  (otherwise gRPC/tonic).
- `MTY_OTLP_SAMPLE_RATE=0.25` switches the sampler to
  `TraceIdRatioBased(0.25)`. Outside `[0.0, 1.0]` falls back to
  `AlwaysOn`. Verified by `init_with_sample_rate_env`.

## `agent_event` shape

```rust
pub fn agent_event(name: &str, fields: &[(&str, &str)]);
```

Routing:

1. If the current tokio task is inside a `HANDLER_SPAN.scope(...)`
   (set by the runtime when dispatching a handler), the event is
   added to the active span as an OTel `Event` with the given
   attributes.
2. Otherwise, the event is written as a single JSON line to stdout:
   `{"kind":"agent_event","name":"...","fields":{"k":"v",...}}`.

The line shape matches the slice-7 sink's existing format so the
fallback works with log tooling already wired up.

## Tests

10 integration tests in `crates/mty-runtime/tests/telemetry.rs`:

1. `init_with_no_env_is_noop`
2. `init_with_env_attempts_otlp`
3. `span_spawn_closes_on_drop`
4. `agent_event_stdout_fallback`
5. `supervisor_and_budget_helpers_outside_handler_are_safe`
6. `span_handler_runs_for_dispatch`
7. `init_with_sample_rate_env`
8. `init_is_idempotent`
9. `shutdown_without_init_is_safe`
10. `span_send_returns_immediately`

Plus inline `#[cfg(test)]` tests in each submodule covering:

- `mod.rs::tests` — `shutdown_when_uninit_is_noop`,
  `is_enabled_false_by_default`.
- `spans.rs::tests` — disabled-path guard creation, closure
  execution, fire-and-forget safety, helper safety, empty-context
  default.
- `events.rs::tests` — line formatter (3 cases) + outside-handler
  safety.

## v0.17 follow-ups

1. **Cross-agent correlation IDs.** Thread a `traceparent` field
   through `MessageFrame` so `ask`'s receiver-side `agent.handler`
   span becomes a true child of the caller's `agent.ask` span. The
   underlying tokio task-local infrastructure is in place; the wire
   change is in `crate::mailbox::MessageFrame`.

2. **Wire instrumentation into the runtime's spawn/send/ask call
   sites.** The helpers are public and ready; the v0.16 ship leaves
   the actual `RuntimeBuilder::build` / `Runtime::spawn_agent` /
   `Runtime::send` / `Runtime::ask` bodies untouched so the
   introspection-agent sibling can land its control-socket changes
   to the same file without conflict. The wiring is one-line each
   (RAII guard at function entry).

3. **OTel standard env vars.** Today we use `MTY_OTLP_*` only.
   Adding `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME`,
   `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_TRACES_SAMPLER`,
   `OTEL_TRACES_SAMPLER_ARG` would make Mighty drop-in compatible
   with the standard OTel SDK env-var contract.

4. **Body-capture capability.** A separate opt-in capability adds
   the (truncated) message body as a `protocol.body` attribute. Off
   by default; lives behind its own env var so privacy stays
   restrictive.

5. **Exemplars.** Once Tier 1.4 metrics ship, expose trace-exemplars
   on the budget-exhaustion + restart counters.

## Wire-shape decisions

* **`agent.*` namespace** (not `mighty.*` and not the OTel
  `messaging.*` semantic convention). The OTel `messaging.*`
  convention is shaped around queueing systems (topics, partitions)
  and doesn't map cleanly to actor handlers. `agent.*` is generic
  enough to share across actor runtimes; `mighty.*` already names
  the v0.3 event-shaped spans so we avoid the collision.

* **`protocol.msg`** for the message-variant name (not `message.name`
  or `msg`). `protocol.*` mirrors how the spec talks about agent
  protocols (`spec §15`); keeping it consistent across `send` /
  `ask` / handler events helps dashboards group related spans.

* **`reason` for restart / budget events.** Not `cause`, not
  `error.message`. Keep it short; the caller picks the string and
  is responsible for cardinality control. Future revision may
  introduce a small `reason_enum` to bound the attribute domain.

* **`tokio::task_local!` for the handler-span context.** Not
  `std::thread_local!` — the runtime is tokio-driven and a single
  OS thread serves many agent tasks. A thread-local would leak
  between agents on the same worker.

## Concurrency notes (swarm build)

The v0.16 swarm built this slice alongside the
introspection-agent sibling (control socket + `agent.introspect()`)
which also touches `crates/mty-runtime/src/lib.rs`. Coordination:

- Telemetry adds a single block at the end of `lib.rs` (one
  `pub mod telemetry;` line — kept from before — plus a second
  `pub use telemetry::{...};` block). The introspect agent's
  changes land before the telemetry block, so the two are
  conflict-free under `git`'s default 3-way merge.
- `telemetry.rs` was renamed to `telemetry/sink.rs` via `git mv`
  so the slice-7 history is preserved.
- No changes to `runtime.rs`, `agent.rs`, `supervisor.rs`, or the
  budget path in this slice; those are the introspect agent's
  surfaces. Wiring the span helpers into those call sites is a
  v0.17 follow-up (see above).
