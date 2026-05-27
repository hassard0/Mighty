# v0.26 Track D — v0.25 leftover-gap cleanup

**Slice**: close 3 of 5 v0.25 Track F narrow gaps (per
`DEMO06_V2_V0_25_NOTES.md`). The remaining two (`const` identifier
resolution in match patterns, `format!` named-arg passthrough) are
surface-polish items deferred to v0.27.

**Status**: shipped — see commit. All three closed gaps pass their
regression suites + the existing v0.25 suites still pass.

## Closed gaps

### Gap #1 — wasm32-web single-instance agent persistence

Before this slice the wasm32-web emitter had no concept of an agent
declaration. `Rvalue::AgentSpawn` and any agent-state field projection
fell through to `WasmError::Unsupported`, so the function body
demoted to a single `unreachable` instruction. Cross-callback state
mirrors had to live host-side in JS (see the v0.25 demo 06 shim's
~60 LOC state mirror).

#### Implementation

`crates/mty-codegen-wasm/src/emit.rs`:

* `AGENT_REGION_BASE = 65536` and `AGENT_REGION_PER_AGENT_BYTES =
  65536` constants reserve a per-agent 64 KiB region in linear
  memory, page-aligned. `agent_region_base(idx)` derives the
  per-agent base address.
* `agent_field_layout(fields)` computes per-field byte offsets
  (4-byte alignment for 32-bit scalars; 8-byte for 64-bit; arrays
  flat `N * size_of(elem)`).
* `Emitter::agent_layouts` caches per-agent layouts; populated lazily
  by `Emitter::agent_layout(AgentIrId)`.
* `Emitter::agent_state_locals` tracks which SIR `Local`s in the
  currently-being-emitted fn hold an agent state pointer. Repopulated
  per-fn (cleared in `emit_fn`); `populate_agent_state_locals_for_fn`
  walks the fn's params and tags any `Adt(agent_state_adt)` /
  `&mut Adt(agent_state_adt)` param.
* `Rvalue::AgentSpawn { agent, args }` lowers to `I32Const(agent_base)`
  — the agent's base address is a constant per-program (single-
  instance). The destination local is tagged in `agent_state_locals`
  via `emit_assign`.
* `Rvalue::FieldRead { receiver, field }` against an agent-tagged
  receiver lowers to `(I32Const(base+offset)) (I32Load offset=0,
  align=2)`. The receiver projection chain is matched for `[]`
  (direct pointer) and `[Deref]` (handler `self.field` shape).
* `Stmt::Assign(Place { local, proj: [Field(N)] }, ...)` and
  `[Deref, Field(N)]` against an agent-tagged local lower to
  `(I32Const(base+offset)) <rvalue> (I32Store offset=0, align=2)`.
* The `cabi_realloc` bump-pointer global is initialised to
  `max(CABI_REALLOC_HEAP_BASE, AGENT_REGION_BASE + n_agents *
  AGENT_REGION_PER_AGENT_BYTES)` so the realloc heap doesn't collide
  with the agent regions.
* The wasm initial linear-memory page count is recomputed at emit
  time as `max(16, ceil((heap_init + 4 pages) / page_size))` so the
  module declares enough memory upfront.

`crates/mty-codegen-wasm/src/web_lower.rs`:

* `is_web_callback_export` extended: ANY user fn whose name starts
  with an ASCII letter and isn't `main` / `cabi_realloc` / `memory`
  /  `_` -prefixed now appears in the wasm export section. Previously
  only `frame` / `keydown` / `keyup` did. The v0.26 web shape needs
  arbitrary entry points (e.g. `set_state(...)`, `tick()`) the JS
  host invokes alongside the canonical RAF + keyboard loop; pinning
  the surface to three names made the spawn-then-read pattern
  unobservable across callbacks.

#### Scope / limitations

* **Single instance per agent declaration**. The base address is
  computed from the agent's declaration index, not from a runtime
  table. Multi-instance agents (the cluster mesh case) stay on the
  SIR-runtime path — same v0.25 Track C plan.
* **Zero-initialisation**. Fields are zero on spawn (wasm linear
  memory's default state). Mighty source-level field initialisers
  (`score = 0`) compile cleanly but the v0.26 emitter doesn't yet
  thread the init values into the AgentSpawn-lowering code path —
  they're a no-op against the already-zero memory. Non-zero init
  values are a v0.27 follow-up.
* **No type-driven layout validation**. The Mighty type-checker
  treats the agent state ADT as a regular struct; the emitter
  derives offsets from `prog.adts[state_adt].variants[0].fields`
  with no schema check. If a future pass mis-orders the fields the
  emitter happily produces well-formed but semantically wrong
  load/store offsets.

#### Tests

`crates/mty-codegen-wasm/tests/wasm32_web_agent_persistence.rs` — 5
tests:

1. `agent_field_value_persists_across_callbacks` — spawn agent in
   `main()`, write field via `set_field(42)` export, read via
   `get_field()`, assert `42` round-trips. This is the headline
   gap-closure check.
2. `agent_array_field_persists` — 200-field state struct (the
   Notetris board shape), write cell 7, read cell 7, assert
   `0xCAFE` round-trips.
3. `multiple_callbacks_share_agent_state` — three callbacks
   (`init` / `bump` / `read`); bump three times, read returns 3.
4. `agent_region_layout_isolates_distinct_agents` — two declared
   agents get non-overlapping bases; writing to A doesn't change B.
5. `agent_region_layout_constants_well_formed` — sanity-check the
   public constants (page-alignment, monotonic spacing, ≥ 1 KiB
   per-agent reservation).

All five drive the compiled core wasm through `wasmtime` and call
the exported fns directly — no JS shim involved.

### Gap #2 — `extern_js` kebab-vs-leading-underscore drift

Before this slice the WIT-side stub generator
(`crates/mty-codegen-wasm/src/wit.rs::emit_extern_js_interface`)
ran each extern-js fn name through `kebab(...)` (stripping the
leading `_`), while the wasm-side emitter
(`crates/mty-codegen-wasm/src/emit.rs::predeclare_extern_js_imports`)
preserved the verbatim source name. `_alert` therefore landed as
`alert` in the WIT stub and as `_alert` in the core module's import
section; `wit-component::wrap_as_component` rejected the result
with `failed to resolve import "mty:web/js::_alert"`.

#### The pivot decision

The v0.25 Track F design notes recommended "preserve verbatim
(keep `_alert` everywhere)" — but verification against
`wit_parser::Resolve::push_str` shows the WIT 0.2 lexer rejects
bare leading-`_` identifiers (`invalid character in identifier
'_'`) AND the canonical `%`-prefix escape (`%_alert`) for the same
reason. There's no WIT-side surface that accepts `_alert` as a
function name in the v0.225 parser the workspace currently
depends on.

The viable pivot is therefore the *other* option from the v0.25
notes: **canonicalise both sides via `kebab`** (strip leading `_`,
then snake → kebab the rest). The resulting wasm import name agrees
with the WIT identifier; `wit-component` resolves cleanly. The user-
visible contract change: a JS shim binding `_alert(...)` must now
bind `alert(...)` instead. This brings extern-js naming in line with
how every other `mty:web/*` interface works (DOM, canvas, input,
log).

#### Implementation

* New helper `crate::wit::extern_js_canonical_name(s) -> String` —
  the single source of truth for the canonical name. Today it's a
  thin wrapper around `kebab(s)`; isolating it through a named
  helper keeps the doc-string + ownership focused so a future
  reversal (e.g. if `wit-parser` ever grows a `_`-escape) only has
  to touch one place.
* `wit.rs::emit_extern_js_interface` calls `extern_js_canonical_name`
  in place of the raw `kebab(...)` call.
* `emit.rs::predeclare_extern_js_imports` imports
  `extern_js_canonical_name` and uses it to canonicalise the wasm
  import name BEFORE handing it to `wasm_encoder::ImportSection`.

The existing `is_exportable_fn` WIT-side check still gates
exports on `name.starts_with('_')` (the original Mighty source
convention), so a `_`-prefixed extern still stays out of the WIT
world's export list — the convention's semantics are unchanged
even though the identifier reaching `wit-component` is now kebab.

#### Tests

`crates/mty-codegen-wasm/tests/extern_js_name_consistency.rs` — 5
tests:

1. `example_15_extern_js_compiles_to_component` — the headline:
   full Mighty-source → `wrap_as_component` succeeds. Pre-fix this
   panicked at encode time.
2. `extern_js_underscore_name_canonical_in_wit_and_wasm` — both
   sides emit the canonical name AND `wrap_as_component` succeeds
   AND the verbatim `_alert` does NOT appear in the wasm imports.
3. `extern_js_call_routes_to_canonical_import` — the dispatch
   `Call(idx)` instruction resolves to the canonical import slot
   (no off-by-one from the rename).
4. `extern_js_multiple_fns_all_kebab_consistent` — three extern
   fns in one program; each canonicalises independently and the
   whole pipeline succeeds.
5. `extern_js_canonical_name_helper_round_trips` — pin the public
   helper's transform table directly.

The existing `crates/mty-codegen-wasm/tests/extern_js_imports.rs`
suite was updated in-place to expect the canonical names (the v0.25
asserts that pinned the verbatim `_alert` are now phrased as
"canonical name must appear AND verbatim must NOT appear" — protects
the pivot from regressing in either direction).

### Gap #3 — Canvas handle taint through fn params

Before this slice `is_canvas_handle_receiver` in
`crates/mty-ir/src/lower/exprs.rs` only consulted the per-fn
`canvas_locals` taint set, which was only populated by inline
`std.web.Canvas.new(...)` constructions + let-binding rebinds. A
fn that received a `Canvas` handle as a parameter dropped the taint
on entry: `c.fill_rect(...)` lowered to a generic
`Rvalue::MethodCall` and silently emitted an empty user-fn body on
the wasm target.

#### Implementation

`crates/mty-ir/src/lower/items.rs`:

* New `is_std_web_canvas_type(pkg, ty_id)` — recognises the
  canonical `HirType::Path(["std", "web", "Canvas"])` shape AND the
  single-segment shorthand `Canvas` (future-proofing for a `use
  std::web::Canvas` syntactic sugar). Borrows are unwrapped.
* `lower_one_fn` walks the HIR fn's params in parallel with the
  type-resolved param list (`params_ty`). When a param's source-
  level type satisfies `is_std_web_canvas_type`, the corresponding
  SIR `Local` is marked via `FnBuilder::mark_canvas_local`. The
  existing `is_canvas_handle_receiver` predicate then routes
  `c.fill_rect(...)` through `BuiltinId::CanvasOp(FillRect)`
  without further changes.

This intentionally uses the *syntactic* HIR type (not the
type-checked `TyId`): the type checker stamps `std.web.Canvas` as
`TyData::Error` because there's no prelude entry for the `std.web`
module or the `Canvas` ADT. v0.25 documented this as the blocker
that drove the `canvas_locals` workaround; v0.26 keeps the workaround
shape (HIR-level detection) rather than landing a prelude shape
change in the same slice.

`crates/mty-ir/src/lower/exprs.rs`: the only edit is a doc-comment
update on `is_canvas_handle_receiver` pointing at the new
items.rs-side population path. The predicate itself is unchanged.

#### Tests

`crates/mty-ir/tests/canvas_taint_through_params.rs` — 5 tests:

1. `canvas_param_handle_routes_to_builtin` — the headline:
   `fn helper(c: std.web.Canvas) { c.fill_rect(...) }` lowers to
   `BuiltinId::CanvasOp(FillRect)`.
2. `canvas_param_in_chain` — passing canvas through TWO fn
   boundaries still routes the deepest fill_rect through CanvasOp.
3. `nested_method_call_on_canvas_param` — `c.fill_rect(c.width(),
   ...)` — both the outer fill_rect AND the inner width() call
   route through CanvasOp.
4. `inline_canvas_local_still_works` — regression backstop: the
   v0.25 inline-construction surface still works.
5. `canvas_borrow_param_routes_too` — `fn helper(c: &std.web.Canvas)`
   also routes through CanvasOp (Borrow is unwrapped).

## Remaining v0.25 Track F leftovers (deferred to v0.27)

These two were the lowest-priority items in the v0.25 Track F notes
and don't block any in-flight surface:

* **`const` identifier resolution in match patterns** (§D in v0.25
  notes). Surface polish; literal patterns work in the meantime.
* **`format!` named-arg passthrough** (§E in v0.25 notes). The
  in-scope `{name}` shorthand covers the common case; deferred until
  a real user surfaces the gap.

## Pre-flight gate

```
cargo build -p mty-codegen-wasm -p mty-ir                         # PASS
cargo test -p mty-codegen-wasm --test wasm32_web_agent_persistence # 5/5 PASS
cargo test -p mty-codegen-wasm --test extern_js_name_consistency   # 5/5 PASS
cargo test -p mty-ir --test canvas_taint_through_params            # 5/5 PASS
cargo test -p mty-codegen-wasm                                      # all PASS
cargo test -p mty-ir                                                # all PASS
```

The workspace-wide pre-flight (`cargo test --workspace`) was
constrained by Windows `link.exe` PDB limit (`LNK1318: LIMIT (12)`)
and a sibling-track WIP in `crates/mty-stdlib/src/llm/`,
`crates/mty-stdlib/src/memory/`, `crates/mty-stdlib/src/mcp/` (Track
A / Track C in-flight, off-limits per the v0.26 swarm ownership
contract). Test discipline was therefore "all my owned crates +
direct downstream consumers" rather than the full workspace.

## Test count

* 5 agent-persistence tests (new file)
* 5 extern-js name-consistency tests (new file)
* 5 canvas-taint-through-params tests (new file)
* 7 existing extern-js tests updated in-place (no count delta —
  the v0.25 asserts were rephrased to track the v0.26 pivot)
* 3 web-callback-export tests updated in-place to cover the
  v0.26 surface expansion

Total: 15 new tests + 10 updated.
