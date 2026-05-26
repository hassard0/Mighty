# RFC-008 — Effect row polymorphism

**Status:** **IMPLEMENTED in v0.13..v0.19**, comment window open
through 2026-06-25 (see [`RFC_DASHBOARD.md`](RFC_DASHBOARD.md)).
Originally Draft (v0.13 swarm).
**Tracks amendment:** new — initially scoped from the post-v1.0 roadmap
("higher-order effects" / "effect propagation"). Pulled forward to v0.13
because the v1.0-RC3 spec is locked and the type-checker is mature
enough to absorb a row-polymorphism layer additively.
**Target release:** v0.13 (infra), v0.14 (stdlib HOF roll-out).
**Owner:** type-system swarm (auto-assigned).

## Implementation Status

**SHIPPED end-to-end across v0.13..v0.19.** Slice-by-slice:

* **v0.13** — Row variables landed in `mty-hir` (`HirEffectRow::Open`)
  and `mty-types` (effect inference threads row variables through HOF
  call sites). The closed-set effect language is now a special case
  of "row tail is empty" / `RowTail::Closed`.
* **v0.18** — Parser absorbed the multi-row-variable tail forms
  (`!{| E1, E2}` and `effect a, b | E1, E2`). The v1.0-RC spec went
  RC3 → RC4 to admit the new grammar (see
  [`v1.0-rc.md`](../v1.0-rc.md) §9.2).
* **v0.19** — HIR multi-row lowering complete (every
  `EFFECT_ROW_VAR` child of an `EFFECT_ROW` is read; previously
  only the first row variable lowered). Multi-var signatures now
  round-trip; the v0.18 parse-then-single-var-lower stopgap is
  retired. `HirEffectRow::Open(Vec<HirRowVar>)` is the canonical
  shape.

Diagnostic codes MT4055..MT4059 (this RFC's `## 8.` table) are all
**active emit** as of v0.19. Conformance fixtures for the row-poly
fire-conditions live under
`tests/conformance/effect_checking/` (the v0.20 normative split).

The public comment window opened 2026-05-26 closes the **process**,
not the design — reviewers may surface bugs / clarifications that
absorb as point patches; the row-polymorphism contract itself is
stable.

## Summary

Introduce **effect-row variables** (Koka / Eff-style) into the
`mty-types` effect language. A row variable `E` stands for "any
additional effects the caller might bring". Higher-order functions
(`map`, `filter`, `fold`, ...) gain a closed-over row variable threaded
from their callable parameter through their return effect, so that a
caller passing an effectful closure no longer trips MT4001
(`effect_undeclared`) on the HOF itself.

Today (v0.12) every fn carries a *closed set* of effects:

```mighty
fn map[A, B](xs: List[A], f: fn(A) -> B!{}) -> List[B]!{}
```

Calling `map(xs, |x| fs.read(x))` is rejected: the closure has effects
`{fs}` but the parameter signature requires `{}`.

With effect rows:

```mighty
fn map[A, B, E](xs: List[A], f: fn(A) -> B!E) -> List[B]!E
```

The row variable `E` unifies with `{fs}` at the call, and the result
type carries `!{fs}`, propagating cleanly into the caller's inferred
effect set.

## Motivation

1. **HOFs are unusable with effectful closures.** The 2 most-requested
   stdlib affordances (`List.map`, `Iterator.fold`) silently force
   callers to roll their own monomorphic loops once any capability
   enters the closure body. This is the single biggest tax v0.12 imposes
   on production user code.
2. **No mechanism for "passes through".** Today's only way to express
   "this HOF inherits whatever the closure does" is to declare every
   conceivable effect on the HOF, which is both wrong (over-grants) and
   un-introspectable (the inferred set hides the actual closure's
   demand).
3. **Spec freeze constraint.** v1.0-RC3 is locked and we cannot rewrite
   the effect system shape on the spec side. Row variables are a
   conservative, *additive* extension: they introduce new syntax
   (`!E` instead of `!{a, b}`) without breaking any existing program.

## Detailed design

### Syntax

The grammar gains one production. The effect-clause non-terminal:

```
EffectClause := '!' EffectSet
EffectSet    := '{' EffectList? RowTail? '}'  |  RowVar
EffectList   := Ident (',' Ident)*
RowTail      := '|' RowVar
RowVar       := UpperIdent   # convention: single capital letter, often E
```

So the surface syntax is one of:

| Form                | Meaning                                              |
|---------------------|------------------------------------------------------|
| `!{}`               | closed, empty (existing behavior)                    |
| `!{fs, net}`        | closed (existing behavior)                           |
| `!E`                | a row variable, fully open                           |
| `!{fs | E}`         | "at least fs, plus whatever E brings"                |

Identifier conventions:

- Lowercase identifiers inside `{ ... }` are *concrete effect names*
  (`fs`, `net`, `time`, `rand`, `model`, `alloc`, `spawn`, `dom`,
  `unsafe`).
- An identifier appearing as a tail (`| E`) or alone (`!E`) is a *row
  variable*. By convention single capitals `E`, `R`, `F`.

Row variables MUST be declared in the function's generic clause, same
list that holds type parameters:

```mighty
fn map[A, B, E](xs: List[A], f: fn(A) -> B!E) -> List[B]!E
```

The order in `[ ... ]` is irrelevant (row vars and type vars share the
slot namespace; they're distinguished by usage position).

### Inference rules

An **effect row** is a pair (effect-set, optional row-variable):

```
Row ::= Closed(S) | Open(S, v)
   where S is a finite set of concrete effects
         v is a row variable
```

Reading: `Open(S, v)` denotes the set `S ∪ φ(v)`, where `φ` is the
ambient row substitution. `Closed(S)` denotes exactly `S`.

**Unification** is the centerpiece. The four cases:

| LHS         | RHS         | Action                                                |
|-------------|-------------|-------------------------------------------------------|
| `Closed(a)` | `Closed(b)` | succeed iff `a == b`                                  |
| `Closed(a)` | `Open(b,w)` | succeed iff `b ⊆ a`; bind `w ↦ a \ b`                 |
| `Open(a,v)` | `Closed(b)` | succeed iff `a ⊆ b`; bind `v ↦ b \ a`                 |
| `Open(a,v)` | `Open(b,w)` | bind `v ↦ Open(b \ a, fresh)`, `w ↦ Open(a \ b, fresh)` (same `fresh` on both sides) |

The last case ("two open rows") deserves a note: the standard Koka
algorithm introduces a *single* fresh row var `r0` on both sides so
that the residual unknowns continue to unify with each other later.
After unification, both `v` and `w` denote the same set, which is `a
∪ b ∪ φ(r0)`.

**Occurs check.** A row variable must not occur in its own
substitution. Direct self-binding (`v ↦ Open(_, v)`) is rejected with
an `MT4020 row_occurs_check` diagnostic. Indirect cycles are caught by
the same walk used for type-var occurs checks.

**Generalization.** When generalizing a fn type at definition time,
free row variables (those not bound by an outer scope) are quantified
into the fn's signature. This mirrors HM generalization of type
variables.

**Instantiation.** At each call site, every quantified row variable is
instantiated with a fresh row variable. The fresh row may later be
solved to either `Closed(S)` or `Open(S, v')` depending on the
arguments.

### Subsumption

A closed row `{fs, net}` is a sub-row of an open row `{fs, net | E}`
(any concrete row is "narrower" than an open row that admits at least
the same effects). Subsumption is what licenses a `Closed({})` closure
to satisfy an `Open({}, E)` parameter — the parameter accepts more,
the closure brings less.

The asymmetric rule (closed accepted where open required, but never
the reverse) is the *whole point* of row polymorphism: it's the
mechanism by which a HOF can advertise "I will pass through whatever
effects you bring, and *only* those".

### Backward compatibility

Existing function signatures are unaffected:

1. A fn declared with `!{}` keeps its closed-empty row. Passing an
   effectful closure to such a fn is still rejected (this is the
   *desired* current behavior for fns that genuinely need to remain
   pure regardless of argument).
2. A fn declared with no effect clause keeps its inferred behavior
   (call-graph fixpoint unions callee effects into the caller). No row
   variables are introduced implicitly.
3. Row variables are **opt-in**: they only appear when the author
   explicitly declares one in the fn's generic clause.

This means **no existing program changes meaning**. The new surface
syntax is purely additive.

### Migration

Stdlib higher-order functions SHOULD be relaxed to row-polymorphic in
v0.14. The candidate list:

| Function           | Today                              | v0.14 target                                            |
|--------------------|------------------------------------|---------------------------------------------------------|
| `List.map`         | `fn(A)->B!{}`                      | `fn[A,B,E](f: fn(A)->B!E) -> List[B]!E`                 |
| `List.filter`      | `fn(A)->Bool!{}`                   | `fn[A,E](p: fn(A)->Bool!E) -> List[A]!E`                |
| `List.fold`        | `fn(B,A)->B!{}`                    | `fn[A,B,E](f: fn(B,A)->B!E) -> B!E`                     |
| `Iterator.map`     | (as above)                         | (as above)                                              |
| `Iterator.filter`  | (as above)                         | (as above)                                              |
| `Iterator.collect` | `!{alloc}`                         | `!{alloc | E}`                                          |
| `Result.map`       | `fn(T)->U!{}`                      | `fn[T,U,E](f: fn(T)->U!E) -> Result[U,Err]!E`           |
| `Result.and_then`  | `fn(T)->Result[U,Err]!{}`          | `fn[T,U,E](f: fn(T)->Result[U,Err]!E) -> Result[U,Err]!E` |
| `Option.map`       | (analogous)                        | (analogous)                                             |

v0.13 ships **only** `List.map` to validate the wiring end-to-end; the
rest is flagged as v0.14 work in the notes file.

### Anti-patterns

1. **Row vars in struct fields.** A `struct Handler { f: fn() -> Unit!E
   }` declaration is rejected (`MT4021 row_var_in_struct`). Row
   polymorphism is a fn-signature feature; struct fields must use a
   closed row or the field's containing struct must be the one
   parameterised by `E`. This avoids accidentally smuggling unresolved
   row vars into long-lived data.
2. **Row var only in return.** `fn foo() -> Int!E` is rejected
   (`MT4022 row_var_unbound`): the row variable is never bound by an
   argument, so it can never be inferred. The author probably meant
   `!{}`.
3. **Row var in `!{ a, b }` with no tail bar.** `!{ E }` is ambiguous —
   is `E` an effect or a row var? Reject with `MT4023
   row_var_in_concrete_set` and direct the user to either `!E` (whole
   set is a row var) or `!{a | E}` (concrete plus tail).

### Diagnostics

| Code     | Name                       | Trigger                                                          |
|----------|----------------------------|------------------------------------------------------------------|
| MT4020   | `row_occurs_check`         | row var bound to a row containing itself                         |
| MT4021   | `row_var_in_struct`        | row var appearing in a struct field type                         |
| MT4022   | `row_var_unbound`          | row var that appears only in the return effect clause            |
| MT4023   | `row_var_in_concrete_set`  | `!{ E }` form (use `!E` or `!{a | E}`)                           |
| MT4024   | `row_effect_mismatch`      | unification: two closed rows differ                              |
| MT4025   | `row_subsumption_fail`     | closed row not a sub-row of expected closed row                  |

These slot into the existing `MT4001..MT4010` effect-checker range.

## Open questions

1. **Effect subtraction (handlers).** Koka supports `try { ... } catch
   <Exn> { ... }` removing effects from the row. This is a separate
   mechanism (effect *handlers*) and is deferred to a future RFC —
   stardust's handler/supervisor primitives may already cover the
   non-exception cases, so the interaction needs design before
   commitment.
2. **Effect aliases / unions.** Should `!IO = !{fs, net, time}` be
   introduceable? Convenient, but adds another layer of resolution.
   Deferred.
3. **Variance.** Rows are currently *invariant* under unification
   (a closed row is not equal to an open row containing the same
   effects). Subsumption handles the natural "narrower is OK" case;
   no covariance/contravariance is needed in v0.13.
4. **Row equality across crate boundaries.** Two crates can each
   declare `fn map[A,B,E]` with different internal naming for `E`. The
   substitution / unification is *structural*; the row var names are
   purely internal. No interop hazard.

## Acceptance for v0.13

- `EffectRow` enum, `RowVar` newtype, `RowSubst` substitution table
  live in `crates/mty-types/src/effects.rs`.
- Unification function `unify_rows(&mut subst, lhs, rhs) -> Result<()>`
  implementing all four cases above + occurs check.
- 8+ unit tests of row arithmetic in `effects.rs` and `effects_row.rs`.
- One stdlib HOF (`List.map`) relaxed to row-polymorphic, with a test
  in `effects_row.rs` demonstrating effectful closure propagation.
- Notes file enumerating what's NOT relaxed (v0.14 work).
- No regression in `cargo test -p mty-types`.

## v0.14 follow-up

- Relax remaining stdlib HOFs (Iterator.map, Result.map, fold, filter,
  Option.map, etc.).
- Surface-syntax parser support in `mty-syntax` for the `!{a | E}` and
  `!E` forms (currently the row machinery is wired in `mty-types`
  only; surface-syntax recognises the existing closed-row form).
- Wire row inference into the call-graph fixpoint so user-authored fns
  can declare `[E]` row vars without a stdlib-style entry point.
- LSP hover support that renders row variables in inferred-type
  display.

## v0.17 — multi row-variable extension

v0.13 shipped the single-row-var path: `unify_rows` already handles
multiple distinct row vars (the two-open case allocates a shared
fresh tail), and `RowPolySig::row_var_count` already counts >= 1
slots. v0.17 generalises the *user-authored* fn surface to match: a
single fn signature may now declare any number of row variables,
each bound by an independent fn-typed parameter, with the call
site's RowSubst threading one fresh `RowVar` per declared slot.

### HIR representation

`HirEffectRow::Open`'s second field changed from
`HirRowVar` to `Vec<HirRowVar>`. Length 1 covers the v0.16
SHIPPED-SUBSET single-row-var path bit-for-bit; length N covers the
new multi-row-var case once the parser starts emitting it.

### Typeck representation

`UserRowPolyIndex` is now `{ fns: HashSet<FnId>, meta:
HashMap<FnId, UserRowPolyMeta> }` where `UserRowPolyMeta` carries
the row-variable names + the fn-typed param count. The call-site
walker uses `meta` to fire MT4058 (arity mismatch) when the caller
supplies the wrong number of closure args, and to keep multi-row-
var instantiation honest once the parser ships multi-var surface
syntax.

### Inference rules (multi-var case)

For a callee declared `fn cross[E1, E2](a: fn() -> Unit !E1,
b: fn() -> Unit !E2) -> Unit !{| E1, E2}`:

1. At each call site, allocate two fresh row vars `f1`, `f2` via
   `RowSubst::fresh()`.
2. Substitute `E1 ↦ f1` and `E2 ↦ f2` in the parameter rows and
   the return row.
3. For each closure arg `i`, compute its body's effect row (per
   §inference) and unify it with the substituted parameter row.
4. The return row resolves to `Open({}, f1) ∪ Open({}, f2)`,
   which after closure bindings collapses to the union of all
   closure-arg effect sets.

### Active diagnostics (v0.17)

| Code     | Trigger                                                       |
|----------|---------------------------------------------------------------|
| MT4055   | Row var declared + multiple non-fn-typed params, no closure   |
| MT4056   | Concrete effects + row var, no fn-typed param (heuristic)     |
| MT4057   | Bare row var, parameterless fn or single non-fn param         |
| MT4058   | Caller's lambda-arg count != callee's fn-typed-param count    |
| MT4059   | Caller's closed-row fn can't accept the row substitution      |

### Acceptance for v0.17

- `HirEffectRow::Open` carries `Vec<HirRowVar>`; HIR readiness
  tests in `crates/mty-hir/src/effects.rs::tests` exercise the
  multi-var shape directly.
- `UserRowPolyIndex` carries per-fn `UserRowPolyMeta`.
- Five new active diagnostics in `crates/mty-types/src/diag.rs`
  (`row_var_unused`, `row_var_in_concrete_only`,
  `row_var_returned_but_unbound`, `row_var_arity_mismatch`,
  `row_var_subsumption_fail`).
- `crates/mty-types/tests/effect_row_multi.rs` covers the new
  diagnostic emit sites + the HIR-level multi-var round-trip.
- `examples/23_multi_row.mty` demonstrates the v0.17 single-var
  shape (multi-var parser surface deferred to v0.18).

### v0.18 follow-ups

- Parser extension for `!{a | E1, E2}` (the syntactic gap).
- MT4056 whole-program emit: confirm no caller's closure ever
  binds the row var, then promote the heuristic warning to an
  error.
- LSP hover that renders the per-call-site `RowSubst` for the
  selected expression (so authors can see "this `each` call binds
  E ↦ {fs, net}").

## v0.18 — multi row-variable parser surface

v0.18 added the **parser-side** complement to v0.17's multi-row-var
typeck: `crates/mty-syntax/src/parser/types.rs::effect_row_tail`
now loops `EFFECT_ROW_VAR (COMMA EFFECT_ROW_VAR)*` under one
`EFFECT_ROW_TAIL`, so users can finally write the multi-row-var
shape at the source level. Both surface forms gain the extension
"for free" because they route through the same `effect_row_tail`
helper:

| Form                      | v0.15-v0.17 status | v0.18 status |
|---------------------------|--------------------|--------------|
| `!{| E}`                  | parses             | parses       |
| `!{fs | E}`               | parses             | parses       |
| `!{| E1, E2}`             | parse error        | parses       |
| `!{fs | E1, E2}`          | parse error        | parses       |
| `effect fs | E1, E2`      | parse error        | parses       |
| `!{| E,}` (trailing comma)| parse error        | parse error  |

The HIR layer in v0.18 still consumed only the first
`EFFECT_ROW_VAR` child (calling the v0.15
`EffectClause::row_var_name()` first-only accessor), so multi-var
parser surfaces lowered to single-var HIR — observationally
equivalent to v0.17 typeck behaviour. The v0.19 lowering completeness
section below closes that gap.

## v0.19 — lowering completeness

v0.19 finishes the v0.17/v0.18 RFC-008 work by making
`mty-hir::lower::items::lower_effect_clause` consume **every**
`EFFECT_ROW_VAR` child of an `EFFECT_CLAUSE`, not just the first.
The HIR `HirEffectRow::Open(_, Vec<HirRowVar>)` now matches the
parser-emitted CST shape and the v0.17 typeck `UserRowPolyMeta`
exactly, with no first-only collapse step in between.

### AST iterator

`crates/mty-ast/src/effects.rs::EffectClause` gains
`row_var_names() -> impl Iterator<Item = EffectRowVar>` covering
all three surface shapes uniformly:

* Direct `!E` child (bare-bang form).
* `EFFECT_SET → EFFECT_ROW_TAIL → EFFECT_ROW_VAR*` (braced form).
* `EFFECT_CLAUSE → EFFECT_ROW_TAIL → EFFECT_ROW_VAR*` (legacy
  `effect` keyword form).

The v0.15 `row_var_name()` first-only accessor is preserved but
marked `#[deprecated(since = "0.19.0")]` so any straggler
consumers migrate.

### HIR lowering

`lower_effect_clause` now reads every row var via
`row_var_names()` and emits a `Vec<HirRowVar>` in source order,
each with a stable `idx` (0, 1, 2, ...):

```rust
let row_vars: Vec<HirRowVar> = clause
    .row_var_names()
    .enumerate()
    .map(|(i, v)| HirRowVar::new(v.text(), i as u32))
    .collect();
```

The single-row-var case is bit-for-bit equivalent to the v0.18
output (one `HirRowVar` with idx 0); only the multi-var case is
newly populated.

### Acceptance for v0.19

- `EffectClause::row_var_names()` iterator + `#[deprecated]` on
  the first-only accessor.
- `lower_effect_clause` reads every `EFFECT_ROW_VAR` child and
  produces a fully-populated `Vec<HirRowVar>`.
- `crates/mty-hir/tests/multi_row_lowering.rs` pins HIR shape
  across single-var, multi-var, three-var, concrete+multi, and
  legacy keyword form (8 tests, 1 of which lowers
  `examples/24_multi_row_full.mty` from disk).
- `crates/mty-types/tests/effect_row_e2e_multi.rs` covers
  end-to-end multi-row-var typeck (6 tests): caller-side effect
  union, partial propagation, arity-mismatch diagnostic, three-way
  row-poly, clean-parse contract.
- `examples/24_multi_row_full.mty` demonstrates the
  parser → HIR → typeck pipeline end-to-end.
