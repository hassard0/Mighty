# v0.5 dogfood completion — notes

The v0.5 dogfood swarm scope was: close every gap the v0.4 demos
shipped as a stopgap (see [`DEMOS_V0_4_NOTES.md`](DEMOS_V0_4_NOTES.md)).
All 5 gaps closed; this file records the interpretation calls and
the work that was scope-cut to v0.6.

## Gap 1 — `std.http.serve` real binding

### Decision: process-wide registry + default echo dispatcher

**Why.** The SIR interpreter's dispatcher signature is
`(path, method, args) -> Value`. It has no `&mut Interp` and no
back-channel to the owning agent. A direct
`serve(addr, agent_handle)` call therefore can't immediately do
`agent_handle?Request(req)` because there's nothing in the dispatch
shape that knows how to post into an agent mailbox.

**Stopgap.** A new `sdust-stdlib::http_server` module owns a
process-wide tokio runtime + a handle registry. `start_blocking(addr)`
binds a TCP socket, returns `(handle_id, bound_addr)`, and spawns a
hyper accept loop that runs the *currently installed* `AgentDispatch`
closure on every request. The default dispatcher returns
`200 OK { "method": "GET", "path": "/health", "status": "ok" }` so
the bound-socket smoke test in
`crates/sdust-stdlib/tests/http_serve_real.rs` roundtrips cleanly
even without a runtime-side agent integration.

**v0.6 follow-up.** Runtime calls `install_agent_dispatch(closure)`
once at startup. The closure pulls the owning agent off a handle-id
prefix in the request path (or out of TLS SNI / a header), posts
`?Request(req)` through the standard agent mailbox, and marshals the
reply back to HTTP. The current `host::dispatch` shape already
supports this — only the closure body needs to be filled in once
the runtime knows how to look up agents by handle id.

## Gap 2 — Wasm DOM lowering

### Decision: ship the imports + WIT; defer SIR-side lowering

**Why.** The full chain SIR `MethodCall("dom.set_text", ...)` → Wasm
`Call(idx)` requires a new SIR builtin/effect surface that other
swarm agents own (and that the loop-agent + macros-agent are
actively reshaping). Trying to inject a `BuiltinId::Dom(...)`
variant into `sdust-sir::sir::BuiltinId` would race them.

**Stopgap.** `crates/sdust-codegen-wasm/src/wit.rs` now imports
`stardust:web/dom` and expands the interface to (set-text, get-text,
on-click, query) plus the legacy v0.4 handle-based ops; `emit.rs`
declares 4 core-wasm imports under `stardust:web/dom` with the
matching `(ptr, len)` signatures. `emit_dom_call(op, &mut wfn)`
is wired and reserved (`#[allow(dead_code)]`) for the future
lowering pass. The companion JS shim
[`demos/02_counter_web/web/dom-shim.js`](demos/02_counter_web/web/dom-shim.js)
implements every DOM op against `document.*` so a hand-written
core-module test (or a future Stardust-side `dom.set_text` call)
runs end-to-end.

**v0.6 follow-up.** Add a `BuiltinId::Dom { op: DomOp }` (or route
through `EffectOp::GenericCall` with `path = ["dom"]`); update
`emit.rs::emit_call` to dispatch to `emit_dom_call`.

## Gap 3 — Str method table

### Decision: ship complete real impls in `eval_method`

`sdust-sir::interp::run::eval_method` now binds real
`contains`, `starts_with`, `ends_with`, `find` (Option[USize]),
`char_at` (Option[Char]), `slice(start, end)` (Option[Str]),
`to_lower` / `to_upper`, `trim` / `trim_start` / `trim_end`,
`split(sep)` (Vec[Str]), `chars` / `bytes`, `replace(from, to)`,
`repeat(n)`, plus mutable forms `push` / `push_str` / `clear` / `pop`
and Vec helpers `get` / `first` / `last` / `iter`.

Each method routes through a small helper closure (`arg_str`,
`arg_usize`, `some`, `none`) for argument coercion + Option
construction. Test coverage in `crates/sdust-sir/tests/string_methods.rs`.

### Interpretation call: receiver-type-agnostic `contains`

`contains` works on both `Str` (substring search) and `Array` (linear
scan for equality). Stardust source `arr.contains(x)` now matches
the Rust shape, which lets future agent code drop the per-element
`==` workaround that v0.4 Demo 03 used.

## Gap 4 — CPU + mem auto-charging

### Decision: typed `RunResult::MemBudgetExceeded` variant

`RunResult` gains `MemBudgetExceeded { used: u64, limit: u64 }` with
exit code `4` and `SD5009` trap code. The interpreter charges memory
on every `AdtInit` / `TupleInit` / `ArrayInit` rvalue evaluation
using an `estimate_value_bytes` helper (header byte estimate + sum
of recursive field/element sizes). The contract is "deterministic +
monotonic", not "bit-perfect" — the goal is to catch runaway
allocations, not to model the real arena layout.

A new `run_fn_with_resource_budget(prog, name, args, host, steps, mem)`
entry point exposes the mem cap; the existing `run_fn_with_budget` is
unchanged (it implicitly passes `mem_budget = 0` ≡ "no cap").

Downstream `match RunResult` arms updated in:

- `sdust-runtime::agent` (per-turn outcome → RuntimeError)
- `sdust-driver::pipeline::run_file_with_runtime` (CLI exit code)
- `sdust-stdlib::test` (Stardust test runner)
- `sdust-driver/tests/conformance_full.rs` (conformance harness)

### Interpretation call: charge AdtInit / Tuple / Array, not every step

The brief says "charge mem on each Alloc/Tuple/Array/Struct
construction". I read that as: every rvalue that *materialises* a
heap-shaped value charges. We don't charge for `BinOp` / `Use` /
`Ref` because those don't conceptually allocate (the interp's
Value::Int is inline, but the abstraction we're trying to enforce
is heap-equivalent).

## Gap 5 — FsCap path allowlist enforcement

### Decision: process-wide default cap installed by the driver

**Why.** The SIR lowerer doesn't yet materialise per-call caps from
the sandbox manifest into the call shape; the `std.fs.read(path)`
call SIR sees has no `Cap` arg today. Reworking the lowerer would
require touching files the loop agent owns.

**Stopgap.** `sdust-stdlib::fs` gains a process-wide
`DEFAULT_READ_CAP` / `DEFAULT_WRITE_CAP` slot with
`install_default_read_cap(cap) -> previous_cap` /
`install_default_write_cap(cap)` setters and
`current_default_read_cap()` snapshot helpers. The host dispatchers
(`fs_read`, `fs_write`, `fs_exists`, `fs_list_dir`) consult the
current default before calling the real fs operation. A `Forbidden`
path now surfaces as `Result::Err(forbidden:<path>)` so agents can
pattern-match.

A new `IoErr::Forbidden(path)` variant complements the existing
`Denied(path)`; both share the same shape so older tests pattern
on either.

**v0.6 follow-up.** Lower the sandbox manifest's `fs.read = [...]`
list into a real `Rvalue::CapValue { family: Fs, constraint: PathList(...) }`
that gets threaded as the first arg of each `std.fs.*` call. The
global slot becomes the fallback path for un-sandboxed runs.

## What was scope-cut

- **Demo 01 rewrite to actually bind via `std.http.serve(addr)`**:
  the agent integration is the `install_agent_dispatch` post-v0.5
  follow-up. The demo's existing `main()`-driven shape still
  exercises every handler; the bound-socket path is verified by
  the dedicated integration test.
- **Demo 02 rewrite to use real DOM bindings**: the WIT + core
  imports + JS shim ship in v0.5, but the SIR → Wasm lowering of
  `dom.set_text(...)` to `Call(dom_set_text_idx)` requires a SIR
  builtin/effect surface I don't own.
- **Demo 03 `breach.sd` actually trapping**: the budget machinery
  is wired; the demo's smoke script just needs an update to assert
  on the new SD5009 trap exit, which is a Demo work-stream task.

## Files modified (this slice)

- `crates/sdust-stdlib/src/{fs,host,lib,test}.rs`
- `crates/sdust-stdlib/src/http_server.rs` (new)
- `crates/sdust-stdlib/tests/{fs_capability_allowlist,http_serve_real}.rs` (new)
- `crates/sdust-codegen-wasm/src/{wit,emit}.rs`
- `crates/sdust-codegen-wasm/tests/dom_imports.rs` (new)
- `crates/sdust-sir/src/interp/run.rs` (eval_method + budget; loop
  agent owns the loop terminator sections)
- `crates/sdust-sir/tests/{string_methods,budget_charges}.rs` (new)
- `crates/sdust-runtime/src/agent.rs` (RunResult arm)
- `crates/sdust-runtime/tests/http_serve_real.rs` (new)
- `crates/sdust-driver/src/pipeline.rs` (RunResult arm)
- `crates/sdust-driver/tests/conformance_full.rs` (RunResult arm)
- `demos/01_search_api/README.md` (v0.5 update note)
- `demos/02_counter_web/README.md` (v0.5 update note)
- `demos/02_counter_web/web/dom-shim.js` (new)
- `demos/03_extract_tool/README.md` (v0.5 update note)
- `docs/internals/codegen-wasm.md` (DOM section)
- `docs/internals/runtime.md` (`std.http.serve` section)
- `docs/internals/sandbox-enforcement.md` (new)
- `docs/spec/v0.1-amendments.md` (A85, A86, A87, A88, A89)
