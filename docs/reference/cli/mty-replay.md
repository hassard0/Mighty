# `mty replay`

Load a recorded runtime trace (`mty-trace-*.bin`) and either summarize
it, dump every event as JSON, or step-replay it through an in-process
handler.

Shipped in v0.17. Tracking: Tier 1.4 of
[`docs/internals/agent-features-roadmap.md`](../../internals/agent-features-roadmap.md).

## Usage

```
mty replay <trace.bin> [--dump-json] [--step] [--json]
```

| Flag           | Meaning                                                                       |
|----------------|-------------------------------------------------------------------------------|
| `<trace.bin>`  | Path to a trace file produced by setting `MTY_RECORD_TRACE` (see below).      |
| `--dump-json`  | Emit one JSON line per recorded event to stdout. Always works.                |
| `--step`       | Drive the events through an in-process step handler and print the totals.    |
| `--json`       | Emit the default summary as a JSON object instead of the pretty-printed text. |

The default behaviour (no flags) validates the trace + prints a
human-readable summary (wire version, runtime seed, event counts,
agent count, total handler elapsed microseconds).

## Recording a trace

Recording is opt-in. Set the `MTY_RECORD_TRACE` environment variable
to a writable path before running the Mighty program you want to
capture:

```sh
MTY_RECORD_TRACE=/tmp/run-1.bin mty run my-service.mty
```

When the runtime exits cleanly, the recorder serializes its buffer to
that path. The on-disk format begins with the 8-byte magic header
`MTYTRACE` so `mty replay` can reject unrelated binaries before
attempting decode.

If `MTY_RECORD_TRACE` is unset, the runtime never installs the
recorder and every recording call is a no-op — zero overhead.

## Inspecting a trace

### Default (summary)

```sh
mty replay /tmp/run-1.bin
```

```
=== Mighty replay trace (/tmp/run-1.bin) ===
  wire version : 1
  runtime seed : 42
  worker count : 4
  created at   : 2026-05-26T03:00:00Z
  events       : 9 (2 agent(s))
  breakdown    :
    spawns                 2
    messages sent          1
    messages handled       1
    io reads               1
    clock reads            1
    random reads           1
    budget exhausted       1
    exits                  1
  handler elapsed (sum) : 12345 us
```

### JSON dump (`--dump-json`)

```sh
mty replay /tmp/run-1.bin --dump-json | head -3
```

Each line is one JSON object with `index` + `event` fields:

```json
{"index":0,"event":{"Spawn":{"agent_id":1,"agent_type":"Echo","supervisor":null}}}
{"index":1,"event":{"MessageSent":{"from":0,"to":1,"msg":"Ping","payload":[]}}}
{"index":2,"event":{"MessageHandled":{"agent":1,"msg_idx":0,"msg":"Ping","elapsed_us":5}}}
```

### Step-replay (`--step`)

Drives every event through a counting step handler and prints the
totals. v0.17 ships the counting handler; the v0.18 follow-up wires
a real `Runtime`-driven step handler so user code re-executes.

## Wire format

The trace file is the postcard/JSON-encoded serialization of a
`TraceFile { version, created_at_ms, runtime_seed, worker_count,
events }` struct (see
`crates/mty-runtime/src/replay/wire.rs` for the canonical
definition). Variants are append-only:

| Variant            | Captures                                                |
|--------------------|---------------------------------------------------------|
| `Spawn`            | agent id + type name (+ supervisor parent if any)       |
| `MessageSent`      | sender + recipient + protocol-message name + payload    |
| `MessageHandled`   | per-agent monotonic `msg_idx` + handler elapsed (us)    |
| `IoRead`           | logical IO source label + bytes returned to user code   |
| `ClockRead`        | `std.time.now_ms` style read                            |
| `RandomRead`       | `std.random.fill` style read                            |
| `BudgetExhausted`  | agent + free-form reason string                         |
| `Exit`             | agent + free-form reason (`normal`, `trap:MT5020`, …)   |

The wire version is **1**. Future versions may *add* variants or
fields with serde defaults, but never rename or repurpose existing
ones. `mty replay` refuses to decode traces whose `version` is
greater than the binary supports.

## Privacy

`MessageSent.payload` + `IoRead.bytes` are captured verbatim — they
can carry secrets. Because recording is opt-in via `MTY_RECORD_TRACE`,
no user-data ever leaves the process unless the operator explicitly
chooses to write a trace.

For shared traces, redact at the source (in user code before the
recorder hook fires); v0.17 does not provide a built-in redactor.

## v0.18 plan

- **Step debugger** — `mty debug <trace.bin>` REPL with `step`,
  `peek <agent>`, `print msg`, breakpoints by handler name (Tier 2.2).
- **Runtime-driven step replay** — instead of the v0.17 counting
  handler, `--step` will re-drive the recorded `Runtime` and assert
  byte-identical handler outputs.
- **Postcard codec** — swap the JSON-after-magic layout for postcard's
  compact varint encoding (gated behind a `replay-postcard` feature so
  deterministic-only builds stay slim).

See `dev/history/notes/REPLAY_V0_17_NOTES.md` for the shipping notes.
