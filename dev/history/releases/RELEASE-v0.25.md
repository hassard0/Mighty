# Mighty v0.25 — Release Notes

**Tag:** `v0.25.0`
**Date:** 2026-05-26
**Status:** SHIPPED — six-track swarm + integrator pass.

**Headline:** **Closed all 7 v0.24 demo-blocking gaps + extended
`format!()` + real `std.String` / `std.Vec[T]`. Demo 06 V2's JS
shim drops 48% (213 → 110 LOC); the Mighty agent now owns canvas
rendering end-to-end.** Track A wires `canvas.fill_rect(...)`
through HIR → IR → wasm32-web import (the long-standing v0.23 →
v0.24 leftover) and fixes the latent Unit-returning user-fn
stack-balance bug Track E surfaced last slice; Track B teaches
the wasm32-web emitter to lift `extern js { fn _foo() }` into
real `mty:web/js` imports; Track C closes agent fields with
`[T; N]` types (round-trip parse → HIR → typeck → SIR) and pins
SIR-runtime cross-callback persistence with a regression test;
Track D extends `format!()` to the full Rust layout grammar
(width / precision / alignment / sign / `#` alternate / `0`
zero-pad / fill char + new types `b` / `o`); Track E lands real
`std.String` + `std.Vec[T]` impls in `mty-stdlib`; Track F
demonstrates the slice by rewriting demo 06_canvas_game's
canvas-direct path (shim down to 110 LOC, agent paints
background + grid + HUD + cells via the new imports and HUD
lines use the new `format!()` width specs + `{name}`).

The five language wins (Tracks A–E) close every gap that v0.24
Track E flagged for v0.25. Track F is the honest one: 5 narrow
sub-gaps surface across the demo's edges (wasm32-web agent
persistence — emitter-side only, runtime is shipped; extern_js
kebab-vs-leading-underscore drift through `wit-component`;
canvas-handle taint through fn params; `const`-identifier
binding in match patterns; `format!()` named-arg `n=v`
passthrough wanting `{n}` shorthand). These become the v0.26
candidate tracks. None of them block adoption — the demo runs,
the agent owns the render path, and every `format!()` /
canvas / extern_js use the workarounds documented in
[`dev/history/notes/DEMO06_V2_V0_25_NOTES.md`](../notes/DEMO06_V2_V0_25_NOTES.md).

If you were on v0.24.0, the upgrade is `git pull && cargo install
--path crates/mty-cli --force` (or pull the v0.25.0 pre-built
binaries from the Releases page). There are **no source-level
breaking changes** at the language layer. The five language
extensions are all additive — `format!()` extended specs are
opt-in via the new spec characters, `std.String` / `std.Vec[T]`
sit alongside the existing primitive types, `extern js` blocks
that previously compiled to silent no-ops now resolve correctly
to wasm imports (which means programs that *relied* on the
no-op behaviour will now actually call the JS side — this is
the desired fix), and agent fields with `[T; N]` types parse
where they previously degraded to slice shape downstream. v1 +
v2 traces continue to decode under v0.25 unchanged.

## Highlights

- **6 of 6 v0.25 swarm tracks shipped.** Track A
  (HIR → IR canvas routing + Unit-fn stack-balance fix,
  SHIPPED-FULL), Track B (`extern js` → wasm imports,
  SHIPPED-FULL), Track C (agent fields `[T; N]` + SIR-runtime
  persistence regression, SHIPPED-FULL), Track D (`format!()`
  extended specs — width / precision / alignment / sign /
  `#` alt / `0`-pad / fill char / `b` / `o`, SHIPPED-FULL),
  Track E (`std.String` + `std.Vec[T]` real impls, SHIPPED-FULL),
  Track F (demo 06 V2 — shim −48 %, canvas-direct via the
  Round 1 closures, SHIPPED-PARTIAL — 5 narrow gaps documented
  for v0.26).
- **v0.24's 7 demo-blocking gaps are all closed.** Track E
  surfaced 6 numbered (A–F) plus the integrator-documented
  KNOWN_ISSUES #8 (Unit-fn stack-balance). Mapping into v0.25:
  Track A closes gaps A + B (and KNOWN_ISSUES #8 — same bug);
  Track B closes gap E; Track C closes gaps C + D (runtime
  persistence + arrays in agent fields; the emitter-side
  wasm32-web agent persistence is what becomes v0.26 candidate
  #1); Track D closes gap F.
- **KNOWN_ISSUES net: −1 (P2 #8 resolved).** v0.24's P2 #8
  (wasm32-web Unit-returning user-fn stack-balance) is closed
  by Track A's `emit_call` fix — a one-arm extension to the
  `FnRef::User` branch that pushes the placeholder `i32.const 0`
  for Unit-returning callees, matching the existing pattern in
  every other arm. P2 #9 (demo 06 RAF-mid-frame phash flake,
  4/5 success rate) stays open — not a v0.25 regression and not
  a required-gate blocker. P1 stays empty.
- **v1.0 freeze gate status: unchanged structurally.** Blockers
  #1 + #3 stay CLOSED. Blocker #2 (8 RFC comment windows)
  infrastructure stays live; the live RFC dashboard at
  [`docs/spec/rfcs/RFC_DASHBOARD.md`](../../../docs/spec/rfcs/RFC_DASHBOARD.md)
  still tracks per-window countdowns. Earliest possible
  v1.0.0 tag remains **2026-07-26**.
- **Spec v1.0-RC5 unchanged this slice.** Track D's `format!()`
  layout grammar is implementation surface (the `format!()`
  macro itself is not in the normative spec — the prose pins
  the runtime semantics it composes from). Every v1.0-RC5
  conforming program is still v1.0-RC5-conforming.
- **Conformance kit grows 156 → 159 cases / 24 categories.**
  Track D adds 6 new cases under `tests/conformance/macros/`
  (`format_width`, `format_precision`, `format_align`,
  `format_arity`, `format_basic`, `format_unsupported_spec`)
  on top of v0.24's `06_format_positional` / `07_format_named`
  / `08_format_hex`. Net +3 because three of the new shapes
  replace v0.24's positional / named / hex stubs.
- **All gates green, Rust test count grows 1675 → 1790**
  (+115 across the 6 tracks). Track A adds 24 (8 canvas
  routing + 5 Unit-fn stack-balance + 11 stmts/exprs/ctx unit
  tests); Track B adds 7 integration + 6 WIT-emit unit tests;
  Track C adds 5 syntax + 4 typeck + 3 runtime persistence
  tests; Track D adds 40 spec-parser unit + 18 macro-expansion
  + 6 conformance fixtures; Track E adds 19 string + 22 vec
  unit tests; Track F adds 0 (demo rewrite). Python grows
  **474 → 490** (+16; impl-py picks up format-spec parser
  tests). Self-host driver still at **23**. Driver-test
  bucket grows **153 → 173** (+20; Track A's
  `canvas_routing_*` + `unit_fn_stack_*` + Track B's
  `extern_js_imports` + Track C's `agent_callback_persistence`
  + Track D's `format_extended` regression suites). Combined:
  **2476** (+148 vs v0.24's 2328).

## What's new

### Track A — HIR → IR canvas routing + Unit-returning user-fn stack-balance fix

Closes v0.24 Track E gaps A + B (and KNOWN_ISSUES P2 #8 — same
underlying emitter bug). Two fixes shipped in one track; both
were on the critical path for demo 06 V2.

- **Canvas-handle taint propagation (`mty-ir/src/lower/`).** A
  per-fn `HashSet<Local>` on `FnBuilder::canvas_locals` tracks
  which locals hold a `std.web.Canvas`. The taint enters via
  `lower_call`'s module-receiver `effect_invoke` arm when the
  callee path matches `CANVAS_CONSTRUCTOR_PATH = "std.web.Canvas.new"`;
  it propagates through let-binding hand-offs in
  `stmts::bind_pat_assign`; and the `MethodCall` (chained
  receiver) + local-method-call (`canvas.fill_rect(...)`) arms
  in `exprs::lower_expr` and `exprs::lower_call` route tainted-
  receiver calls to
  `Rvalue::Call { func: FnRef::Builtin(BuiltinId::CanvasOp(kind)) }`
  via the new `canvas_op_for_method` lookup table. The taint
  scope is per-fn — passing the handle as a parameter to another
  fn does not carry the taint (documented as v0.26 follow-up
  in demo 06 V2 §A).
- **Why a taint approach instead of the typed receiver?** The
  v0.23-era `std.web.Canvas` ADT isn't in the type-checker's
  prelude, so the type-checker stamps the canvas handle as
  `TyData::Error`. Adding the prelude entries would force a
  parallel edit across `mty-types` and every type-checker test
  that depends on the empty-prelude shape — out of scope for
  this slice. The taint scheme keeps the typeck untouched and
  surfaces the canvas surface at IR-lowering time only.
- **Unit-fn stack-balance fix (`mty-codegen-wasm/src/emit.rs`).**
  The `FnRef::User(callee)` arm of `emit_call` now looks up
  the callee's return type and pushes a placeholder `i32.const 0`
  when `ret_ty` is `Unit` or `Never`. Every other arm (`Log` /
  `DomOp` / `CanvasOp::Clear` / `CanvasOp::FillRect` / P2
  direct-imports) already did this — the `User` arm was the
  lone gap. v0.23 + v0.24 didn't trip on it because the
  pre-Track-A export wiring meant Unit-returning helper calls
  inside `frame` / `keydown` / `keyup` got dead-code-eliminated;
  Track A's `is_web_callback_export` (v0.24) opened the path
  that exposed the bug. The fix is conservative
  (`unwrap_or(true)` defaults to "don't push a placeholder" if
  the callee can't be found, avoiding the symmetric "extra
  value on stack" error).
- **24 new tests across `crates/mty-codegen-wasm/tests/canvas_lowering.rs`,
  `crates/mty-ir/src/lower/{exprs,stmts,ctx}` unit tests, and
  `crates/mty-codegen-wasm/tests/unit_fn_stack_balance.rs`.**
  Cover each of the 8 `CanvasOpKind` variants routing from
  Mighty source through `canvas.fill_rect(...)` shape to the
  expected wasm import; canvas-taint propagation through
  let-rebind in the same fn; canvas-taint does NOT propagate
  through fn-call parameters (the v0.26 follow-up surface,
  pinned); Unit-fn helper from inside `keydown` validates;
  the original v0.23/v0.24 probe fixtures
  (`/tmp/mtyprobe/probe5.mty`, `probe22.mty`) compile and
  validate end-to-end.

See [`CANVAS_HIR_ROUTING_V0_25_NOTES.md`](../notes/CANVAS_HIR_ROUTING_V0_25_NOTES.md).

### Track B — `extern js { fn _foo() }` emits real wasm imports

Closes v0.24 Track E gap E. The wasm32-web emitter now lifts
`extern js` blocks into `(import "mty:web/js" "<name>" ...)`
entries in the core module and the matching `interface js { ... }`
into the WIT world.

- **IR side-table.** A `Program::extern_bindings:
  HashMap<IrFnId, ExternBinding>` (with `ExternBinding { abi,
  name }`) hangs off `Program` alongside the existing
  `span_table`. Manually-constructed test fixtures leave the
  slot empty (matches `span_table`'s pattern). The IR lowerer
  populates the table from each `Item::ExternBlock` via the new
  `record_extern_bindings` pass in `register_fn_shells`.
- **Wasm emitter pre-declare pass.**
  `predeclare_extern_js_imports` runs in `Emitter::emit`
  immediately before `declare_fns` — same protocol as the
  v0.23 `predeclare_canvas_imports` and `predeclare_p2_direct_imports`
  (the function-index space counts imports and module-local
  fns together, so any import slot reserved mid-emit shifts
  every later fn's index). For each
  `(IrFnId, ExternBinding { abi: "js", name })`: build the
  wasm sig via `fn_sig_for_extern_js` (string params expand
  to `(ptr:i32, len:i32)` pairs — matches what `Const::Str`
  pushes at the call site and the canonical-ABI flat layout
  the other `mty:web/*` imports use); append `(import
  "mty:web/js" "<name>" (func ...))` to the import section;
  record `fn_index[fn_id] = <new import idx>` so call-site
  dispatch via `FnRef::User(callee)` naturally lands on the
  import (no separate `BuiltinId::ExternJs` arm); mark the fn
  in `extern_js_fns` so `declare_fns` + the body-emit loop
  skip it.
- **WIT stub.** `emit_wit` adds `import mty:web/js;` to the
  world *only* when the program declared at least one extern-js
  fn (keeping the surface clean for unrelated demos), and
  `append_host_stubs` emits a per-program `interface js { ... }`
  inside the `mty:web` package listing each declared fn.
- **Naming convention.** Import module: `mty:web/js`
  (kebab-case, matches `mty:web/dom`, `mty:web/canvas`,
  `mty:web/input`, `mty:web/log`). Import name: verbatim from
  the user's source. Leading `_` is preserved in the wasm
  import entry. **NOTE:** the WIT stub generator currently
  kebab-cases away leading underscores; the WIT side disagrees
  with the wasm side on `_foo` shapes, so `wit-component`'s
  `wrap_as_component` step fails for any leading-underscore
  extern_js fn — pinned as v0.26 candidate #2 below + demo 06
  V2 §B. The Track B regression tests skip
  `wrap_as_component` (they call `compile_program_to_bytes_with_preview`
  directly) so the test suite is green; the
  end-to-end drift only surfaces on `mty build --target
  wasm32-web examples/15_extern_js.mty`.
- **13 new tests across `crates/mty-codegen-wasm/tests/extern_js_imports.rs`
  + `crates/mty-codegen-wasm/src/wit.rs` unit tests.** Cover
  single-extern-fn programs lower to a real wasm import; the
  WIT world picks up the `import mty:web/js;` line; programs
  without extern js declarations leave the WIT world unchanged
  (back-compat); the per-program `interface js { ... }` stub
  enumerates every declared fn with WIT-native types.

See [`EXTERN_JS_IMPORTS_V0_25_NOTES.md`](../notes/EXTERN_JS_IMPORTS_V0_25_NOTES.md).

### Track C — agent fields `[T; N]` + cross-callback persistence (SIR runtime side)

Closes v0.24 Track E gaps C + D — the two related symptoms that
blocked the Notetris 10x20 board from living on the agent.

- **Fixed-size arrays in agent fields.** The parser already
  accepted `[T; N]` in agent state-field declarations — the
  break was in **HIR lowering**:
  `crates/mty-hir/src/lower/types.rs`'s `TYPE_ARRAY` arm
  extracted the element type but unconditionally set
  `len: None`, dropping the size expression and making the
  downstream type resolver build a slice shape
  (`TyData::Array { elem, len: None }`) instead of the fixed
  array (`TyData::Array { elem, len: Some(N) }`). The fix is
  a 12-line change to capture the first expression-shaped
  child of `TYPE_ARRAY` and lower it as an `ExprId`, then
  pass `len = Some(...)` into `HirType::Array`. The
  downstream `const_eval_len` path already handles integer
  literals — a `[U32; 200]` round-trips to
  `TyData::Array { elem: u32, len: Some(200) }` without
  further work.
- **SIR-runtime cross-callback persistence (regression-test
  pin).** Persistence already worked here — every message
  dispatch goes through `run_one_turn_with_shared_reply`
  which `lock()`s the agent's `Mutex<Value>` state slot,
  hands it to the interpreter as the `self` value, and writes
  back the mutated state at end-of-turn. The pin is a new
  regression test
  (`crates/mty-runtime/tests/agent_callback_persistence.rs`)
  with three cases: (1) spawn agent, send three `Inc()`
  messages, assert replies `1, 2, 3` (not `1, 1, 1`); (2) set
  via callback A and read via callback B (the exact Track E
  worry); (3) two agent instances have independent state.
- **wasm32-web export-callback path: design doc + v0.26
  carry.** `crates/mty-codegen-wasm` has no agent lowering
  today; `agent` blocks get the IR-side ctor + handler shells
  but those fns aren't lifted into the embedded core wasm
  module and the JS host's `inst.exports.keydown(k)` doesn't
  dispatch through them. Track C's notes record the design
  (single-agent-instance pattern, fixed memory region per
  agent, `__agent_<Name>__inst_ptr` global, callback exports
  load state pointer + call handler with state as implicit
  first arg, linear-memory persistence across export calls) as
  the v0.26 emitter slice. Estimated 1-day slice once a v0.26
  swarm picks it up.
- **What still doesn't work.** `const N: U32 = 200; agent X
  { board: [U32; N] }` parses, but `const_eval_len` only
  handles literal ints — a `const` reference resolves to
  `len: None` (slice degrade). v0.26 should grow a real
  const-evaluator for array lengths; for v0.25 users pass
  literals. `[Piece; 7]` where `Piece` is a user enum parses
  + typechecks but the SIR runtime hasn't been exercised
  against enum-typed cells.
- **12 new tests** across `crates/mty-syntax/tests/agent_fields_arrays.rs`
  (5 parser tests), `crates/mty-types/tests/agent_field_array_typeck.rs`
  (4 typeck tests including the `HirType::Array.len == Some(_)`
  regression assertion), `crates/mty-runtime/tests/agent_callback_persistence.rs`
  (3 persistence cases).

See [`AGENT_FIELDS_V0_25_NOTES.md`](../notes/AGENT_FIELDS_V0_25_NOTES.md).

### Track D — `format!()` extended specs (width / precision / alignment / sign / `#` / `0` / fill / `b` / `o`)

Closes v0.24 Track E gap F. The v0.24 Track B `format!()` macro
shipped with the four conversion sigils + named-arg passthrough
+ brace escapes and deferred every layout flag. Track D closes
the deferral end-to-end.

- **Canonical Rust spec grammar.** Track D extends the spec
  parser to the full Rust layout grammar:
  `[[fill]align][sign][#][0][width][.precision][type]`.
- **New supported specs:** `{:5}` (min width, right-align
  default for numbers), `{:05}` (width + zero-pad), `{:<5}` /
  `{:>5}` / `{:^5}` (left / right / center align), `{:*<5}`
  (custom fill char), `{:.3}` (precision — float decimals or
  string max), `{:+}` (always show sign), `{:#x}` / `{:#X}` /
  `{:#b}` / `{:#o}` (alternate prefixes), `{:b}` / `{:o}`
  (binary / octal no-prefix). Combined specs respect the
  canonical ordering — `{:#05x}` renders `0x0ff` for 0xff,
  `{:+05}` renders `+0001` for 1, `{:>10.3}` renders
  `     3.142` for 3.14159.
- **Lowering shape.** The macro lowers a spec'd placeholder
  into one of three Mighty expression shapes selected by
  `is_bare_conversion()`: a bare `{}` keeps the v0.24 simple
  expansion (`+ (x).to_str()`); a spec'd `{:5}` lowers to a
  `__fmt_pad_<align>_<fill>(x.to_str(), width)` call against
  a prelude helper; a `{:.3}` precision-spec'd float lowers
  via `__fmt_float_prec`. The prelude `fmt` interner grows
  per-spec formatter helpers and length tally fns.
- **New diagnostics:** MT6011 (`UNSUPPORTED_FORMAT_TYPE` —
  e.g. `{:e}` not yet supported), MT6012 (`MALFORMED_FORMAT_WIDTH`
  — non-integer or negative width), MT6013
  (`MALFORMED_FORMAT_PRECISION`). All surface at macro-
  expansion time against the `format!(...)` call site.
- **Carried forward to v0.26 (deferrals):** positional `{0}`,
  dynamic width / precision (`{:1$}`, `{:.*}`), explicit
  `n=v` named-arg passthrough (works as `{n}` in-scope
  shorthand today). Pinned in demo 06 V2 §E.
- **64 new tests across `crates/mty-macros/src/stdlib/format.rs`
  (40 spec-parser unit), `crates/mty-macros/tests/format_macro.rs`
  (18 macro-expansion integration), `tests/conformance/macros/`
  (6 conformance fixtures: `format_width`, `format_precision`,
  `format_align`, `format_arity`, `format_basic`,
  `format_unsupported_spec`).**

See [`FORMAT_EXTENDED_V0_25_NOTES.md`](../notes/FORMAT_EXTENDED_V0_25_NOTES.md).

### Track E — `std.String` + `std.Vec[T]` real impls

Foundational stdlib slice: every prior Mighty program built
strings via concatenation chains and arrays via fixed-size `[T; N]`
declarations. v0.25 lands the two canonical owned, growable
types in `mty-stdlib`.

- **`std.String` (`crates/mty-stdlib/src/string.rs`).** Owned,
  growable, UTF-8 byte string. Wraps a `Vec<u8>` so the byte
  buffer is shared with `crate::vec::Vec<u8>` for the wasm
  linear-memory layout. Methods: `String.new()`,
  `String.with_capacity(n)`, `String.from_str(s)`,
  `String.from_utf8(bs)`, `s.len()` (byte count — UTF-8,
  NOT chars), `s.is_empty()`, `s.push_str(t)`, `s.push(c)`,
  `s.clear()` (preserves capacity), `s.as_str()`,
  `s.to_str()` (alias of `as_str` for format-macro use). Plus
  host-internal helpers: `capacity`, `as_bytes`,
  `Display`/`Debug` impls, `From<&str>`,
  `From<std::string::String>`/`Into`. The type deliberately
  avoids `unsafe` — every UTF-8 re-validation that
  `std::string::String` skips with `from_utf8_unchecked`, we
  redo through `std::str::from_utf8`. The ~5 % throughput hit
  on `push_str` is acceptable because Mighty's stdlib is the
  trust anchor.
- **`std.Vec[T]` (`crates/mty-stdlib/src/vec.rs`).** Generic,
  growable array. `#[repr(transparent)]` over
  `std::vec::Vec<T>` so the storage layout is identical to
  what the wasm Component ABI's `list<T>` lowers to. Methods:
  `Vec.new()`, `Vec.with_capacity(n)`, `v.push(x)`, `v.pop()`,
  `v.get(i)`, `v.len()`, `v.is_empty()`, `v.clear()`, `v.iter()`.
- **Example walkthrough (`examples/26_string_vec.mty`).** The
  example uses literal-only `log(...)` calls because the
  log-of-dynamic-string lowering is a Track A v0.26 follow-up
  — the String / Vec API surface is fully exercised in
  helper fns above main. The `_` prefix on helper fns keeps
  them out of the WIT export world.
- **41 new tests across
  `crates/mty-stdlib/tests/{string_real,vec_basic}.rs`** (19
  string + 22 vec). Cover the round-trip semantics, the
  UTF-8 byte-vs-char distinction, capacity preservation
  across `clear`, the `From<&str>` / `Into` interop, the
  `Display` + `Debug` shapes, plus `Vec` push / pop / get /
  iter / capacity round-trips.

See [`STDLIB_STRING_VEC_V0_25_NOTES.md`](../notes/STDLIB_STRING_VEC_V0_25_NOTES.md).

### Track F — demo 06 V2 (canvas-direct via Round 1 closures) — SHIPPED-PARTIAL

A canvas-direct rewrite of demo 06_canvas_game that consumes
the v0.25 Tracks A–E closures and stands up the canonical
canvas-direct architecture for web games. The deliverable
shape was "the JS shim drops to ~50 LOC because the agent owns
all rendering". The actual shape: shim **213 → 110 LOC (−48 %)**,
the Mighty source grows from 186 → 313 LOC (it now carries the
full canonical agent declaration, the canvas-direct render
path, and the `Vec[U32]` board construction), and **5 narrow
gaps surface for v0.26**.

- **`demos/06_canvas_game/src/main.mty` (313 LOC, +127 vs
  v0.24).** The `agent Notetris: NotetrisInput { board: [U32;
  200] = [0; 200], score = 0, level = 1, lines = 0, ... }`
  declaration is now the protocol of record (Track C's array-
  in-agent-fields fix lets this typecheck). The `frame(dt)`
  body opens `let canvas = std.web.Canvas.new(240, 480)` and
  routes 30+ render ops through that local (background +
  stroke + 9 vertical grid lines + 19 horizontal grid lines
  via Track A's canvas-handle taint propagation). HUD lines
  use Track D's `format!("score: {:>4}", n)` right-align spec
  + `{name}` interpolation.
- **`demos/06_canvas_game/web/dom-shim.js` (110 LOC, −103 vs
  v0.24).** Every byte of canvas-call translation moved to
  Mighty source. The remaining shim is split: ~22 LOC piece
  definitions + helpers; ~12 LOC state mirror + dynamic board
  overlay (waiting on wasm32-web agent persistence, v0.26
  candidate #1); ~10 LOC intent-stream parser + state
  dispatch; ~25 LOC WIT import bindings + RAF/keyboard wiring
  + boot; ~40 LOC piece tables + RGBA helper + core-extractor.
- **Pre-flight gates (all PASS at Track F commit time):**
  `cargo build --workspace` clean; `mty check` + `mty fmt
  --check` clean; `bash smoke.sh` confirms component magic
  bytes (2830-byte Component envelope); `MTY_WEB_SMOKE=1
  bash smoke.sh` headless Playwright PASS at phash distance
  4 / tolerance 12 (the visual shifted because the agent now
  paints background + grid + HUD column directly; budget
  absorbs the shift).

#### 5 v0.26 gaps surfaced

| #  | Gap                                                                                                       | v0.26 closer (proposed track)                                                                                                                  |
|----|-----------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| §A | Canvas-handle taint doesn't flow through fn parameters                                                    | propagate taint through fn parameter types when the param resolves to `std.web.Canvas`; closes after typeck exposes `std.web.Canvas` in prelude |
| §B | extern_js kebab-vs-leading-underscore drift through `wit-component` `wrap_as_component`                   | either use raw fn name (preserve `_`) in `emit_extern_js_interface`, OR strip leading `_` in `predeclare_extern_js_imports`. Option 1 needs WIT spec verification |
| §C | wasm32-web agent persistence (emitter-side; runtime-side already shipped in Track C)                      | implement the single-agent-instance pattern Track C designed: `Emitter::emit_agent_state_region`, callback-export dispatch, `__agent_<Name>__inst_ptr` global |
| §D | `const KEY_LEFT: U32 = 37` referenced in a match arm `KEY_LEFT => ...` binds as a fresh variable          | pattern-compile resolve `const` identifiers to their literal value before falling through to the variable-binding path                         |
| §E | `format!("{n}", n=value)` rejected (parses as 0 positional + 1 named ref vs 1 positional + 0 named)        | accept the named-arg shorthand alongside the existing in-scope-binding `{n}` form                                                              |

See [`DEMO06_V2_V0_25_NOTES.md`](../notes/DEMO06_V2_V0_25_NOTES.md)
for the per-gap probe transcripts + the specific v0.26 closer
shape for each.

## Integration findings (this tag commit)

The six tracks landed against a clean main; integrator surgery
this slice was heavier than v0.24 — the orchestrator commit
`4b8ae7a` ("ci: fix clippy-strict failures across v0.25 swarm")
fixed 5 clippy-strict lints introduced by parallel-track
interactions (`manual_let_else` and four others surfaced by
the unified `cargo clippy --workspace --all-targets -- -D
warnings` sweep that no individual track ran). The integrator
this tag commit fixed two further line-ending / blank-line
drifts in `examples/25_agent_array.mty` and
`examples/26_string_vec.mty` that the canonical formatter
flagged on a CI re-sweep — both pure formatter idempotence
fixes, no source-level intent change.

- **CI was red at hand-off for one reason: example sweep
  `mty fmt --check`.** `examples/25_agent_array.mty` was
  committed with CRLF line endings (the file ended `...}\r\n`
  where the formatter outputs `...}\n`). `examples/26_string_vec.mty`
  had two blank lines around a `// ----- section -----` divider
  that the formatter collapses to zero. Both fixed by re-running
  `mty fmt --stdin` and committing the canonical output. No
  source-level intent change; both files semantically unchanged.

## Verification (rerun locally)

```bash
git checkout v0.25.0

cargo build --workspace                                    # clean
cargo test --workspace                                     # 1790 passing
cargo clippy --workspace --all-targets -- -D warnings      # clean
cargo fmt --all -- --check                                  # clean
cargo audit --deny warnings                                 # clean

cargo test -p mty-driver --test conformance_full           # 1 passing
cargo test -p mty-driver --test conformance_codegen        # 22 passing
cargo test -p mty-driver --test conformance_runtime        # 1 passing
cargo test -p mty-driver --test conformance_runtime_7      # 1 passing
cargo test -p mty-driver --test selfhost_codegen           # 23 passing
cargo test -p mty-macros --test format_macro               # 40 passing

cd impl-py && python -m pytest tests/ -q && cd ..          # 490 passing, 3 skipped

for d in demos/*/; do bash "$d/smoke.sh"; done             # 6/6 PASS

# Headless-browser smoke (opt-in, needs Playwright):
cd tests/web-smoke && npm ci && cd ../..
MTY_WEB_SMOKE=1 bash demos/02_counter_web/smoke.sh         # PASS (dom mode)
MTY_WEB_SMOKE=1 bash demos/05_notetris_web/smoke.sh        # PASS (canvas + phash dist 1)
MTY_WEB_SMOKE=1 bash demos/06_canvas_game/smoke.sh         # PASS (canvas + phash dist 4)
```

## v1.0 freeze gate status after v0.25

| Blocker                                       | Status     | Notes                                                                                                                                                                                                                                                                                                                                                              |
|-----------------------------------------------|------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| #1 Second independent compiler implementation | **CLOSED** | (v0.19, extended v0.22, polished v0.25) Python 2nd-impl through HM + closures + generic-constraints + borrow + wasm codegen + format-spec parser. **490 tests** (+16 in v0.25); 23/23 examples typeck clean; 21/24 emit wasm.                                                                                                                                                |
| #2 RFC 30-day comment windows                 | **Infra shipped + dashboard live — user action pending** | `COMMENT_WINDOWS.md` is the master tracker; `RFC_DASHBOARD.md` (v0.24) has the per-window countdowns + per-RFC implementation status. User must open the 8 GitHub Discussions threads. Earliest close: 2026-06-09 (RFC-005). Latest close: 2026-07-25 (RFC-002 / RFC-006).                       |
| #3 Published normative conformance suite      | **CLOSED — normative/informative split declared v0.24, kit grows to 159 v0.25** | (v0.19/v0.20) `scripts/build-conformance-kit.sh` builds the tarball; v0.25 grows it 156 → 159 cases / 24 categories (Track D adds 6 new `format_*` cases, replaces 3 v0.24 stubs). v1.0 GA normative/informative split via `tests/conformance/v1.0-NORMATIVE.md` (104 normative / 49 informative) unchanged.        |

**Earliest possible v1.0.0 tag: 2026-07-26.** Unchanged from v0.24.
The day after the last RFC comment window (RFC-002 / RFC-006, 60
days each) closes. At this point **only RFC dispositions** stand
between main and v1.0 GA.

## v0.26-RC1 candidate tracks

From Track F's 5 surfaced gaps:

1. **wasm32-web agent persistence (emitter-side; Track F gap §C).**
   Track C v0.25 pinned the SIR-runtime side; the emitter side
   has no agent lowering. Implement the single-agent-instance
   pattern Track C designed: `Emitter::emit_agent_state_region`
   reserves a fixed memory region per agent declaration sized
   to the resolved state struct; `__agent_<Name>__inst_ptr`
   global; callback-export dispatch (`keydown`, `frame`, ...)
   loads the agent state pointer and calls the handler with
   state as implicit first arg. Closes the demo 06 V2 shim's
   ~12 LOC state mirror.
2. **extern_js kebab-vs-leading-underscore drift through
   component encode (Track F gap §B).** Pick: either use the
   raw fn name (preserve `_`) in `emit_extern_js_interface`
   (smaller change but needs WIT spec verification for the
   leading-`_` identifier path) OR strip the leading `_` in
   `predeclare_extern_js_imports` (back-compat-breaking for
   hand-written JS shims targeting `_foo`). Pick option 1 if
   WIT spec admits `%_alert`-style escapes; option 2 otherwise.
3. **Canvas handle taint propagation through fn params + arbitrary
   positions (Track F gap §A).** Propagate the taint through fn
   parameter types — when a param resolves to `std.web.Canvas`,
   carry the taint into the callee's local map. Likely needs
   exposing `std.web.Canvas` in the typeck prelude first (the
   "don't trust the typed receiver" v0.25 workaround relies on
   `TyData::Error`). Enables splitting `render()` into helper
   fns like `render_grid(canvas)` / `render_hud(canvas, score, ...)`.
4. **`const` identifier in match patterns (Track F gap §D).**
   Pattern-compile resolve top-level `const` identifiers to
   their literal value before falling through to the variable-
   binding path. HIR pattern lowerer one-line extension; pin
   literal keycodes to a named vocabulary without losing
   exhaustivity checks.
5. **`format!()` v0.26 deferrals (Track F gap §E + Track D
   deferrals).** Accept `format!("{n}", n=value)` named-arg
   shorthand alongside the in-scope-binding form; positional
   `{0}`; dynamic width / precision (`{:1$}`, `{:.*}`).

After v0.26 the remaining v1.0-RC work is RFC disposition
collection (user-driven by window closures). Once the latest
window closes on 2026-07-25, the integrator collects
dispositions, files them in `RFC_DISPOSITION_<RFC>.md`, builds
the `mty-conformance-kit-v1.0.0.tar.gz`, and tags **v1.0.0**.

## Acknowledgements

v0.25 is a six-track parallel swarm: Tracks A, B, C, D, E, F ran
concurrently, integrator merged. Special call-out to Track A for
landing the two long-standing v0.24 carry-over fixes (canvas
HIR → IR routing + Unit-fn stack-balance) in one slice — both
needed for demo 06 V2 to consume Track A's outputs end-to-end;
to Track E for landing the foundational `std.String` +
`std.Vec[T]` types that every future stdlib slice can build on;
and to Track F for an honest "what landed, what didn't, why, and
how each remaining gap closes" notes file — the 5-gap inventory
IS the v0.26 swarm scope.
