# v0.4 demos — implementation notes

Decisions, interpretation calls, and v0.4 limitations encountered
while building the three dogfood demos under
[`demos/`](demos/). Each entry below is a deliberate v0.4
stopgap — none are silent compromises.

## Demo 01 — `01_search_api`

### Decision: drive handlers from `main()` rather than `http.serve`

**Why.** `std.http.serve` ships as a real `hyper`-backed API in
`crates/sdust-stdlib/src/http.rs::serve`, but the v0.3 generic-call
dispatcher in `crates/sdust-stdlib/src/host.rs::dispatch` only routes
`std.http.get` and `std.http.post` today. The serve path also needs
an agent-side `Handler` adapter that lets the runtime invoke an
`AgentRef[T]` when a request lands; that bridge is post-v0.4.

**Stopgap.** `main()` calls the agent through the regular ask
operator (`s?Health()` etc.) so the handler bodies, protocol shape,
agent state mutation, and JSON-response formatting are all exercised
on every run. The same handler logic would back a future
`http.serve(addr, s)` call without touching the agent definition.

**v0.5 follow-up.** Wire `std.http.serve` through the
`host::dispatch` path (probably synthesising a `Handler` closure that
posts a synthetic ask into the agent's mailbox and waits for the
reply).

### Decision: hand-roll JSON responses

`std.json.encode` exists but takes an already-serialised string; the
parsed-tree binding is post-v0.3. The demo concatenates response
strings inline; both shapes round-trip through `std.json.parse`
deterministically.

## Demo 02 — `02_counter_web`

### Decision: parse imported `log` stream from JS instead of using DOM

**Why.** `crates/sdust-codegen-wasm/src/wit.rs` emits WIT stubs for
the `stardust:web/dom` interface (`get-element-by-id`, `set-text`)
but the slice-8 lowerer in `crates/sdust-codegen-wasm/src/emit.rs`
only wires the `log` import end-to-end — DOM call lowering is open
work. The `dom.set_text("#count", n.to_str())` calls in
`examples/20_frontend_component.sd` lower to extern stubs that aren't
present in the emitted core module.

**Stopgap.** The agent calls `log("count++")` (a deterministic tag);
the JS host in `web/index.html` parses those log lines and updates
the visible DOM counter. The Stardust agent is still the source of
truth for the count — the JS just renders it.

**v0.5 follow-up.** Bind the `stardust:web/dom` interface in
`emit.rs` parallel to the existing log path; switch the demo body to
`dom.set_text(...)` and drop the JS-side parsing.

### Decision: zero-dep loader (raw component → embedded core module)

**Why.** Browsers don't run Component Model components natively
today. The canonical path is `npx @bytecodealliance/jco transpile
main.wasm`, which generates ESM you can `import`. Requiring `npx`
breaks the "external user clones + builds + runs" alpha-gate flow.

**Stopgap.** The loader in `web/index.html` walks the component
bytes looking for the canonical core-wasm preamble
(`\0asm\x01\0\0\0`), extracts the embedded core module, and
instantiates that directly with a hand-provided `log` shim. The
README spells out the production pattern (`jco`) as well.

**v0.5 follow-up.** Bundle a prebuilt `jco transpile` output and a
script that drops the user into a known-good loader.

## Demo 03 — `03_extract_tool`

### Decision: drive the extractor by string equality, not tokenisation

**Why.** The slice-6 SIR interpreter's method table
(`crates/sdust-sir/src/interp/run.rs::eval_method`) only binds:

- `len`, `to_str` / `to_string` / `as_str`, `is_empty`
- `unwrap`, `unwrap_or`, `ok`, `ok_or`
- `ro` / `rw` / `path` / `host` (cap-narrowing pass-throughs)
- `get` / `query` (return `None`)
- `contains` / `starts_with` / `ends_with` (always return `false`)

`char_at`, `slice`, `find`, `chars` fall through to `Unit` and
`contains` returns a constant `false`. The probe `s.contains("Alice")`
on `s = "Alice met Bob"` returned `false` — confirming the stub.

**Stopgap.** Each token is sent as a separate `Classify(token)`
message; the agent compares the token against an inlined entity
vocabulary using `==` (which the interpreter handles correctly).
The shape of the agent — protocol, state, handler — is the real one.

**v0.5 follow-up.** Replace the stubs with real `String` impls
(probably folded into the v0.5 stdlib pass that ships
`String::contains` etc.). The demo body becomes the natural
char-by-char tokeniser.

### Decision: companion `breach.sd` with caps that *should* trip

**Why.** Per amendments A34 / A37 / A43, the v0.4 runtime *records*
sandbox + budget entries into a `BudgetTracker` but the cpu / wall /
memory checks only fire on the next capability-marked call. The
slice-6 interpreter is synchronous and has no auto-charging for
pure-compute loops (A37), so the impossible-cap sandbox in
`src/breach.sd` runs to completion today. When v0.5 wires
auto-charging through the SIR interpreter the breach will start
trapping with `MT5009 budget_exceeded`. The smoke script tolerates
both outcomes.

### Decision: fs.read fixture is consumed in-process

**Why.** `std.fs.read` (via `crates/sdust-stdlib/src/host.rs::fs_read`)
uses an unrestricted `FsCap::unrestricted()` — the path allow-list in
the sandbox header is recorded but not enforced against this bridge.
Loading `inputs/sample.json` through it wouldn't exercise the cap
check meaningfully; running the demo without fs at all keeps the
output deterministic and platform-independent.

**v0.5 follow-up.** When the cap-checked `Fs` handle is plumbed
end-to-end (it exists in `sdust-stdlib::fs::FsCap` but the
generic-call bridge bypasses it), the demo body switches to
`fs.read("./inputs/sample.json")` and the breach test gains a fs-out-
of-allowlist case.

## Cross-cutting

- **Smoke scripts ship in both bash and PowerShell.** The repo
  targets Windows / Linux / macOS so each demo carries `.sh` +
  `.ps1` companions. The PowerShell scripts use `pwsh`-friendly
  syntax (no `&&`-chains).
- **All demos use `host` profile**, not `core` — the latter would
  trigger MT4002 on `alloc` effects (A30 / A65.d) which the agent
  spawn machinery uses.
- **No demo modifies the workspace `Cargo.toml`** or any crate
  source. Per the v0.4 swarm-agent agreement, the demos are
  observers of the compiler/runtime as it stands at HEAD `69de965`.

## What blocks each demo from going further

| Demo | Next-most-valuable v0.5 feature |
|------|--------------------------------|
| 01 | `host::dispatch` route for `std.http.serve` + agent-side `Handler` adapter |
| 02 | `stardust:web/dom` import lowering in `sdust-codegen-wasm/src/emit.rs` |
| 03 | Auto-charging in the SIR interpreter so cpu/mem caps trip on pure-compute loops |

None of these block the alpha gate.
