# Telemetry (slice 7)

**Module:** `sdust_runtime::telemetry`
**Spec:** §25.1 (`telemetry emitter`)

Slice 7 emits structured logs as one JSON object per event line. The
schema is OpenTelemetry-flavoured but not strict OTLP (per A38).
Strict OTLP wire format ships with the slice-8 codegen.

## Event kinds

```rust
pub enum TelemetryEvent {
    TurnStart   { agent, msg },
    TurnEnd     { agent, msg, duration_us },
    Send        { from, to, msg },
    Ask         { from, to, msg, deadline_ms },
    Reply       { from, msg, ok },
    Spawn       { name, agent_id },
    Restart     { supervisor, child, attempt },
    BudgetBreach { agent, kind },
    Shutdown,
}
```

Every event carries `ts` (ms since UNIX epoch) and `kind` fields;
additional fields vary per kind.

## Example output

```json
{"ts":1716552123000,"kind":"spawn","name":"Echoer","agent_id":0}
{"ts":1716552123001,"kind":"ask","from":"(extern)","to":"Echoer","msg":"Ping","deadline_ms":null}
{"ts":1716552123002,"kind":"turn_start","agent":"Echoer","msg":"Ping"}
{"ts":1716552123003,"kind":"turn_end","agent":"Echoer","msg":"Ping","duration_us":127}
{"ts":1716552123003,"kind":"reply","from":"Echoer","msg":"Ping","ok":true}
{"ts":1716552123004,"kind":"shutdown"}
```

## Sinks

```rust
pub enum TelemetrySink {
    Discard,
    Stderr,
    File(PathBuf),
    Buffer(Arc<Mutex<Vec<String>>>),
}
```

Selected via `TelemetrySink::from_env()`:

| `MTY_TRACE` value | Sink |
|-------------------|------|
| (unset) or anything not below | `Discard` |
| `stderr` | `Stderr` |
| `file:/path/to/log` | `File(/path/to/log)` |

The legacy `STARDUST_TRACE` spelling is honoured when `MTY_TRACE`
is unset (with a one-shot deprecation warning).

`TelemetrySink::buffer()` returns a buffered sink + a shared `Vec<String>`
the test can inspect — used by integration tests that want to assert
on emitted events.

## String escaping

JSON strings escape `\` and `"`. The emitter is minimal — no nested
objects, no Unicode escapes beyond what `serde_json` would do (slice
7 doesn't depend on serde_json to keep build times down).

## Tests

`crates/mty-runtime/src/telemetry.rs` (inline `#[cfg(test)] mod tests`):

1. JSON shapes for representative events.
2. `BufferSink` captures emitted lines in order.
3. Quote escaping handles `"` and `\` correctly.

## See also

- `docs/internals/runtime.md`
- `docs/spec/v0.1-amendments.md` — A38
