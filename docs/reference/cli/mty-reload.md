# `mty reload`

State-preserving hot reload of a running agent.

```text
mty reload <agent-type> --from <new.wasm> [--deadline-ms 5000]
                                         [--sock <path>]
                                         [--json]
                                         [--dry-run]
```

**Status:** v0.20 introduces the CLI surface and the runtime-side
swap pipeline (`crates/mty-runtime/src/reload`). The control-socket
wire-up — i.e. the runtime listener for `op=reload` — lands in v0.21.
Until then `mty reload` against a live runtime reports the gap with
a clear error and exits non-zero. `--dry-run` exercises the
input-validation path end-to-end today.

## Synopsis

Drain the agent's current handler, snapshot its state via the
`Resumable` trait, swap to the new wasm module, decode the snapshot
into the freshly-loaded agent, and resume — all while the mailbox
continues to accept new messages.

## Arguments

| Flag | Description |
|------|-------------|
| `<agent-type>` | Registry name of the agent to reload (e.g. `ConnAgent`). |
| `--from <path>` | Path to the replacement wasm module. Must start with the wasm magic header (`\0asm`). |
| `--deadline-ms <ms>` | Maximum drain time before the swap fails with `MT5062` (default: 5000). |
| `--sock <path>` | Override `MTY_RUNTIME_CONTROL_SOCK`. |
| `--json` | Print the raw `ReloadReport` JSON instead of the pretty-printed table. |
| `--dry-run` | Validate the inputs without contacting the runtime. |

## Reload report

The runtime returns a `ReloadReport` describing the swap:

```json
{
  "agent_id": 7,
  "agent_type": "ConnAgent",
  "old_schema_hash": "0xdeadbeefcafef00d",
  "new_schema_hash": "0xdeadbeefcafef00d",
  "state_bytes_size": 128,
  "drain_elapsed_ms": 3,
  "total_elapsed_ms": 7
}
```

Pretty-printed:

```text
=== mty reload report ===
  agent          #7 ConnAgent
  schema hash    0xdeadbeefcafef00d  (old: 0xdeadbeefcafef00d)
  snapshot size  128 B
  drain elapsed  3 ms
  total elapsed  7 ms
```

## Failure modes

| Code | Condition | What to do |
|------|-----------|------------|
| `MT5060` | New module's `SCHEMA_HASH` is incompatible with the live agent's. | Bump the module's `Resumable` schema version, or write a `migrate_from` hook (v0.21). |
| `MT5061` | No live agent with that name in the registry. | Check `mty inspect` for the actual agent type, or spawn the agent first. |
| `MT5062` | The agent's in-flight handler didn't return inside `--deadline-ms`. | Increase the deadline, or restart the agent manually with `--force` (v0.21). |
| `MT5063` | Snapshot encode/decode failed. | Inspect the agent's `Resumable` impl — likely a serializer trait bound was unintentionally widened. |
| `MT5064` | The runtime doesn't yet support raw-wasm reloads (v0.20). | Use a `SameProgram` reload for state-only restarts; full wasm swap ships in v0.21. |
| `MT5069` | Internal runtime error during reload. | File a bug; include the report. |

## Examples

```bash
# Reload a connection-handling agent against a freshly-built wasm.
mty reload ConnAgent --from target/wasm32-wasi/release/conn_agent.wasm

# Give a slow handler more time to drain.
mty reload BatchWorker --from worker.wasm --deadline-ms 30000

# CI sanity check: parse the args + wasm header without a runtime.
mty reload ConnAgent --from conn.wasm --dry-run
```

## Wire shape

The CLI talks to `MTY_RUNTIME_CONTROL_SOCK` using the same
newline-delimited JSON contract `mty inspect` uses
(`docs/internals/agent-features-roadmap.md`, Tier 1.1). The request:

```json
{"op":"reload","agent_type":"ConnAgent","module_b64":"...","deadline_ms":5000}
```

The module bytes are base64-encoded so the request stays one line.
On Windows the same value maps to the runtime's local named pipe; use
the same `--sock` value that the runtime received through
`MTY_RUNTIME_CONTROL_SOCK`.

## See also

- `docs/internals/hot-reload.md` — architecture, schema-hash design,
  v0.21 follow-up (forward-compatible schema ranges + multi-version
  support during a rolling restart).
- `dev/history/notes/HOT_RELOAD_V0_20_NOTES.md` — design log.
