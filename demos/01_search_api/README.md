# Demo 01 — `search_api`

A minimal HTTP-shaped search service written in Stardust. Demonstrates:

> **v0.5 dogfood update.** `std.http.serve(addr)` now binds a real
> socket. The host dispatcher routes `std.http.serve` /
> `std.http.shutdown` through the new
> `sdust-stdlib::http_server` registry, which spins up a tokio
> runtime and a hyper accept loop. A default dispatcher returns
> `200 OK` JSON describing the request; the runtime's agent-binding
> hook (post-v0.5) will replace that with a real `?Request(req)`
> ask into the owning agent. See
> `crates/sdust-stdlib/tests/http_serve_real.rs` for the
> bound-socket roundtrip smoke test.

- `package` + `use std.*` imports
- A protocol with three messages and a backing agent
- Per-handler state mutation (running counters)
- Ask-style messaging (`agent?Msg(args)`)
- Hand-rolled JSON response shaping

## Layout

```
01_search_api/
  star.toml           # package manifest (host profile, no deps)
  src/main.sd         # the search service
  smoke.sh / smoke.ps1 # cross-platform behavioural test
  README.md           # this file
```

## Build / run

From the Stardust repo root:

```bash
# build the compiler (once)
cargo build -p sdust-cli

# type-check
./target/debug/sdust check demos/01_search_api/src/main.sd

# run — exercises every endpoint, prints golden output
./target/debug/sdust run   demos/01_search_api/src/main.sd
```

PowerShell:

```powershell
cargo build -p sdust-cli
.\target\debug\sdust.exe check demos\01_search_api\src\main.sd
.\target\debug\sdust.exe run   demos\01_search_api\src\main.sd
```

Expected output:

```
== health ==
{"status":"ok"}
== search ==
{"q":"stardust","hits":[]}
== search-2 ==
{"q":"agents","hits":[]}
== metrics ==
{"health":1,"search":2}
== 404 ==
{"error":"not found"}
```

## Smoke test

The `smoke.sh` (bash) and `smoke.ps1` (PowerShell) scripts spawn the
compiler in `run` mode and assert that each endpoint's expected line
appears in the captured output:

```bash
bash demos/01_search_api/smoke.sh
```

```powershell
pwsh demos\01_search_api\smoke.ps1
```

Either prints `01_search_api: PASS` on success.

## What this demo does NOT (yet) do

The Stardust standard library's `std.http.serve` is a real
`hyper`-backed Rust API (see `crates/sdust-stdlib/src/http.rs`), but
the v0.3 generic-call dispatcher in
`crates/sdust-stdlib/src/host.rs::dispatch` only routes `get` and `post`
calls today — there is no host shim that lets a Stardust agent be
*invoked* by an inbound HTTP request inside the SIR interpreter. The
spec calls this out under amendment A36 / A47.

That means: this demo **does not bind a TCP socket** when run under
`sdust run`. Instead, `main()` drives the handler bodies directly so
the demo's behaviour is observable on stdout (and exercisable by the
smoke scripts). The shape of the agent — protocol, state, handler
bodies — is exactly what would back a real `http.serve` once the bridge
ships.

Tracking item: see `DEMOS_V0_4_NOTES.md` in the repo root.

### A note on HTTPS

HTTPS over the `std.http` client is also a v0.3 follow-up; today only
`http://` URLs work (see `STDLIB_V0_2_NOTES.md`). The demo would
therefore serve plain HTTP behind a TLS-terminating proxy in
production.

## Optional: Docker

A simple multi-stage Dockerfile is provided. It builds the compiler in
a `rust:1` stage and copies both the compiler and the demo source into
a slim runtime image. Run with:

```bash
docker build -t stardust-search-api -f demos/01_search_api/Dockerfile .
docker run --rm stardust-search-api
```

The container's entrypoint runs `sdust run src/main.sd`, which prints
the same deterministic golden output the smoke script checks against.
