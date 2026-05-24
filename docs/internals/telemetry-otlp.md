# Telemetry — OTLP wire format (v0.3)

**Module:** `sdust_runtime::otlp` + `sdust_runtime::telemetry`
**Spec:** §25.1 telemetry emitter
**Closes:** amendment A38 (slice-7 was OTLP-flavoured JSON only)

## Overview

Slice 7 emitted line-delimited JSON shaped like OpenTelemetry
attributes; v0.3 adds a real OTLP wire-format exporter built on the
`opentelemetry-otlp` crate. The legacy JSON-line emitter remains as
a fallback so local development and the test suite continue working
without an OTLP collector running.

## Sink selection

`TelemetrySink::from_env()` chooses one of these sinks at runtime:

| Precedence | Env var(s)                              | Sink                       |
|-----------:|-----------------------------------------|----------------------------|
| 1          | `STARDUST_OTLP_ENDPOINT=<url>` (set)    | `TelemetrySink::Otlp`      |
| 2          | `STARDUST_TRACE=stderr`                 | `TelemetrySink::Stderr`    |
| 3          | `STARDUST_TRACE=file:/path/to/log`      | `TelemetrySink::File(p)`   |
| 4          | (none)                                  | `TelemetrySink::Discard`   |

If OTLP init fails (collector unreachable at startup), the runtime
silently falls through to the next sink and prints one diagnostic
line to stderr. **OTLP failure never aborts runtime construction.**

The `otlp` feature flag is on by default; build with
`cargo build -p mty-runtime --no-default-features` to strip the
exporter and its transitive deps (e.g. for minimum-binary builds).

## Semantic conventions

Mighty emits spans under the `mighty.*` namespace:

| Event            | Span name                   | Attributes                                    |
|------------------|-----------------------------|-----------------------------------------------|
| `TurnStart`      | `mighty.turn.start`       | `agent`, `msg`                                |
| `TurnEnd`        | `mighty.turn.end`         | `agent`, `msg`, `duration_us`                 |
| `Send`           | `mighty.send`             | `from`, `to`, `msg`                           |
| `Ask`            | `mighty.ask`              | `from`, `to`, `msg`, `deadline_ms` (optional) |
| `Reply`          | `mighty.reply`            | `from`, `msg`, `ok`                           |
| `Spawn`          | `mighty.spawn`            | `name`, `agent_id`                            |
| `Restart`        | `mighty.restart`          | `supervisor`, `child`, `attempt`              |
| `BudgetBreach`   | `mighty.budget_breach`    | `agent`, `kind` (SD5xxx)                      |
| `Shutdown`       | `mighty.shutdown`         | (none)                                        |

Every span carries the standard OTel resource attributes:

- `service.name = "mighty-runtime"`
- `service.version = <CARGO_PKG_VERSION>`

Spans are emitted with `SpanKind::Internal` and zero duration
(point-in-time events). A future revision may pair `TurnStart` and
`TurnEnd` into a single duration span.

## Wire transport

The exporter uses **gRPC over Tonic** (`grpc-tonic` feature of
`opentelemetry-otlp`). The endpoint URL passed via
`STARDUST_OTLP_ENDPOINT` should be the collector's gRPC port —
typically `http://localhost:4317` for an OTLP collector or
`http://otel-collector:4317` inside a docker-compose stack.

HTTP/protobuf transport is wired through the same `opentelemetry-otlp`
crate (`http-proto` feature is enabled) but is **not** yet selectable
via env var; flip the builder in `src/otlp.rs::OtlpHandle::try_init`
to use `.with_http()` instead of `.with_tonic()` for HTTP transport.

## Resource attribute customization

Today the resource attribute set is hardcoded to
`service.name = "mighty-runtime"` plus the crate version.
Per-application overrides (e.g. `service.namespace`,
`deployment.environment`) will land via the env vars
`OTEL_RESOURCE_ATTRIBUTES` and `OTEL_SERVICE_NAME` in v0.4.

## Local testing

The simplest collector to run for testing is the
[OpenTelemetry Collector](https://opentelemetry.io/docs/collector/)
in `otlp-exporter` mode pointing at a debug logger:

```yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

exporters:
  logging:
    loglevel: debug

service:
  pipelines:
    traces:
      receivers: [otlp]
      exporters: [logging]
```

Run with `otelcol --config=config.yaml`, then:

```bash
STARDUST_OTLP_ENDPOINT=http://localhost:4317 mty run examples/07_agent_echo.sd
```

Each `?Ping` ask should show up as four spans:
`mighty.ask`, `mighty.turn.start`, `mighty.turn.end`,
`mighty.reply`.

## Troubleshooting

- **No spans arrive at the collector**: confirm `STARDUST_OTLP_ENDPOINT`
  is exported (env, not just the shell), and that the runtime didn't
  fall through to the JSON fallback (stderr prints
  `mighty: OTLP exporter init failed: ...` when init fails).
- **Spans arrive but with empty attributes**: the resource attribute
  set is exposed only on the resource, not on each span — the
  collector's logging exporter at `loglevel: debug` displays both.
- **Build fails with linker errors involving tonic / rustls**: the
  default `grpc-tonic` feature pulls in TLS deps; on minimal targets
  build with `--no-default-features` to drop the OTLP exporter
  entirely.

## See also

- `docs/internals/telemetry.md` — slice-7 JSON-line emitter shape
- `docs/internals/runtime.md` — runtime cancellation arch
- `docs/spec/v0.1-amendments.md` — A70+ (v0.3 runtime amendments)
