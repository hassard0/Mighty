# Mighty Slice 3 — Complete

**Tag:** `v0.3.0-typeck`
**Date:** 2026-05-24

## What landed

- **New crate `mty-types`** implementing the type checker MVP.
  Distinct from `mty-hir` so the dep graph stays honest (formatter +
  dumper don't depend on the inference engine).
- **Resolved type representation (`Ty`)**: interned arena;
  `Bool`/`Int(IntKind)`/`Float(FloatKind)`/`Char`/`Str`/`String`/`Bytes`/
  `Unit`/`Never`/`Tuple`/`Array { elem, len }`/`Ref { mutable, inner }`/
  `Fn { params, ret, effects }`/`Adt(AdtId, Vec<TyId>)`/`Var`/`Param`/
  `RawPtr`/`Module`/`Error`.
- **DefMap with two-pass `build_def_map`**: declare ADTs + fns first
  with placeholders, then fill signatures so forward references work.
  Captures struct/enum/fn generics from HIR (HIR lowering also updated
  to actually populate `generics` — was previously always empty).
- **Synthetic `std.core` prelude**: primitive aliases, `Option[T]`,
  `Result[T, E]`, `AgentRef[T]`, opaque module stubs
  (`std.http`, `std.json`, ...), opaque types referenced by examples
  (`Url`, `Page`, `IoErr`, `Logger`, `Net`, `Fs`, ...), built-in fns
  (`log`, `panic`, `spawn`, `move`, `fetch`, `raw_ptr`, `null`,
  `valid`), and a permissive built-in method table.
- **Bidirectional inference** (`synth_expr` / `check_expr`):
  - Literals (with int/float suffix recognition)
  - Path resolution (locals, fn defs, variant constructors, modules)
  - Block tail / statement / let / if / if-let / match
  - Calls + generic instantiation (per-call `HashMap<ParamId, TyId>`)
  - Struct literals + field arity/missing/duplicate diagnostics
  - Field access with auto-deref through `Ref`
  - Method calls (built-in table + receiver-shape specials)
  - `?` operator (Result-only enclosing fn, matching err types)
  - Pattern checking (binding, ref, tuple, struct, enum, range)
  - Lambdas, casts
  - Send/Ask/Deadline (placeholder — slice 5 wires real protocol
    checking)
  - Spawn → `AgentRef[T]`
- **Unification engine** with occurs check, IntInfer/FloatInfer flex,
  `Error`/`Never` poison-passes-through.
- **Pub-signature validation** (`MT2020 pub_param_needs_type`).
- **25 new diagnostic codes** MT2001..MT2025 with `mty explain`
  entries.
- **Driver integration**: `pipeline::type_check(pkg)` runs the checker;
  `mty check` now does lex + parse + lower + type-check, returning
  non-zero only on errors (warnings tolerated).
- **Negative test corpus** (`tests/typeck_neg/*.sd` + driver test).
- **Examples integration test**: every canonical example asserted
  clean.

## All 20 examples type-check clean

```
mty check examples/01_hello.sd            → ok
mty check examples/02_struct_enum.sd      → ok
mty check examples/03_generic_fn.sd       → ok
mty check examples/04_result_propagation  → ok
mty check examples/05_match_expr.sd       → ok
mty check examples/06_for_while_loop.sd   → ok
mty check examples/07_agent_echo.sd       → ok
mty check examples/08_agent_state.sd      → ok
mty check examples/09_send_ask_deadline   → ok
mty check examples/10_supervisor.sd       → ok
mty check examples/11_budget_block.sd     → ok
mty check examples/12_arena.sd            → ok
mty check examples/13_capabilities.sd     → ok
mty check examples/14_extern_c.sd         → ok
mty check examples/15_extern_js.sd        → ok
mty check examples/16_macro.sd            → ok
mty check examples/17_unsafe.sd           → ok
mty check examples/18_sandbox.sd          → ok
mty check examples/19_backend_service.sd  → ok
mty check examples/20_frontend_component  → ok
```

## Spec interpretation calls (validate in slice 4+)

These are the BOLD calls slice 3 made; record them so slice 4 can
revisit if any look wrong.

- **A7** — `?` strictly inside Result-returning fn (slice 3 amendment).
  Examples 06 and 11 escape strict-mode only because of the
  permissive tolerance policy (unknown ident → fresh Var → `?` on Var
  short-circuits). Slice 4 will tighten this; those examples will
  need explicit `Unit!WorkErr` / `Unit!RunErr` return types when the
  agent/cap scopes go strict.
- **A8** — Unsuffixed integer literals are `IntInfer`, default to
  `I32` via context. Slice 4 may add an explicit defaulting pass.
- **A9** — Primitive type names (`String`, `Bytes`, ...) live in both
  type and value namespaces.
- **A10** — Built-in method table is the slice-3 substitute for
  trait-dispatch method resolution. Replaced in slice 5 once
  coherence lands.
- **A11** — `T!{A, B}` (anonymous error union) resolves to
  `Result[T, Error]` permissively. Real union machinery is v0.2+.
- **A12** — Postfix `?` and `!` require their `Msg` identifier on the
  same line (parser bug fix from slice 2's trivia-skipping lookahead).
- **Opaque tolerance**: unknown values resolve to fresh inference
  vars rather than `MT2021 unresolved_value` errors. This is what
  lets examples that reference agent-state/supervisor-scope/cap-narrow
  items (`work`, `n`, `cache`, `draw`, ...) compile without first
  modelling those scopes. Slice 4+ tightens this.
- **`fetch` returns `Str` not `Bytes`**: prelude built-in to keep
  example 04's `parse(body)?` (where user `parse` takes `Str`)
  composing. Real stdlib will likely revisit.

## Stats

- **224 tests pass** (slice 2: 174 → slice 3: +50)
- ~3 000 lines of Rust added (mty-types)
- 25 new SD2xxx diagnostic codes
- 12 negative-test fixtures
- 20 integration tests (one per example)

## Still deferred (slice 5+)

CLOSED in slice 4:
- ~~Ownership / move / affine / borrow checking~~ — mty-borrow crate
- ~~Explicit defaulting pass for `IntInfer`/`FloatInfer`~~ — A19
- ~~Match exhaustiveness as an error~~ — A16 (MT2015 flipped to Error)
- ~~Real protocol message-type checking for agent handlers~~ — A18
- ~~Real impl-method dispatch on user ADTs~~ — A17
- ~~Scope-aware unresolved-value tolerance~~ — A21

Still open:
- Trait coherence + dyn dispatch (slice 5)
- Effect closure + capability narrowing enforcement (slice 5)
- Top-level `sandbox` items per spec §16.1 (slice 5)
- Stdlib opaque modules → real definitions (slice 6+)
- Per-item canonical printers for the formatter
- HTML template `{expr}` interpolation parsing

## Files of note

- `crates/mty-types/` — new crate
- `docs/internals/typeck.md` — type-checker internals
- `docs/spec/v0.1-amendments.md` — A7..A12
- `docs/reference/diagnostics.md` — MT2001..MT2025 table
- `docs/reference/cli/mty-check.md` — updated semantics
- `docs/superpowers/specs/2026-05-24-slice3-type-checker-design.md`
  — slice design
- `docs/superpowers/plans/2026-05-24-slice3-type-checker.md` —
  implementation plan
- `tests/typeck_neg/` — negative test corpus
- `crates/mty-driver/tests/{examples_typeck,typeck_negatives}.rs`
  — integration tests
