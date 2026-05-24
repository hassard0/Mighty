# Type Checker Internals

> Status: slice 3 (`v0.3.0-typeck`). Implements Hindley-Milner-style
> bidirectional inference with first-order generics. Ownership / borrow
> checking, effect closure, and capability narrowing are deferred to
> slices 4 and 5.

## Crate layout

The type checker lives in `crates/mty-types`:

```
src/
  ty.rs        Ty, TyId, IntKind, FloatKind, EffectId, ParamId, AdtId,
               FnDefId, TyArena (interned), pretty_ty
  defs.rs      AdtDef, VariantDef, FieldDef, FnDef, DefMap, DefRef
  prelude.rs   build_prelude: synthetic std.core
  resolve.rs   build_def_map (two-pass), resolve_hir_type, ParamScope
  infer.rs     Substitution, unify, occurs_check, substitute_params,
               default_inference
  check.rs     Cx, synth_expr, check_expr, check_block, check_pattern
  items.rs     check_package (entry point), pub-signature validator
  diag.rs      SD2xxx diagnostic builders
```

The driver crate calls `sdust_types::check_package(&pkg) -> Vec<Diagnostic>`
after HIR lowering.

## Ty representation

`Ty` is distinct from `HirType` (which is syntactic). `TyData` values
are interned in a `TyArena`; equal types share the same `TyId`. The
arena pre-allocates common primitives (`bool_`, `i32`, `unit`,
`error`, ...).

Notable variants:

- `Int(IntKind)` / `Float(FloatKind)` — concrete primitives plus
  `IntInfer` / `FloatInfer` for unsuffixed literals.
- `Adt(AdtId, Vec<TyId>)` — struct/enum applied to type arguments.
- `Var(TyVarId)` — inference variable. Bindings live in the
  `Substitution`, not in the arena (so binding doesn't mutate
  intered types).
- `Param(ParamId)` — generic-parameter slot. ParamIds are *global*
  across the `DefMap`; each `AdtDef` / `FnDef` records the specific
  ParamIds it owns in `param_ids` so substitution at use sites can
  build a `HashMap<ParamId, TyId>`.
- `Error` — poisoned sentinel. Unifies with anything; suppresses
  cascading diagnostics.
- `Never` — bottom type; unifies with anything.
- `Module(String)` — opaque stdlib module placeholder (`std.http`
  etc.).

## DefMap

`DefMap` is the central name-resolution table. It is built once per
package by `resolve::build_def_map(pkg, &mut arena)`. The build runs in
two passes so forward references work:

1. **Declare**: for each top-level item, allocate an `AdtId` / `FnDefId`
   with placeholder data and register the name in `by_name`. Agents are
   registered as opaque ADTs in the second pass (so their methods can
   reference their own ctor params).
2. **Fill**: with all names visible, walk every item again and resolve
   field types, variant payloads, fn parameter+return types, etc.,
   using `resolve_hir_type`.

The synthetic prelude (see below) is merged into the `DefMap` *before*
the user items so user names shadow.

## Synthetic prelude

`prelude::build_prelude` adds:

- All primitive type aliases (`Bool`, `I8..I128`, `U8..U128`, `USize`,
  `ISize`, `F32`, `F64`, `Char`, `Str`, `String`, `Bytes`, `Unit`,
  `Never`, `Duration`, `Size`).
- `Option[T]`, `Result[T, E]`, `AgentRef[T]` as ADTs.
- Opaque module stubs: `std`, `std.core`, `std.http`, `std.json`,
  `std.dom`, `std.trace`, `std.io`, `std.fs`, `std.net`, `std.time`.
- Opaque types referenced by the canonical examples (`Url`, `Page`,
  `IoErr`, `NetErr`, `ParseErr`, `Logger`, `Fetcher`, `Lowered`,
  `RunErr`, `Fs`, `Path`, `Net`, `Model`, `Dom`, `MainErr`,
  `SearchErr`, `Json`, `Map`, `Config`, `ConfigErr`, `WorkErr`,
  `Planner`, `Tokens`, `Ast`, `Clock`, `Search`, `UserId`, `Shape`).
- Built-in functions: `log: fn(Str) -> Unit effect io`,
  `panic: fn(Str) -> Never`, `spawn: fn[T](T) -> AgentRef[T]`,
  `move: fn[T](T) -> T`, `fetch: fn(Url) -> Str!NetErr`,
  `raw_ptr: fn(USize) -> *U8`, `valid: fn(*U8, USize) -> Bool`,
  `null: fn() -> *U8`.
- A built-in method table covering `len`, `to_str`, `get`, `ok_or`,
  `query`, `set_text`, `read`, `write`, `embed`, `post`, `encode`,
  `serve`, `on`, `restart`, `backoff`, etc. Each entry is permissive
  (variadic arity, fresh-Var return) so the example corpus
  compiles without a full stdlib.

### Tolerance strategy

The prelude tolerance + the permissive-fallback policy in `synth_path`
let the type checker accept references to identifiers that aren't
modelled yet (`work`, `n`, `cache`, `draw`, ...). Unknown values
produce a fresh inference variable rather than `MT2021
unresolved_value`. Unknown methods on opaque receivers produce a fresh
inference variable rather than `MT2007 unknown_method`. Slice 4+
tightens these once agent state / supervisor scope / impl-method
resolution are modelled.

## Inference algorithm

Standard Hindley-Milner with bidirectional propagation:

- `synth_expr(cx, e) -> TyId` — bottom-up, returns the inferred type.
- `check_expr(cx, e, expected: TyId)` — pushes a known type down.

Most expression forms have both modes; `check_expr` is implemented as
`synth_expr` followed by `unify(actual, expected)`. Forms that pick
the mode based on context (calls, struct literals) handle expectations
explicitly.

### Unification

`unify(a, b, subst, arena)`:

1. Walk `a`, `b` through `subst.resolve` to representatives.
2. If equal, ok.
3. `Error` / `Never` on either side: ok (poison).
4. `Var(v)`: occurs-check, then bind.
5. Structural cases zip args (tuple, array, ref, fn, adt, raw_ptr).
6. `IntInfer` vs concrete `Int(_)`: ok (permissive; the concrete
   wins via context). Same for `FloatInfer`.
7. Otherwise: `MT2001 type_mismatch`.

### Generic instantiation

At call sites and constructor use sites, the checker:

1. Looks up the def's `param_ids: Vec<ParamId>`.
2. Allocates a `Vec<TyId>` of fresh inference vars (or uses
   turbofish-supplied arguments).
3. Zips into a `HashMap<ParamId, TyId>`.
4. Walks the def's stored types via `substitute_params`, which is a
   recursive walk that replaces each `Param(p)` with the map lookup.

This is the key reason `AdtDef` / `FnDef` store `param_ids` parallel
to `generics`: the global ParamId space lets the substitution work
without per-call positional bookkeeping.

### Defaulting

Slice 3 does not have an explicit defaulting pass. `IntInfer` /
`FloatInfer` are themselves interned types that unify permissively
with concrete kinds; at diagnostic time, pretty-printing surfaces
them as `{integer}` / `{float}` if no concrete type was ever forced.
Slice 4 may add a real defaulting pass (Rust-style: leftover
`IntInfer` -> `I32`, `FloatInfer` -> `F64`).

## `?` operator

`synth_question(cx, inner)`:

1. Synthesizes `inner`'s type.
2. Must resolve to `Result[T, E]`. If receiver is `Var` or `Error`,
   produces a fresh var silently.
3. Enclosing fn return must also be `Result[_, E']`. Otherwise
   `MT2010 question_outside_result`.
4. `unify(E, E')` — otherwise `MT2011 question_error_mismatch`.
5. The result of `expr?` is `T`.

## Effects and capabilities (parsed only)

`Fn { params, ret, effects: Vec<EffectId> }` carries the declared
effect set; the resolver populates it from `HirFn::effects`. Slice 3
does **not** enforce closure (i.e. callers narrowing or carrying the
callee's effects). Slice 5 adds enforcement. Same story for
capability-typed parameters.

## Diagnostic codes

Slice 3 reserves `MT2001..MT2099`. Currently assigned: `MT2001`
(type_mismatch) through `MT2025` (cannot_take_ref). See
`crates/mty-diagnostics/src/codes.rs` and
`docs/reference/diagnostics.md`.

## Tested invariants

- All 20 canonical examples in `examples/` type-check clean.
- The negative test corpus under `tests/typeck_neg/` exercises every
  load-bearing SD2xxx code.
- Unit tests in `crates/mty-types/src/{ty,defs,infer,prelude}.rs`
  cover interning, lookup, unification (15+ scenarios), and the
  prelude-builder.
- The integration tests in `crates/mty-driver/tests/` drive the full
  pipeline.

## Future work (slice 4+)

- Real defaulting pass.
- Trait coherence and method dispatch via impl-block resolution.
- Ownership / affine / borrow checking (slice 4).
- Effect closure + capability narrowing (slice 5).
- Match exhaustiveness as an error (slice 5).
- Replace the permissive-fallback policy with real diagnostics now
  that the agent/cap/supervisor scopes become first-class.

## Scope-aware permissive/strict policy (v0.3 / A65)

Slice 3's amendment A21 introduced a **permissive** name-resolution
fallback: unresolved identifiers silently resolve to fresh inference
variables so the canonical examples kept compiling while slice 4-5
hardened other surfaces. v0.3 tightens this: each lexical scope now
carries a `ScopeKind` and the unresolved-name fallback only fires in
**permissive** scopes. Strict scopes hard-error with MT2021
(`unresolved_value_strict`).

| ScopeKind         | Strict? | Where it triggers                              | Unknown-name behavior                                |
|-------------------|--------:|-----------------------------------------------|------------------------------------------------------|
| `TopLevelFn`      |     no  | `fn ... { ... }` at module top-level          | Slice-3 fresh-var fallback (A21) — kept              |
| `ExternBlock`     |     no  | `extern { fn ... }` shims                     | Permissive (foreign ABI surface)                     |
| `Macro`           |     no  | macro expansion sites                          | Permissive (token soup; expanded later)              |
| `Unsafe`          |     no  | `unsafe { ... }` sub-block                     | Permissive (raw-ptr builtins, spec §21)              |
| `Arena`           |     no  | `arena name { ... }` body                      | Permissive (arena-implicit names)                    |
| `Budget`          |     no  | `budget { ... } { ... }` body                  | Permissive (budget-category names)                   |
| `Sandbox`         |     no  | `sandbox ID { ... }` body (no narrowing)       | Permissive (legacy v0.2 behavior)                    |
| `AgentBody`       | **yes** | agent state-init, agent methods, fn-in-agent   | **MT2021** if not in tolerance set / locals / prelude |
| `HandlerBody`     | **yes** | `on Msg(...) { ... }` handler body             | **MT2021** if not in tolerance set / locals / prelude |
| `SupervisorBody`  | **yes** | supervisor `children { ... }` expressions      | Currently kept permissive (`tolerance_open=true`)     |
| `CapNarrowBody`   | **yes** | `sandbox ... with { e1; e2 } { ... }` body     | Currently kept permissive (`tolerance_open=true`)     |

Strict scopes still grant two escape hatches:

1. **per-body `tolerance` set** — for `AgentBody`/`HandlerBody`, this
   set carries the agent's state names, ctor-param names, and sibling
   method names so handlers can mention them without lookup.
2. **`tolerance_open` toggle** — opened by inner permissive sub-blocks
   (`unsafe`, `arena`, `budget`, `sandbox`). When true, all names are
   tolerated; when false (the default in strict scopes), the
   ScopeKind's policy applies.

`SupervisorBody` and `CapNarrowBody` are marked strict for *framework
consistency* but currently leave `tolerance_open=true` because slice-7
hasn't yet wired real supervisor / cap-narrowing name resolution. When
slice-7 lands, flipping the toggle activates MT2021 in those scopes
without any further code change.

### Implementation notes

`ScopeKind` lives on the `Cx` struct alongside the existing
`tolerance` / `tolerance_open` fields. Sub-scope openers (`unsafe`,
`arena`, `budget`, `sandbox`) save the parent's `(tolerance_open,
scope_kind)` pair, push their own kind, and restore on exit — so a
strict outer handler with a nested `unsafe { ... }` block does
permissive resolution inside the unsafe block then restores the
strict policy on the closing brace.

The strict path in `synth_path` consults `tolerance_open ||
tolerance.contains(name) || !scope_kind.is_strict()` (any of which
keeps the slice-3 A21 fallback); only when ALL three are false does
MT2021 fire.
