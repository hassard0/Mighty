# RFC-001 — First-class union ADTs

**Status:** Draft (v0.9 spec-freeze prep).
**Tracks amendment:** A11 (anonymous error unions resolve to
`Result[T, Error]` sentinel — OPEN since slice 3).
**Target release:** v1.1.
**Owner:** *unassigned* — design owner needed before promotion.

## Summary

Replace the v0.1..v1.0 sentinel lowering of anonymous error unions
(`T!{A, B}` → `Result[T, Error]` with a poison `Error` that unifies
permissively with any concrete error) with **first-class union ADTs**:
an algebraic datatype whose variants are unnamed and whose payload
types are structural rather than nominal. Allow `enum Foo { ... }`-style
syntax to declare ordinary nominal sums *and* to participate in
inference with anonymous unions.

This RFC concerns the type system; runtime representation, codegen,
and the v0.4 plain-call macro form are out of scope.

## Motivation

The slice-3 stopgap (A11) was a pragmatic concession to keep the
canonical examples compiling without forcing a union ADT in the slice-3
typeck. Five years on, the sentinel:

1. **Loses precision.** `Page!{NetErr, ParseErr}` does not carry the
   variants through inference; downstream code that wants to
   pattern-match on the specific error kind must convert via an
   intermediate nominal enum or rely on `dyn Error`.
2. **Hides bugs.** Because the poison `Error` unifies permissively,
   `expr?` chains can mask a type-error that should have been a
   compile-time failure.
3. **Blocks `match` exhaustiveness.** The anonymous union has no
   variant set known to the checker, so `match e { ... }` cannot fire
   MT2015 against an incomplete arm list.
4. **Forces synthetic nominal enums in user code.** Every non-trivial
   error path in v1.0 stdlib code defines a one-off `enum FooErr`,
   inflating LOC and obscuring intent.

A first-class union ADT closes all four issues with a single change.

## Detailed design

### Surface syntax

Two complementary constructs:

```mty
// 1. Nominal sum (existing-style; first-class in v1.1):
enum HttpErr {
  Timeout,
  BadStatus(I32),
  Body(ParseErr),
}

// 2. Anonymous union (replaces A11 sentinel):
fn fetch(u: Url) -> Page!{NetErr, ParseErr} { ... }
//                       ^^^^^^^^^^^^^^^^^^ anonymous union
```

The anonymous form `T!{A, B}` desugars to a synthetic ADT with the
deterministic name `__Union_{hash}` where `hash` is the FNV-1a digest
of the variant list, sorted alphabetically. Two anonymous unions over
the same variant set unify by construction.

### Subsumption and conversion

- A nominal enum is **never** subsumed by an anonymous union (and
  vice versa); coercion is explicit via `expr.into()` or a `From` impl.
- Two anonymous unions are equivalent iff their variant sets match
  set-equally (`A!{X, Y}` ≡ `A!{Y, X}`).
- Anonymous union narrowing: `A!{X, Y} -> A!{X}` by `match` collapse
  is sound; widening (`A!{X} -> A!{X, Y}`) is sound and free.

### Pattern matching

`match` on a union ADT requires every variant to be covered (or `_`).
The new MT2015 fire-condition uses the *declared* variant set; for an
anonymous union, that's the desugared synthetic ADT's variants.

```mty
let r: Page!{NetErr, ParseErr} = fetch(u)?
match r {
  Ok(p)            => render(p),
  Err(NetErr(e))   => retry(e),
  Err(ParseErr(e)) => bail(e),    // exhaustive — no MT2015
}
```

### Question-mark propagation

`expr?` against an anonymous union widens the enclosing function's
return-error union to include every variant in `expr`'s union. If the
enclosing fn's declared error type is a *narrower* anonymous union,
emit a new diagnostic `MT2030 question_widens_anon_union`.

If the enclosing fn's declared error is a nominal enum, emit the
existing `MT2011 question_error_mismatch` (unchanged).

### Lowering

The HIR gains `TyData::Union(UnionId)` parallel to `TyData::Adt(AdtId)`.
The desugar pass interns each anonymous variant set as a `UnionId`
and registers it in `DefMap::synthetic_unions`. Borrow check and
codegen treat unions as ordinary tagged ADTs with the variant tags
assigned in alphabetical order at desugar time.

### Migration from A11

The v1.0 spec section §17.2 currently states:

> Anonymous error unions `T!{A, B}` lower to `Result[T, Error]` where
> `Error` is the poison sentinel that unifies permissively with any
> concrete error.

After v1.1 promotion, replace with:

> Anonymous error unions `T!{A, B}` lower to a synthetic union ADT
> with deterministic name and alphabetically-sorted variants. See
> RFC-001.

Existing v1.0 source that relied on the permissive unification (e.g.
`expr?` widening through an opaque error path) will need either an
explicit `into()` conversion or to declare the wider union in the
enclosing fn's signature.

## Drawbacks

- **Migration churn.** v1.0 code that leaned on the permissive
  unification will need updates; LSP `MT2011` quick-fix gains a
  union-widening offer to soften this.
- **Synthetic-ADT explosion.** Every distinct variant set creates an
  interned ADT. Anecdotal slice-3 telemetry suggested fewer than 200
  distinct anonymous unions across the dogfood corpus; this is small
  but warrants a check during v1.1 implementation.
- **Hash collision risk.** FNV-1a over the variant list is collision-
  resistant enough for source-level use, but pathological projects
  could theoretically clash. Mitigation: append a counter suffix if a
  collision is ever observed; emit `MT2031` (warning) to surface it.

## Alternatives considered

1. **Keep the sentinel forever.** Maintains v1.0 ergonomics but
   forfeits exhaustiveness and bug-detection. Rejected: the slice-3
   stopgap was always going to need a real implementation.
2. **Anonymous unions as `dyn Error`.** Cheap to implement but loses
   variant-level matching entirely. Rejected: defeats the motivation.
3. **Row-polymorphic error rows (à la OCaml polymorphic variants).**
   More expressive but requires inference machinery v1.0 does not have
   (presence/absence variables). Reconsider in v2.0.
4. **Nominal-only — remove `T!{A, B}` syntax.** Forces every error
   path into a named enum. Rejected as a regression against the
   existing dogfood ergonomic.

## Unresolved questions

- Should anonymous unions be allowed in **value** position (not just
  error position)? `Json!{Str, I32}` as a payload type is tempting but
  conflates the `!` punctuation; might need a different sigil.
- How does this interact with the v1.1+ trait coherence overlap
  detection (RFC-future)? An anonymous union's `impl Trait` would need
  variant-set-aware coherence rules.
- Default representation: stable on-disk tag layout (the union's
  variant tag is part of its identity), or implementation-defined
  pending performance work?

## Adoption plan

1. **v1.1-alpha.1:** typeck adds `TyData::Union`, desugar pass interns
   anonymous unions, MT2015 fires on incomplete match arms.
2. **v1.1-alpha.2:** widening diagnostic MT2030, LSP quick-fix.
3. **v1.1-beta:** runtime/codegen support; conformance tests under
   `tests/conformance/types/union/`.
4. **v1.1.0:** A11 is reclassified SUPERSEDED → A11.b in
   `v0.1-amendments.md`; v1.1-RC spec replaces §17.2 with the union
   ADT description; LSP cross-file rename gains union-variant
   awareness.

A 30-day public comment window opens with v1.1-alpha.1.
