# `mty inspect`

Live agent introspection for a running Mighty runtime. Connects to
the runtime's local control socket (opt-in via the
`MTY_RUNTIME_CONTROL_SOCK` environment variable) and prints a
snapshot of every live agent — name, mailbox depth, in-flight
handler, budget consumption, and (optionally) the last N message
names handled.

Shipped in v0.16. Tracking: Tier 1.1 of
[`docs/internals/agent-features-roadmap.md`](../../internals/agent-features-roadmap.md).

## Usage

```
mty inspect [--sock <PATH>] [--agent <ID>] [--json] [--watch <MS>]
```

| Flag             | Meaning                                                                                  |
|------------------|------------------------------------------------------------------------------------------|
| `--sock <PATH>`  | Override `MTY_RUNTIME_CONTROL_SOCK`. Required when the env var is unset.                 |
| `--agent <ID>`   | Return one agent's snapshot instead of the whole runtime.                                |
| `--json`         | Emit the raw JSON payload instead of the pretty-printed table.                           |
| `--watch <MS>`   | Poll every N milliseconds. Ctrl-C to exit.                                               |

## Enabling the control socket

The Mighty runtime listens on a control socket **only** when the
environment variable `MTY_RUNTIME_CONTROL_SOCK` is set at the time
the runtime starts. The value is a filesystem path:

```sh
# Server side: start a Mighty program with introspection enabled.
MTY_RUNTIME_CONTROL_SOCK=/tmp/mty.sock mty run my-service.mty
```

```sh
# Client side: connect to it from another shell.
mty inspect --sock /tmp/mty.sock
```

If the env var is unset, the runtime does not bind any socket and
`mty inspect` reports an error.

## Platform support

- **Unix** (Linux, macOS, *BSD): the path is a Unix-domain socket.
- **Windows**: the named-pipe backend is not yet implemented; the
  runtime logs a warning and `mty inspect` reports a stub error.
  Tracking: see `INTROSPECT_V0_16_NOTES.md` "Tier-1 followups".

## Example output

Default (pretty-printed table):

```
=== Mighty runtime (4 workers, snapshot at 2026-05-26T03:00:00Z) ===
    id  type                          mb   hi  state                  budget
     1  search::Worker                 0    3  handler:lookup +12ms   5.0KB 100ms/1.0s
     2  search::Worker                 0    2  idle                   1.2KB
     3  search::Supervisor             0    0  idle                   0B
```

`--json` emits the raw `RuntimeSnapshot` JSON (one object per line,
newline-delimited so it pipes nicely into `jq`):

```sh
mty inspect --sock /tmp/mty.sock --json | jq '.agents[].agent_type'
```

`--agent <ID>` returns one `AgentSnapshot` instead:

```sh
mty inspect --sock /tmp/mty.sock --agent 1
```

`--watch 1000` polls every second:

```sh
mty inspect --sock /tmp/mty.sock --watch 1000
```

## Wire shape

```
{"version":1,
 "worker_count":4,
 "timestamp_ms":1779580800000,
 "agents":[
   {"version":1,
    "agent_id":1,
    "agent_type":"search::Worker",
    "supervisor_parent":3,
    "mailbox_depth":0,
    "mailbox_high_water":3,
    "in_flight_handler":"lookup",
    "in_flight_elapsed_ms":12,
    "budget":{
      "mem_used_bytes":5120,
      "mem_limit_bytes":null,
      "ticks_used":100000000,
      "ticks_limit":1000000000,
      "deadline_ms":null
    },
    "last_messages":[]
   }
 ]
}
```

The wire-version field is locked at `1` for the v0.16 series.
Future versions may *add* fields, but never rename or repurpose
existing ones; clients should accept additive shapes silently.

## Security note

By default, the per-agent `last_messages` ring is **empty** — the
introspect surface exposes message *names* during the active handler
window but not message bodies. To capture bodies in the ring (eight
most-recent messages per agent), set the runtime-side environment
variable:

```sh
MTY_INSPECT_CAPTURE_BODIES=1 MTY_RUNTIME_CONTROL_SOCK=/tmp/mty.sock mty run ...
```

Anyone with read/connect access to the control-socket path can see
every captured body. In production, place the socket under a
directory with `0700` permissions (e.g.
`/run/mty/<service>/control.sock`).

## Related

- [`docs/internals/agent-features-roadmap.md`](../../internals/agent-features-roadmap.md) — Tier 1.1 spec
- [`dev/history/notes/INTROSPECT_V0_16_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/dev/history/notes/INTROSPECT_V0_16_NOTES.md) — implementation notes
- [`docs/internals/runtime.md`](../../internals/runtime.md) — runtime architecture
