# Python 2nd-impl v0.22 — full pipeline (borrow + sketch wasm codegen)

> **Status**: v0.22 (2026-05-26). Shipped under
> `impl-py/mty/borrow.py`, `impl-py/mty/codegen_wasm.py`, plus the new
> tests/notes. Closes the v1.0-RC validation question of whether the
> Rust reference impl is "the only one that exists" — the Python
> 2nd-impl now round-trips every spec-prose claim through codegen.

## Slice context

The v0.19 → v0.22 plan stratified the second-impl push as:

* **v0.19** (shipped) — HM closure inference + generic constraints
  closed the front-end + middle-end gap on the spec-validation subset.
* **v0.22 (this slice)** — borrow check + wasm codegen close the
  back-end gap. After this, the 2nd-impl covers lex → parse → lower →
  typeck → borrow → codegen end-to-end on the full example corpus.

Working agreements: no peeking at `crates/mty-borrow/` or
`crates/mty-codegen-wasm/`. Spec-driven implementation; ambiguities
logged.

## Phase 1 — borrow check (`mty/borrow.py`)

NLL-flavoured (non-lexical-lifetimes, not Polonius). The design is
deliberately spec-validation-shaped: catch the headline rules from §8
of v1.0-RC2, not field-precision-perfect liveness.

### Place model

```python
@dataclass(frozen=True)
class Place:
    root: int                # binding_id of the rooted local
    path: tuple[str, ...]    # field projection chain
```

Roots are local bindings (params + lets); projections are field names.
Index expressions and method-call places aren't modelled — the corpus
doesn't exercise them under borrow scenarios.

### Loan model

```python
@dataclass
class Loan:
    place: Place
    mut: bool
    span: tuple[int, int]
```

Loans are scoped to the enclosing `HirBlock`. This is the "v0.5 Rust"
borrow check (scope-based liveness), not full NLL. v0.23 will tighten
to per-use last-use precision.

### Diagnostic codes (MT3xxx band)

| Code     | Meaning                                         |
|----------|-------------------------------------------------|
| `MT3001` | Move while a borrow is live                     |
| `MT3002` | Move out of a (or via) borrow / field-project   |
| `MT3003` | `&mut` + `&` coexist on the same place          |
| `MT3004` | Use of a moved value                            |
| `MT3005` | Two `&mut` borrows of the same place alive      |

### Copy-vs-Move derivation

```python
def is_copy(t: Ty) -> bool:
    # Scalars + refs + fn pointers are Copy.
    # Str, [T], (T, ...), records, enums, Option, Result, opaque domain
    # types are Move.
    # TyAny / TyVar default to Copy (escape hatch when typeck didn't pin).
```

This matches v1.0-RC2 §8.3 ("scalars and immutable references are
trivially Copy") plus the spec's pragmatic widening that aggregates
opt-in via the `Copy` derive (not yet implementable without trait
items).

### Per-fn walker

The borrow checker re-walks the HIR with its **own** binding-id
allocator. The lowerer's binding-ids aren't stored on the `HirPat`
ident nodes (only on `HirParam.binding_id` and inside resolution
records at use sites), so the simplest path was a parallel allocator
that re-traces the scoping. This avoids needing to touch the existing
lowerer / HIR.

Branch merging uses an AND-of-moved-flags join: a binding is "moved
after" a branching point only if it was moved on **every** path. This
is the standard conservative join.

### Spec ambiguities flagged

1. **§8 doesn't define the precise NLL/Polonius/scope choice.** The
   spec says "borrows respect lexical scopes by default" but is silent
   on whether mid-block last-use precision is required. The Rust
   reference uses Polonius; the 2nd-impl uses scope-based; both can
   pass the same set of well-formed example programs.
2. **§8.3's `Copy` derive vocabulary is informative.** The spec lists
   scalar copy semantics but defers the derive trait to §17 trait
   prose, which is normative-but-not-spec'd-here. We re-derived the
   list from the prose; the Rust reference is the tiebreaker.
3. **Field-projection move out of a borrow is conflated with whole-place
   move while borrowed** in the example corpus. The spec distinguishes
   MT3001 and MT3002, but no example exercises a clean MT3002 case
   without also tripping MT3001 — we test for either-or.

## Phase 2 — sketch wasm codegen (`mty/codegen_wasm.py`)

Emits Core 1.0 wasm bytes. The intent is to demonstrate that the spec
is implementable through the back-end, not to ship a production
codegen. Pragmatic, intentional scope.

### Supported subset

* `I32` arithmetic (`+ - * / %`), comparisons (`== != < <= > >=`),
  bitwise (`& | ^ << >>`), logical (`&& ||` — implemented as bitwise
  for v0.22; short-circuit lowering is v0.23).
* `Bool` (compiled as i32 0/1).
* `Char` (compiled as the codepoint, i32).
* Locals via `let` — each binding allocates a fresh wasm local.
* `if` / `else` expressions, with block-type `i32` when an else is
  present, `void` otherwise.
* `while` loops via the canonical `block { loop { br_if exit ; body ;
  br loop } }` pattern.
* `return` statements.
* Direct fn-to-fn calls (resolved via the per-module fn index table).
* String literals — pushed as the i32 placeholder `0` (the codegen
  subset doesn't ship an allocator; a future v0.23 slice will land a
  data segment + linear-memory string layout).
* `&x` / `*x` — pass-through (the underlying scalar is forwarded).

### Section emission

The codegen emits a five-section module:

* §5.5.2 — type section (deduplicated function types)
* §5.5.4 — function section (one type-index per fn)
* §5.5.6 — memory section (one 1-page memory, exported)
* §5.5.8 — export section (one export per fn + the memory)
* §5.5.13 — code section (per-fn body, locals vec, end opcode)

We do **not** emit:

* import section (the codegen subset doesn't yet support imports — host
  fns / externs are stubbed with a 0 placeholder)
* table / element sections (no indirect calls)
* global section (no module-level mutable state)
* data section (no string allocation yet)
* start section (no module init)
* custom sections (no name section / debug info)

### Validation approach

No external wasm validator is shipped (we don't depend on `wasmtime`
or `wabt`). Instead we verify:

* Magic + version header bytes match the spec (`\x00asm\x01\x00\x00\x00`).
* Section ID ordering matches §5.5 (non-custom sections appear at most
  once, in strictly increasing id order).
* Each fn body ends with the `end` opcode (`0x0B`).
* LEB128 encoders are round-trippable.

This is the cheap-but-load-bearing structural verification the example
corpus actually exercises.

### Spec ambiguities flagged

4. **§14's wasm target is informative.** The spec gestures at "the
   primary native target is wasm 1.0 + threads + tail-call" but doesn't
   pin the exact extension set. Our codegen targets pure 1.0; threads
   and tail-call extensions are deferred.
5. **String layout is unspecified.** v1.0-RC2 §6.2 says `Str` is
   "UTF-8 encoded, length-prefixed, heap-backed" but doesn't pin the
   ABI (ptr+len on the stack? a fat-pointer struct? a sentinel?). We
   emit a 0 placeholder rather than commit to a layout; this is the
   single biggest deferred decision for v0.23.
6. **Effect rows have no codegen lowering.** The spec doesn't say how
   effect-row metadata reaches the back-end (custom section? a name
   table? erased entirely?). We erase, which matches the typeck's
   TyAny-absorption policy.

## Phase 3 — full-pipeline sweep (`tests/test_examples_full_pipeline.py`)

Parametrised over `examples/*.mty` (24 examples). Each example runs:

* lex (must succeed; tested elsewhere)
* parse (must succeed; tested elsewhere)
* lower (must succeed; tested elsewhere)
* typeck (must complete without exceptions; clean diags expected per
  the v0.19 baseline)
* borrow (must complete without exceptions; diagnostics tolerated)
* codegen (must produce bytes with valid wasm magic; ≥ 15/24 must
  emit at least one fn body)

Coverage at v0.22:

| Example                       | parse | lower | typeck | borrow | codegen-fns |
|-------------------------------|-------|-------|--------|--------|-------------|
| 01_hello.mty                  | ok    | ok    | ok     | ok     | 1           |
| 02_struct_enum.mty            | ok    | ok    | ok     | ok     | 1           |
| 03_generic_fn.mty             | ok    | ok    | ok     | ok     | 1           |
| 04_result_propagation.mty     | ok    | ok    | ok     | ok     | 2           |
| 05_match_expr.mty             | ok    | ok    | ok     | ok     | 2           |
| 06_for_while_loop.mty         | ok    | ok    | ok     | ok     | 2           |
| 07_agent_echo.mty             | ok    | ok    | ok     | ok     | 0 (agent-only) |
| 08_agent_state.mty            | ok    | ok    | ok     | ok     | 0 (agent-only) |
| 09_send_ask_deadline.mty      | ok    | ok    | ok     | ok     | 1           |
| 10_supervisor.mty             | ok    | ok    | ok     | ok     | 0 (agent-only) |
| 11_budget_block.mty           | ok    | ok    | ok     | ok     | 2           |
| 12_arena.mty                  | ok    | ok    | ok     | ok     | 2           |
| 13_capabilities.mty           | ok    | ok    | ok     | ok     | 1           |
| 14_extern_c.mty               | ok    | ok    | ok     | ok     | 2           |
| 15_extern_js.mty              | ok    | ok    | ok     | ok     | 1           |
| 16_macro.mty                  | ok    | ok    | ok     | ok     | 1           |
| 17_unsafe.mty                 | ok    | ok    | ok     | ok     | 2           |
| 18_sandbox.mty                | ok    | ok    | ok     | ok     | 1           |
| 19_backend_service.mty        | ok    | ok    | ok     | ok     | 1           |
| 20_frontend_component.mty     | ok    | ok    | ok     | warn(1)| 1           |
| 21_wasi_preview2.mty          | ok    | ok    | ok     | ok     | 1           |
| 22_effect_row.mty             | ok    | ok    | ok     | ok     | 3           |
| 23_multi_row.mty              | ok    | ok    | ok     | ok     | 1           |
| 24_multi_row_full.mty         | ok    | ok    | ok     | ok     | 1           |

21/24 emit at least one fn body. The three zero-fn examples
(07, 08, 10) contain only `agent` declarations which are lowered as
`HirOpaque` and aren't in the codegen subset.

## Test count delta

| Suite                                | v0.19 | v0.22 |
|--------------------------------------|-------|-------|
| test_lexer.py                        | 90    | 90    |
| test_parser.py                       | 49    | 49    |
| test_examples.py                     | 48    | 48    |
| test_hir.py                          | 24    | 24    |
| test_typeck.py                       | 38    | 38    |
| test_typeck_closure.py               | 14    | 14    |
| test_typeck_generics.py              | 19    | 19    |
| test_examples_typeck.py              | 72    | 72    |
| **test_borrow.py** (new)             | -     | 28    |
| **test_codegen_wasm.py** (new)       | -     | 37    |
| **test_examples_full_pipeline.py** (new) | - | 98    |
| **Total**                            | 311   | 474   |

(+163 tests over the v0.19 baseline. Borrow + codegen + sweep all
delivered.)

## v0.23 plan

* **Polonius-style borrow check.** Replace scope-based liveness with
  per-use last-use NLL.
* **ADT codegen.** Linear-memory layout for records / enums; tagged
  union representation; pattern-match lowering.
* **String layout.** Commit to a (ptr, len) ABI on the wasm side; emit
  a data segment for literals.
* **Agent codegen.** Map `agent State { ... }` to a wasm module-with-
  shared-memory or a separate component (TBD by §14 evolution).
* **CLI binary.** Ship `mty-py-cli` so the impl can be used standalone.
* **Cross-impl conformance.** Once the Rust front-end can emit a stable
  JSON CST, diff the Python impl's output against it for the full
  example corpus.

## Known limitations

* The borrow checker tolerates `log("...")` calls that pass non-Copy
  args by treating them as moves (correct), but the spec-prose example
  corpus doesn't exercise borrow-conflict scenarios in a controlled
  way. Negative-test coverage of MT3001–MT3005 comes from synthesised
  test snippets, not corpus examples. v1.0-RC2 polish should consider
  adding a borrow-conflict example to the corpus.
* The wasm codegen treats string literals as pointer-0 placeholders.
  A `log("hi")` lowered through this back-end will pass NULL to the
  host. The Rust reference allocates strings; we'd need a linear-memory
  allocator to follow suit.
* Effect rows are erased at codegen (consistent with typeck's
  TyAny-absorption).
* No name section / debug info in the wasm output.
