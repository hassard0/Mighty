# Demo 04 — `kvstore`

A sharded, supervised, in-memory key-value store written end-to-end
in Mighty. This is the **"agents + protocols + supervisors"
showcase**: in ~400 lines of `.mty` source we cover the whole story
Mighty was designed to tell.

## What this demonstrates

| Surface | What this demo does |
|---|---|
| Multi-agent system | 5 agents (3 `Shard` + `Coordinator` + `Counter` + `Frontend`) running concurrently. |
| Protocols + typed mailboxes | 4 protocols carrying `Put`/`Get`/`Delete`/`Crash`/`Tick`/`Request` between agents. |
| Sharding by hash | Hand-rolled byte-rolling DJB2 in ~12 lines of Mighty source picks the right shard per key. |
| Supervisor tree | `supervisor KvTree { strategy one_for_one; restart up_to 3 in 30s; backoff 100ms..1s; restart }` (spec §15). |
| Crash + survive + restart | `Crash` message panics shard 1; the supervisor traps + automatically restarts the agent. |
| `?Msg @deadline` ask | Every routed call carries a deadline; deadline-exceeded surfaces as `Result::Err`. |
| `panic` capture inside an agent | Per-agent loop traps the panic; mailbox stays alive. |
| HTTP-shaped frontend | `Frontend` agent accepts `Request(method, path, body)` triples — the shape `std.http.serve` delivers in production. |

Brought to its current shape by **v0.12** (supervisor restart
wiring: `KvTree` is now a real supervisor that observes shard
crashes and respawns; pre-v0.12 the demo's crash story relied on
in-loop panic capture only).

## Layout

```
04_kvstore/
├── README.md
├── mighty.toml            # host-profile package, no deps
├── src/main.mty           # the whole demo (~400 LOC of source)
└── smoke.sh / smoke.ps1   # cross-platform behavioural test
```

## Build + run

From the Mighty repo root:

```bash
cargo build -p mty-cli
./target/debug/mty check demos/04_kvstore/src/main.mty
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
underlying `route_*` helpers (the file header in `src/main.mty`
documents the bridge gap).

## Smoke test

```bash
bash demos/04_kvstore/smoke.sh
```

```powershell
.\demos\04_kvstore\smoke.ps1
```

Asserts every JSON round-trip in the deterministic workload. Exits
non-zero with a captured-output dump on the first miss.
