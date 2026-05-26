# `mty replay`

Load a recorded runtime trace (`mty-trace-*.bin`) and either summarize
it, dump every event as JSON, step-replay it through an in-process
handler, or **byte-identically re-execute** the recorded program and
diff against the recorded event stream.

Shipped in v0.17. Tracking: Tier 1.4 of
[`docs/internals/agent-features-roadmap.md`](../../internals/agent-features-roadmap.md).
v0.19 added byte-identical full re-execution (`--byte-identical`).

## Usage

```
mty replay <trace.bin>
           [--dump-json] [--step] [--json]
           [--byte-identical --program <src.mty> [--mock-io]]
```

| Flag                  | Meaning                                                                       |
|-----------------------|-------------------------------------------------------------------------------|
| `<trace.bin>`         | Path to a trace file produced by setting `MTY_RECORD_TRACE` (see below).      |
| `--dump-json`         | Emit one JSON line per recorded event to stdout. Always works.                |
| `--step`              | Drive the events through an in-process step handler and print the totals.    |
| `--json`              | Emit the default summary as a JSON object instead of the pretty-printed text. |
| `--byte-identical`    | **v0.19.** Re-execute the recorded program from the trace and diff each emitted event against the recorded one. Requires `--program`. Exits non-zero on any mismatch. |
| `--mock-io`           | **v0.19.** IO/Clock/Random reads return recorded bytes instead of touching the live host. Default `true`; ensures deterministic replay across machines. |
| `--program <path>`    | **v0.19.** Path to the `.mty` source the trace was recorded against. Required with `--byte-identical` — the trace itself doesn't carry the source. |

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
totals. v0.17 ships the counting handler; v0.19 adds the full
`Runtime`-driven re-execution path behind `--byte-identical`.

### Byte-identical re-execution (`--byte-identical`, v0.19)

```sh
mty replay /tmp/bug-repro.bin \
    --byte-identical \
    --program services/echo.mty
```

```
=== Byte-identical replay (/tmp/bug-repro.bin) ===
  events recorded: 7
  events replayed: 7
  mismatches     : 0

byte-identical replay OK (7 event(s) matched)
```

On any divergence:

```
=== Byte-identical replay (/tmp/bug-repro.bin) ===
  events recorded: 7
  events replayed: 6
  mismatches     : 1

byte-identical replay FAILED: 1 mismatch(es) over 6 replayed event(s)
  #4: recorded message_handled did not match any replayed event after idx 4
```

The exit code is `0` on byte-identical success, `1` on any mismatch,
`2` when the required `--program` is missing.

#### Example workflow — capture a bug, replay deterministically

```sh
# 1. Capture a problematic run.
MTY_RECORD_TRACE=/tmp/bug-repro.bin mty run my-service.mty

# 2. Iterate the source until the replay diverges differently:
mty replay /tmp/bug-repro.bin \
    --byte-identical \
    --program my-service.mty

# 3. Once green, ship the fix + commit the trace as a regression oracle.
```

The default `--mock-io` ensures the replay never touches the live
filesystem, network, clock, or RNG — every IO/Clock/Random read
returns the recorded bytes. The replay is reproducible across Linux
/ macOS / Windows because there's no OS-specific behaviour in the
deterministic path (the v0.18 working-agreements rule).

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

The wire version is **2** as of v0.19. v1 traces continue to decode
(the `MessageSent.payload` `Vec<u8>` lifts into
`ReplayPayload::Opaque`). Future versions may *add* variants or
fields with serde defaults, but never rename or repurpose existing
ones. `mty replay` refuses to decode traces whose `version` is
greater than the binary supports.

### v0.19 — structural payloads (`ReplayPayload`)

The `MessageSent.payload` field is now a `ReplayPayload`:

```rust
enum ReplayPayload {
    Opaque(Vec<u8>),         // v0.18 hot path — Debug-formatted bytes
    Values(Vec<ReplayValue>) // v0.19 structural — byte-identical
}
```

`ReplayValue` is a structural mirror of the runtime's `Value`
enum (`Unit`/`Bool`/`Int`/`Float`/`Str`/`Char`/`Duration`/`Size`/
`Tuple`/`Array`/`Record`/`Variant`/`Opaque`). Live host references
(`Ref`/`Fn`/`Agent`/`Cap`) fold into `Opaque(String)` carrying
their Debug rendering.

## Privacy

`MessageSent.payload` + `IoRead.bytes` are captured verbatim — they
can carry secrets. Because recording is opt-in via `MTY_RECORD_TRACE`,
no user-data ever leaves the process unless the operator explicitly
chooses to write a trace.

For shared traces, redact at the source (in user code before the
recorder hook fires); v0.17 does not provide a built-in redactor.

## v0.20 / v1.0 plan

- **Step debugger** — `mty debug <trace.bin>` REPL with `step`,
  `peek <agent>`, `print msg`, breakpoints by handler name. Builds
  on `ReplayDriver::replay_all` (v0.19).
- **Strict byte-identical for legacy traces** — upgrade the v0.18
  hot path to emit `ReplayPayload::Values` so pre-v0.19 traces
  re-record losslessly.
- **Postcard codec** — swap the JSON-after-magic layout for postcard's
  compact varint encoding (gated behind a `replay-postcard` feature so
  deterministic-only builds stay slim).
- **Recording compression** — postcard + zstd framing for long-
  running production traces.

See `dev/history/notes/REPLAY_V0_17_NOTES.md`,
`dev/history/notes/REPLAY_HOTPATH_V0_18_NOTES.md`, and
`dev/history/notes/REPLAY_BYTE_IDENTICAL_V0_19_NOTES.md` for the
shipping notes.
