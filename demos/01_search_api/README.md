# Demo 01 — `search_api`

A minimal HTTP-shaped search service written in Mighty. The
`Searcher` agent owns a per-handler counter and answers three
endpoints — `/health`, `/search`, `/metrics` — over an ask-style
protocol. The demo's deterministic output drives the smoke; the
shape is the same one `std.http.serve` routes to in production.

## What this demonstrates

The first "backend service in Mighty" forcing function. The
demo exercises:

| Surface | What this demo does |
|---|---|
| `package` + `use std.*` | Standard manifest-driven package layout. |
| `protocol` + typed message contract | Three messages (`Health`, `Search`, `Metrics`) on one protocol. |
| `agent` + per-handler state mutation | Running counters update on every dispatch. |
| `agent?Msg(args)` ask-style messaging | Each endpoint roundtrips through a typed mailbox ask. |
| Hand-rolled JSON response shaping | `format!()` builds the wire reply; no allocator needed at this scale. |

Brought to its current shape by **v0.5** (real
`std.http.serve` host bridge — `crates/mty-stdlib/src/http.rs`
binds a hyper accept loop today; demo runs the same handler
bodies via `mty run` for deterministic smoke output).

## Layout

```
01_search_api/
├── README.md
├── mighty.toml            # host-profile package, no deps
├── src/main.mty           # protocol + Searcher agent + driver
├── smoke.sh / smoke.ps1   # cross-platform behavioural test
└── Dockerfile             # optional multi-stage container
```

## Build / run

From the Mighty repo root:

```bash
cargo build -p mty-cli
./target/debug/mty check demos/01_search_api/src/main.mty
./target/debug/mty run   demos/01_search_api/src/main.mty
```

PowerShell:

```powershell
cargo build -p mty-cli
.\target\debug\mty.exe check demos\01_search_api\src\main.mty
.\target\debug\mty.exe run   demos\01_search_api\src\main.mty
```

## Expected output

```
== health ==
{"status":"ok"}
== search ==
{"q":"mighty","hits":[]}
== search-2 ==
{"q":"agents","hits":[]}
== metrics ==
{"health":1,"search":2}
== 404 ==
{"error":"not found"}
```

## Smoke test

```bash
bash demos/01_search_api/smoke.sh
```

```powershell
pwsh demos\01_search_api\smoke.ps1
```

Either prints `01_search_api: PASS` on success. The script spawns
the compiler in `run` mode and asserts every endpoint's expected
line appears in the captured output.

## How HTTP wiring works today

`std.http.serve` binds a real hyper-backed socket via
`crates/mty-stdlib/src/http.rs`; the v0.5 host dispatcher routes
the `serve` and `shutdown` calls through `mty-stdlib::http_server`.
The runtime's *agent-binding hook* — the path that lets an
`?Request(req)` ask reach an agent's mailbox from an inbound HTTP
frame — is still on the post-v0.30 roadmap, so the demo drives the
handler bodies directly inside `main()` rather than letting hyper
deliver them.

The shape of the agent — protocol, state, handler bodies — is
exactly what a wired-up `http.serve(addr)` will route to once the
bridge ships. When that lands, the demo replaces its inline driver
with `let _ = std.http.serve("0.0.0.0:8080", Searcher::new())` and
the smoke output is identical.

## Optional: Docker

A simple multi-stage Dockerfile is provided. It builds the compiler
in a `rust:1` stage and copies both the compiler and the demo
source into a slim runtime image. Run with:

```bash
docker build -t mighty-search-api -f demos/01_search_api/Dockerfile .
docker run --rm mighty-search-api
```

The container's entrypoint runs `mty run src/main.mty`, which
prints the same deterministic golden output the smoke script
checks against.
