# Telemetry — OpenTelemetry agent spans (internals, v0.16)

**Module:** `mty_runtime::telemetry` (submodules `spans`, `events`, `sink`)
**Roadmap:** Tier 1.2 + Tier 1.3 — `docs/internals/agent-features-roadmap.md`
**User-facing reference:** `docs/reference/telemetry.md`

This document explains the internal architecture of the v0.16
OpenTelemetry agent-span layer. For configuration and user-visible
behaviour see `docs/reference/telemetry.md`.

## Module shape

```
crates/mty-runtime/src/telemetry/
  ├── mod.rs       — init_from_env() / shutdown() / global tracer slot
  ├── spans.rs     — RAII span guards + tokio task-local handler context
  ├── events.rs    — agent_event() + stdout-fallback line formatter
  └── sink.rs      — slice-7 JSON-line + TelemetryEvent / TelemetrySink
                     (moved from telemetry.rs — backwards-compat re-exported)
```

The split reflects two cooperating layers:

* `sink` — the original slice-7 event emitter. Used by the runtime's
  internal call sites (`Runtime::send`, `Runtime::ask`,
  `run_one_turn_async`, supervisor, budget breach) via
  `TelemetrySink::emit`. Output goes to a JSON file/stderr sink or to
  OTLP via the v0.3 `crate::otlp::OtlpHandle`.
* `spans` + `events` + `mod` — the v0.16 span layer. Exposes RAII
  guards that wrap OTel spans, plus a `tokio::task_local!` holding
  the active handler-span context. User code calls `agent_event` to
  attach structured events to the active span.

The two layers do **not** share state. Each can be enabled or
disabled independently. Setting `MTY_OTLP_ENDPOINT` enables the span
layer; setting `STARDUST_OTLP_ENDPOINT` enables the sink's OTLP
bridge. Either, both, or neither may be active in any given run.

## Lifecycle

### Initialisation

```text
main()
 ├── init_from_env()         (read MTY_OTLP_ENDPOINT, build provider)
 │     └── otlp_provider::install()
 │           ├── SpanExporter::builder().with_tonic()|with_http()
 │           ├── opentelemetry_sdk::trace::TracerProvider::builder()
 │           │     .with_batch_exporter(..., Tokio runtime)
 │           │     .with_resource(service.name + version)
 │           │     .with_sampler(AlwaysOn or TraceIdRatio)
 │           ├── opentelemetry::global::set_tracer_provider(provider)
 │           └── stash provider in OnceLock<Mutex<Option<Provider>>>
 └── ... runtime work ...
```

The provider is stashed in a `OnceLock<Mutex<Option<…>>>` so
`shutdown()` can `.take()` it and flush+shutdown gracefully. The
global provider via `opentelemetry::global::set_tracer_provider` is
what `global::tracer(name)` calls hit on the hot path.

### Shutdown

```text
shutdown()
 └── otlp_provider::shutdown()
       └── if let Some(p) = OnceLock slot.take():
             p.force_flush()
             p.shutdown()
```

Idempotent — calling `shutdown()` twice or without prior init is a
no-op. The atomic `TELEMETRY_ENABLED` flag is cleared so a subsequent
`init_from_env()` re-installs cleanly (useful in tests that toggle
env vars).

## Span construction — disabled path

Every span helper calls `build_span(name, attrs)` which:

```rust
fn build_span(name, attrs) -> SpanContext {
    let Some(tracer) = super::global_tracer("mty-runtime") else {
        return SpanContext::empty();
    };
    // ... build span ...
}
```

`global_tracer` returns `None` whenever:

1. The `otlp` cargo feature is disabled (compile-time), or
2. `is_installed()` returns false (no provider stashed).

On the disabled path the returned `SpanContext` is `{ inner: None }`
— a `Option<Arc<…>>`. Drop is a single null-check. No allocation, no
mutex. This is what makes the helpers ~free when telemetry is off.

## RAII guards

```rust
pub struct SpawnGuard { inner: SpanContext }
impl Drop for SpawnGuard { fn drop(&mut self) { end_span(&self.inner) } }
```

Same shape for `HandlerGuard`. `end_span` is `if let Some(inner) =
&ctx.inner { inner.end(); }` — null on the disabled path, a single
`.lock().take().end()` on the enabled path.

The two split guard types (`SpawnGuard` / `HandlerGuard`) exist to
distinguish "this is a long-lived span around agent creation" from
"this is a per-message-dispatch span". Future revisions may use the
type distinction for static checks (e.g. forbidding nesting of two
spawn guards).

## Task-local handler context

```rust
tokio::task_local! {
    pub(crate) static HANDLER_SPAN: SpanContext;
}
```

Set with `HANDLER_SPAN.scope(ctx, async move { ... }).await` — this
is the wiring point future runtime patches will use to make `ask`
spans children of the calling handler. Today only the receiver-side
handler span is bound; the caller-side `span_ask` opens a sibling
span that ends when the closure returns.

`current_handler_context()` does `HANDLER_SPAN.try_with(|c| c.clone())`
so reading the context outside a scope is safe (returns
`SpanContext::empty()`).

## `agent_event` routing

```rust
pub fn agent_event(name: &str, fields: &[(&str, &str)]) {
    let ctx = current_handler_context();
    if ctx.is_some() {
        ctx.add_event(name, fields);   // OTel span event
        return;
    }
    println!("{}", format_event_line(name, fields));  // stdout fallback
}
```

The JSON line format is stable: log aggregators already wired to the
slice-7 sink's `{"kind":"agent_event","name":"...","fields":{...}}`
line shape pick the events up without changes.

## Integration with existing call sites

The v0.16 layer is **additive**: the existing `TelemetrySink::emit`
calls in `Runtime::spawn_agent`, `Runtime::send`, `Runtime::ask`,
`run_one_turn_async` etc. continue to emit their slice-7 JSON-line
events. A future revision will wrap those same call sites in
`span_spawn` / `span_send` / `span_ask` so the duration-span layer
captures the wall-clock latency directly (today the slice-7 sink
already emits `TurnStart` + `TurnEnd` with `duration_us`, which the
v0.3 `crate::otlp::OtlpHandle` translates into a span pair).

The shape we ship in v0.16:

* Span helpers + `agent_event` are public API and usable from any
  call site (including downstream user crates).
* Wiring them into the runtime's internal spawn/send/ask paths is
  trivial (one line each) but is held to a minimal-touch edit so the
  v0.16 swarm-build's introspection-agent sibling can land its
  control-socket changes to the same files cleanly.

## Sampling

Today the sampler is configured at init time via
`MTY_OTLP_SAMPLE_RATE`. The OTel SDK's `TraceIdRatioBased` keeps the
sampling decision consistent across all spans in a trace.

A future revision will support `OTEL_TRACES_SAMPLER` /
`OTEL_TRACES_SAMPLER_ARG` (the standard OTel env vars) so production
deploys can plug in `parentbased_traceidratio` and friends.

## Test coverage

* `crates/mty-runtime/src/telemetry/mod.rs::tests` — `shutdown` /
  `is_enabled` smoke.
* `crates/mty-runtime/src/telemetry/spans.rs::tests` — disabled-path
  guards, closure execution, helper safety.
* `crates/mty-runtime/src/telemetry/events.rs::tests` — line
  formatter (no-fields, with-fields, escaping).
* `crates/mty-runtime/tests/telemetry.rs` — public-API smoke: init
  noop, init with env, RAII drop, `agent_event` fallback, supervisor
  + budget helpers safe outside a handler, sample-rate env, idempotent
  init, shutdown without init, `span_send` throughput sanity.

## See also

- `docs/reference/telemetry.md` — user-facing reference
- `docs/internals/agent-features-roadmap.md` — Tier 1 plan
- `docs/internals/telemetry.md` — slice-7 sink
- `docs/internals/telemetry-otlp.md` — v0.3 OTLP bridge
- `dev/history/notes/TELEMETRY_SPANS_V0_16_NOTES.md` — v0.16 ship notes
