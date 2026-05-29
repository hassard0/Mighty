# Telemetry — OpenTelemetry agent spans (v0.16)

**Status:** stable for v0.16
**Module:** `mty_runtime::telemetry`
**Roadmap:** Tier 1.2 + Tier 1.3 (`docs/internals/agent-features-roadmap.md`)

Mighty's runtime emits OpenTelemetry spans for every agent operation
when an OTLP collector endpoint is configured. The instrumentation is
off by default — runtimes with no `MTY_OTLP_ENDPOINT` pay zero cost.

For the slice-7 JSON-line event emitter (`MTY_OTLP_ENDPOINT`,
`MTY_TRACE`; legacy `STARDUST_*` spellings still honoured), see
`docs/internals/telemetry.md` and
`docs/internals/telemetry-otlp.md`. The two layers coexist: the
slice-7 sink emits point-in-time events at every call site; the
v0.16 span layer adds long-lived spans with parent-child relationships
and a user-callable `agent.event()` helper.

## Configuration

| Env var                  | Meaning                                                            | Default      |
|--------------------------|--------------------------------------------------------------------|--------------|
| `MTY_OTLP_ENDPOINT`      | OTLP collector URL (e.g. `http://localhost:4317`). Unset = disabled. | unset        |
| `MTY_OTLP_PROTOCOL`      | `grpc` or `http` (= `http/protobuf`).                              | `grpc`       |
| `MTY_OTLP_SAMPLE_RATE`   | Trace-id-based sampler ratio (`0.0`..=`1.0`).                      | `1.0` (AlwaysOn) |

Call `mty_runtime::init_telemetry_from_env()` once at program startup
and `mty_runtime::shutdown_telemetry()` before exiting. Both are
idempotent; both are safe if telemetry was never configured.

```rust
fn main() {
    mty_runtime::init_telemetry_from_env();
    let runtime = mty_runtime::RuntimeBuilder::new().build(prog);
    // ... drive the runtime ...
    mty_runtime::shutdown_telemetry();
}
```

## Span schema

| Span name             | When                                          | Attributes                                     |
|-----------------------|-----------------------------------------------|------------------------------------------------|
| `agent.spawn`         | `Runtime::spawn_agent` is called              | `agent.type` (string)                          |
| `agent.send`          | A fire-and-forget message is enqueued         | `protocol.msg` (string)                        |
| `agent.ask`           | An `ask` round-trip runs (parent of the reply) | `protocol.msg` (string)                        |
| `agent.handler`       | A handler dispatch runs                        | `agent.type`, `agent.handler` (both string)    |
| `supervise.restart`   | Event recorded on the active supervisor span  | `reason` (string)                              |
| `budget.exhausted`    | Event recorded on the failing agent's span    | `reason` (string)                              |

All spans use `SpanKind::Internal`. `agent.spawn`, `agent.ask`, and
`agent.handler` are duration spans (open + close); `agent.send` is a
point-in-time span (open + immediate close). `supervise.restart` and
`budget.exhausted` are emitted as events on the currently-active
span if one is set, otherwise as standalone single-shot spans.

Resource attributes (constant per process):

- `service.name = "mighty-runtime"`
- `service.version = <CARGO_PKG_VERSION>`

## User code — `agent_event`

User code inside an agent handler can attach structured events to the
active handler span by calling `agent_event`:

```rust
use mty_runtime::agent_event;

fn on_request(req: &Request) {
    agent_event(
        "http.request",
        &[
            ("method", req.method.as_str()),
            ("path", &req.path),
            ("status", "200"),
        ],
    );
}
```

Routing rules:

- Inside a handler dispatch — the event is attached to the
  `agent.handler` span as an OTel `Event` with the given attributes.
- Outside a handler (e.g. called from `main`, from a unit test, or
  from a background tokio task with no task-local context) — the
  event is written as a single JSON line to stdout. The line shape
  matches the slice-7 sink's `{"kind":"agent_event","name":"...",
  "fields":{"...":"..."}}` schema so existing log tooling picks it up.

The helper takes `&[(&str, &str)]` rather than a more elaborate
`KeyValue` slice so that the call site stays trivial in user code and
so the WIT-shaped equivalent (`wasi:logging@0.2.x`) can be lowered
1:1 in a future component-model integration.

## Privacy

Spans include message **names** (the protocol message variant — e.g.
`"Ping"`) but never message **bodies**. This matches the privacy
stance in `docs/internals/agent-features-roadmap.md`: a process with
the telemetry capability should see structure, not payload. Body
capture would be a separate, explicit opt-in capability (not shipped
in v0.16).

## Sampling

The default sampler is `AlwaysOn` (every trace is recorded). Set
`MTY_OTLP_SAMPLE_RATE=0.1` to record 10% of traces. The sampler is
trace-id-based, so all spans in a given trace are kept-or-dropped
together.

The slice-7 JSON-line emitter is unaffected by the sample rate; it
emits every event when its sink is enabled.

## Cost when disabled

When `MTY_OTLP_ENDPOINT` is unset:

- No tracer provider is built.
- `span_spawn`, `span_handler`, etc. return empty guards whose
  `Drop` is a single atomic-load check.
- `agent_event` from inside a handler skips the OTel path entirely;
  from outside a handler it does the stdout fallback (one `println!`).

There is no global mutex acquisition or allocation on the disabled
path. Benchmarks against the slice-7 sink's `Discard` baseline show
< 5 ns overhead per call site.

## Local testing with the OpenTelemetry Collector

The minimal collector config that prints every received span is:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  debug:
    verbosity: detailed

service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [debug]
```

Run with `otelcol --config=collector.yaml`, then:

```bash
MTY_OTLP_ENDPOINT=http://localhost:4317 mty run examples/07_agent_echo.sd
```

Each `?Ping` ask should produce four spans:

```
agent.spawn { agent.type = "Echoer" }
  └─ agent.ask { protocol.msg = "Ping" }
       └─ agent.handler { agent.type = "Echoer", agent.handler = "Ping" }
agent.send { protocol.msg = "Hit" }  (fire-and-forget messages)
```

(The parent-child relationship is established when callers run
inside a handler — `span_ask` reads the task-local handler context
when present.)

## Troubleshooting

| Symptom                                  | Likely cause                                                  |
|------------------------------------------|---------------------------------------------------------------|
| No spans arrive                          | `MTY_OTLP_ENDPOINT` not exported, or collector unreachable.   |
| `mighty: OTLP span exporter init failed` on stderr | Endpoint URL is malformed, or the configured protocol can't be built. |
| Spans arrive but no `agent.handler`      | The handler dispatched outside the runtime's instrumented loop (e.g. directly via `agent::run_one_turn`). |
| Spans arrive but no user events          | `agent_event` was called outside a handler — check stdout for the JSON-line fallback. |

## v0.17 follow-ups

The current shape leaves the following work for v0.17:

1. **Cross-agent correlation IDs.** Today an `ask` opens a child
   span only when the caller is already inside an instrumented
   handler. The receiver-side handler span is opened separately;
   the two are not yet linked via trace context. v0.17 will thread
   a `traceparent` field through the `MessageFrame` payload so
   replies show up under the caller's trace.

2. **Exemplars.** Once metrics ship (Tier 1.4), expose
   trace-exemplars on the budget-exhaustion + restart counters.

3. **Body-capture capability.** A separate opt-in capability that
   adds the (truncated) message body as an attribute. Off by
   default; lives behind its own env var so the privacy default
   stays restrictive.

4. **Per-component overrides.** `OTEL_RESOURCE_ATTRIBUTES` /
   `OTEL_SERVICE_NAME` standard env vars override
   `service.name` / `service.version`.

## See also

- `docs/internals/agent-features-roadmap.md` — Tier 1.2 + 1.3 plan
- `docs/internals/telemetry.md` — slice-7 JSON-line emitter shape
- `docs/internals/telemetry-otlp.md` — v0.3 OTLP event-emitter bridge
- `docs/internals/telemetry-spans.md` — internal architecture notes
- `dev/history/notes/TELEMETRY_SPANS_V0_16_NOTES.md` — what shipped + what deferred
