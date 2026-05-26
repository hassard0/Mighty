# v0.4+ demos

The external-user dogfood demos that satisfy spec §31.8's first two
clauses ("external users can install compiler; examples compile from
scratch"). Each demo has a runnable smoke script (bash + PowerShell)
and a step-by-step README in its own directory.

| Demo | Source | Smoke | What it shows |
|------|--------|-------|---------------|
| `01_search_api` | [`demos/01_search_api/`](../../demos/01_search_api/) | `bash demos/01_search_api/smoke.sh` | Protocol-shaped HTTP service: agent + per-handler state + JSON-shaped responses |
| `02_counter_web` | [`demos/02_counter_web/`](../../demos/02_counter_web/) | `bash demos/02_counter_web/smoke.sh` | Wasm Component Model output via `mty build --target wasm32-web`, browser-side host that consumes the `log` import |
| `03_extract_tool` | [`demos/03_extract_tool/`](../../demos/03_extract_tool/) | `bash demos/03_extract_tool/smoke.sh` | Top-level `sandbox` with budget + cap allow-lists, agent-driven token classifier |
| `04_kvstore` | [`demos/04_kvstore/`](../../demos/04_kvstore/) | `bash demos/04_kvstore/smoke.sh` | Sharded, supervised in-memory KV store: 5 agents, 4 protocols, supervisor tree with `one_for_one` + `restart up_to 3 in 30s`, hash-routed PUT/GET/DELETE, intentional crash + survive |

## Running all four

```bash
cargo build -p mty-cli
for d in demos/0*/; do bash "$d/smoke.sh"; done
```

Each prints `<demo>: PASS` on success. Total wall time on a debug
build is well under 30 seconds end-to-end.

## Demo 04 highlight — the "agents + supervisors" story

`04_kvstore` is the densest feature-coverage demo. In ~400 LOC of
`.mty` source it builds a sharded, supervised, in-memory KV store
and proves the crash-and-survive story end-to-end:

```
== crash ==
panic: shard 1 crashed on purpose
{"crashed_shard":1,"status":"trapped"}
== post-crash get ==
{"shard":1,"k":"alpha","hit":true,"v":"1"}
{"shard":2,"k":"charlie","hit":true,"v":"3"}
{"shard":1,"k":"delta","hit":true,"v":"4"}
```

The panicked shard's mailbox stays alive (slice-7 partial-restart
shape), the other shards keep serving, and the final `stats` shows
the per-shard sizes + the telemetry counter's running totals. See
the demo's [README](https://github.com/hassard0/Mighty/blob/main/demos/04_kvstore/README.md) for the
architecture diagram + the per-feature breakdown.

## v0.4 limitations the demos document

The detailed notes live in
[`DEMOS_V0_4_NOTES.md`](https://github.com/hassard0/Mighty/blob/main/DEMOS_V0_4_NOTES.md). At a glance:

- `std.http.serve` is a real Rust API but not yet generic-call
  dispatchable from agent code — Demo 01 exercises the handler bodies
  directly.
- `wasm32-web` wires `log` but not the DOM stubs in the emitted WIT —
  Demo 02's JS host parses the log stream to drive the visible UI.
- Top-level `sandbox` records cpu/wall/mem/path caps; runtime
  enforcement is wired but trips only when a capability-marked call
  yields back to the runtime — Demo 03 ships a `breach.sd` companion
  that will start failing (as expected) the moment enforcement
  catches up.
- Slice-6 interpreter string ops: `len`/`to_str`/`is_empty` work;
  `contains`/`find`/`char_at`/`slice` are permissive stubs.

None of these block the alpha gate — they're the v0.5/v0.6 backlog
items the demos make visible.

## See also

- [Tour](../tour/) — guided language walk-through
- [Getting started](../getting-started.md) — install + first program
- [Spec amendments](../spec/v0.1-amendments.md) — chronological record
  of every implementation decision, including the slice-8 / v0.3 +
  v0.4 entries the demos exercise
