# Mighty Demos

Eleven end-to-end demos that exercise the Mighty compiler + runtime
the way an external adopter would. Each demo lives in its own
directory with a `mighty.toml`, source under `src/`, a smoke script
(bash + PowerShell where it makes sense), and a per-demo `README.md`.

The demos are the **forcing function** for every Mighty release —
each maps to a specific milestone, and each shipped only after the
underlying compiler / runtime surface compiled the source end-to-end.
The order roughly tracks Mighty's history: backend → frontend →
sandboxes → stateful → web → agents → swarms → distributed.

## The matrix

| # | Demo | Theme | Featured surfaces |
|---|------|-------|-------------------|
| 01 | [`01_search_api`](01_search_api/) | HTTP backend service | protocols + agents + per-handler state |
| 02 | [`02_counter_web`](02_counter_web/) | Browser counter | `mty build --target wasm32-web` + Component Model |
| 03 | [`03_extract_tool`](03_extract_tool/) | CLI extractor | `sandbox` block + capability allow-lists |
| 04 | [`04_kvstore`](04_kvstore/) | Sharded KV store | 5 agents + supervisor tree + crash-recovery |
| 05 | [`05_notetris_web`](05_notetris_web/) | Browser game (log-driven) | `mty:web/log` import + JS host parsing |
| 06 | [`06_canvas_game`](06_canvas_game/) | Browser game (canvas-direct) | `mty:web/canvas@0.1` WIT bindings + RAF callbacks |
| 07 | [`07_research_agent`](07_research_agent/) | LLM research agent | `std.llm` + `std.memory` + `@tool` decorator |
| 08 | [`08_swarm_review`](08_swarm_review/) | Multi-LLM code review | `std.swarm` consensus + capability-typed tools |
| 09 | [`09_distributed_swarm`](09_distributed_swarm/) | Cross-node swarm | cluster mesh + typed bang-send returns + `std.eval` |
| 10 | [`10_vision_rag`](10_vision_rag/) | Multi-modal RAG agent | `std.rag.Index` + `std.rag.Rag` + `std.llm.Image` |
| 11 | [`11_ffi_winit_stub`](11_ffi_winit_stub/) | FFI scaffold | `[[extern_lib]]` static-lib linking + `extern c { ... }` |

Demos 5–8 are the agent-heavy ones — they exercise the
**v0.27–v0.30 marketing claims** (capability-typed tools,
`std.swarm`, `std.eval`, taint types, computer use). Demo 09
demonstrates the v0.29 cluster surface; the v0.30 differentiators
(`Tainted[T]`, `std.observe`, `std.computer`) ship as canonical
examples in [`examples/`](../examples/) (33–36). Demo 10 covers the
v0.33 `std.rag` + multi-modal pipeline; demo 11 is the v0.36 Track T2
FFI scaffold (`[[extern_lib]]` + `extern c {}`).

## How each demo lays out

```
demos/<NN>_<name>/
├── README.md          # what it does, what surfaces it exercises, how to run
├── mighty.toml        # package manifest
├── src/               # `.mty` source
├── smoke.sh           # bash smoke test (golden output / artefact shape)
├── smoke.ps1          # PowerShell equivalent (where applicable)
├── web/               # browser-side host glue (demos 02, 05, 06 only)
└── tools/             # mock LLM + sample inputs (demos 07, 08 only)
```

The smoke script for each demo is the same kind of test the per-PR
CI runs — `bash smoke.sh` (or `pwsh smoke.ps1`) exits 0 on the
expected outcome and prints `<demo>: PASS` on success.

## Run every demo

```bash
cargo build -p mty-cli
for d in demos/0*/; do
  bash "$d/smoke.sh" || { echo "$d FAILED"; exit 1; }
done
```

PowerShell:

```powershell
cargo build -p mty-cli
Get-ChildItem demos\0*\ -Directory | ForEach-Object {
    $smoke = Join-Path $_.FullName "smoke.ps1"
    if (Test-Path $smoke) {
        pwsh $smoke
        if ($LASTEXITCODE -ne 0) { throw "$($_.Name) FAILED" }
    }
}
```

All eleven print `<demo>: PASS` when the run succeeds.

## Opt-in extras

A few demos carry expensive end-to-end stages behind an env var so
the default smoke stays fast:

| Env var | Demo(s) | What it enables |
|---|---|---|
| `MTY_AGENT_SMOKE=1` | 07, 08 | Spins up a stdlib-Python mock LLM server, points the demo's `AnthropicClient` at it, asserts the full pipeline markers fire. No real API tokens spent. |
| `MTY_CLUSTER_SMOKE=1` | 09 | Spawns a two-process cluster (node-a + node-b), exercises the cross-node bang-send, asserts the hop verification. No API key required. |
| `MTY_WEB_SMOKE=1` | 02, 05, 06 | Drives the wasm32-web artefact through a headless-browser perceptual-hash smoke (Chromium via `headless_shell`). |

Every opt-in is documented in the demo's own `README.md`.

## What each demo proves

The demos exist to **prove the marketing claims compile**. The
mapping from claim to demo:

| Claim | Proven by |
|---|---|
| "Agents are first-class" | Demos 01, 04 — multi-agent supervised systems running end-to-end |
| "Web is a target, not a port" | Demos 02, 05, 06 — Mighty source compiles to a Component-Model wasm and runs in a browser |
| "Sandboxes are capability-typed" | Demo 03 — top-level `sandbox` block with `cpu` / `wall` / `mem` / `mailbox` caps |
| "Tools are capability-typed" | Demos 07, 08 — `@tool(cap: fs.read)` enforced by the runtime, not the prompt |
| "Multi-LLM consensus is stdlib" | Demo 08 — `std.swarm` votes Majority across 3 providers under a $0.50 shared budget |
| "Cluster is in-language" | Demo 09 — cross-node `?Review(snippet)` over framed CBOR + mTLS |

When a demo's pitch line drifts away from its README's claims, the
release that ships the drift updates the demo first.

## Per-demo release history

Each demo's README has a "What this demonstrates" section pointing
at the specific Mighty version that brought the demo to its current
shape. The short version:

| Demo | Brought-to-shape by |
|---|---|
| 01 | v0.5 (HTTP host bridge) |
| 02 | v0.5 (DOM imports through WIT) |
| 03 | v0.5 (string methods + budget enforcement) |
| 04 | v0.12 (supervisor restart wiring) |
| 05 | v0.22 (`mty:web/canvas@0.1` WIT stubs) |
| 06 | v0.25 (canvas-direct emit path + agent array fields) |
| 07 | v0.26 (std.llm + std.memory + @tool) |
| 08 | v0.27 (std.swarm + handler-safe ADTs + `mty run -- argv`) |
| 09 | v0.29 (BuiltinId::Swarm + typed bang-send + cluster routing) |
| 10 | v0.33 (`std.rag` + `std.llm.Image` multi-modal pipeline)     |
| 11 | v0.36 T2 (`[[extern_lib]]` + `extern c {}` static-lib linking) |

The v0.30 differentiator surface (`Tainted[T]`, `std.observe`,
`std.computer`, `mty test --eval`) ships as canonical examples
33–36 rather than as a full demo — those surfaces compose more
naturally as focused vignettes than as full apps.

## See also

- [`examples/README.md`](../examples/README.md) — 36 canonical
  one-file examples, one per language / stdlib feature.
- [`bench/swe/README.md`](../bench/swe/README.md) — SWE-bench
  Verified harness.
- [`docs/getting-started.md`](../docs/getting-started.md) — your
  first agent in five minutes.
