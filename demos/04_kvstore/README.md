# Demo 04 — `kvstore`

A sharded, supervised, in-memory key-value store written end-to-end
in Mighty. This is the "agents + protocols + supervisors" showcase:
in ~400 lines of `.mty` source we cover the whole story Mighty was
designed to tell.

## What it shows off

- **5 agents + 4 protocols** running concurrently
  - 3 `Shard` agents owning isolated slices of the key-space
  - 1 `Coordinator` agent routing by hash
  - 1 `Counter` (Metrics) agent accumulating per-op telemetry
  - 1 `Frontend` agent fronting an HTTP-shaped `Request` protocol
- **Sharding by hash**: a hand-rolled byte-rolling DJB2 in
  ~12 lines of Mighty source picks the right shard per key
- **Supervisor tree** declaration with `one_for_one` strategy +
  `restart up_to 3 in 30s` and `backoff 100ms..1s; restart`
  policies (spec §15)
- **Crash + survive demo**: a `Crash` message panics shard 1;
  the agent loop traps the panic + the surrounding system keeps
  serving the other shards. Once v0.12's supervisor wiring lands
  the same shape will trigger an automatic restart.
- **HTTP-shaped frontend**: the `Frontend` agent accepts
  `Request(method, path, body)` triples — the shape
  `std.http.serve` will deliver once the runtime hook is wired
  through the SIR interpreter
- **Stdlib + language features touched**: agents, protocols,
  `?Msg @deadline` ask, supervisor blocks, restart policies,
  match-on-string, mutable state on agents, parallel arrays,
  string slicing + bytes, `panic` capture, JSON shaping

## Layout

```
04_kvstore/
  mighty.toml         # package manifest (host profile, no deps)
  src/main.mty       # the whole demo (≈400 LOC of source)
  smoke.sh / smoke.ps1 # cross-platform behavioural test
  README.md          # this file
```

## Build + run

From the Mighty repo root:

```bash
# build the compiler (once)
cargo build -p mty-cli

# type-check
./target/debug/mty check demos/04_kvstore/src/main.mty

# run — drives PUT/GET/DELETE/CRASH + prints golden output
./target/debug/mty run   demos/04_kvstore/src/main.mty
```

PowerShell:

```powershell
cargo build -p mty-cli
.\target\debug\mty.exe check demos\04_kvstore\src\main.mty
.\target\debug\mty.exe run   demos\04_kvstore\src\main.mty
```

## Expected output

```
== boot ==
spawned: counter, 3 shards, coordinator, frontend
== put ==
{"shard":1,"k":"alpha","v":"1","ok":1}
{"shard":0,"k":"bravo","v":"2","ok":1}
{"shard":2,"k":"charlie","v":"3","ok":1}
{"shard":1,"k":"delta","v":"4","ok":1}
{"shard":0,"k":"echo","v":"5","ok":1}
{"shard":2,"k":"foxtrot","v":"6","ok":1}
== get ==
{"shard":1,"k":"alpha","hit":true,"v":"1"}
{"shard":0,"k":"bravo","hit":true,"v":"2"}
{"shard":2,"k":"charlie","hit":true,"v":"3"}
{"shard":1,"k":"delta","hit":true,"v":"4"}
{"shard":0,"k":"echo","hit":true,"v":"5"}
{"shard":2,"k":"foxtrot","hit":true,"v":"6"}
== miss ==
{"shard":2,"k":"ghost","hit":false}
== delete ==
{"shard":0,"k":"bravo","removed":1}
{"shard":0,"k":"bravo","hit":false}
== crash ==
panic: shard 1 crashed on purpose
{"crashed_shard":1,"status":"trapped"}
== post-crash get ==
{"shard":1,"k":"alpha","hit":true,"v":"1"}
{"shard":2,"k":"charlie","hit":true,"v":"3"}
{"shard":1,"k":"delta","hit":true,"v":"4"}
== stats ==
{"shards":[1,2,2],"metrics":{"puts":6,"gets":11,"dels":1,"misses":2,"crashes":1}}
== http front ==
{"PUT":{"shard":1,"k":"http_key","v":"http_val","ok":1},"GET":{"shard":1,"k":"http_key","hit":true,"v":"http_val"},"DELETE":{"shard":1,"k":"http_key","removed":1}}
```

## Architecture

```
                   ┌────────────────────────────────┐
                   │           Supervisor           │
                   │     KvTree (one_for_one)       │
                   └────────────────┬───────────────┘
                                    │ supervises
                ┌───────────────────┼───────────────────┐
                ▼                   ▼                   ▼
        ┌──────────────┐   ┌──────────────┐   ┌──────────────┐
        │   Shard #0   │   │   Shard #1   │   │   Shard #2   │
        │  keys/vals   │   │  keys/vals   │   │  keys/vals   │
        └──────────────┘   └──────────────┘   └──────────────┘
                ▲                   ▲                   ▲
                │   ?Put/Get/Del    │                   │
                └───────────────────┼───────────────────┘
                                    │
                          ┌─────────┴─────────┐
                          │    Coordinator    │
                          │  shard_of(key) %3 │
                          └─────────┬─────────┘
                                    │  ?Tick("PUT"|...)
                ┌───────────────────┼───────────────────┐
                ▼                                       ▼
       ┌──────────────┐                         ┌──────────────┐
       │   Frontend   │ ──── ?Request(...) ──▶  │   Counter    │
       │   /kv/<k>    │                         │  metrics     │
       │   /crash/<N> │                         └──────────────┘
       │   /stats     │
       └──────────────┘
```

## Querying the store (when the HTTP hook lands)

Once `std.http.serve` is wired through to the agent interpreter,
the Frontend agent will accept these HTTP shapes:

```
PUT    /kv/<key>          body: <value>   → JSON write-receipt
GET    /kv/<key>                          → JSON hit / miss
DELETE /kv/<key>                          → JSON delete-receipt
POST   /crash/<shard-id>                  → JSON trap-receipt
GET    /stats                             → JSON shard sizes + metrics
```

The same shapes are demonstrated by the `== http front ==` block at
the end of the demo's output, which feeds them through the same
underlying `route_*` helpers (see the file header in
`src/main.mty` for the v0.11 trip that requires this workaround).

## v0.11 caveats

This demo lives on top of the v0.11 alpha runtime. Three notable
trips are documented inline in `src/main.mty`:

1. **`std.http.serve` agent hook**: binds a real socket but does
   not yet pump requests into the agent loop. Demo drives the
   `Frontend` shape from `main()` instead, the same way demo 01
   exercises `Searcher`.

2. **Agent constructor parameters**: parse but are not threaded
   into agent state. We use a `SetShardId(I32)` init message per
   shard as the workaround.

3. **`panic(...)` inside an agent handler**: captured by the
   per-agent loop; the agent's mailbox stays alive. The demo's
   `crash + survive + serve` story relies on this. With v0.12's
   supervisor-restart wiring the same shape will instead trigger
   an automatic spawn + state-replay.

See [`dev/history/notes/DEMO04_V0_12_NOTES.md`](../../dev/history/notes/DEMO04_V0_12_NOTES.md)
for the full set of implementation notes + language gaps the demo
made visible.

## Smoke test

```bash
bash demos/04_kvstore/smoke.sh
```

```powershell
.\demos\04_kvstore\smoke.ps1
```

Asserts every JSON round-trip in the deterministic workload. Exits
non-zero with a captured-output dump on the first miss.
