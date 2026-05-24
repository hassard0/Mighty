# Borrow Model — v0.3

Formal-ish specification of Stardust's borrow checker as of v0.3.
Supersedes the slice-4 description in `docs/internals/borrowck.md`
sections 1–10, which remain accurate but are now augmented by
sections 17–19 covering A54/A55/A56.

This document is the load-bearing reference for spec §7.2/§7.3 in
v0.3.

## 1. Vocabulary

- **Local** — a named binding introduced by `let`, fn parameter,
  pattern destructuring, or handler parameter.
- **Place** — a memory location identified by `(root, projs)` where
  `root` is a local name and `projs` is a possibly-empty sequence of
  projection steps. A bare local is `Place { root, projs: [] }`.
- **Projection** — `Field(name)` for `.field`, `Index` for `[_]`,
  `Deref` for `*`.
- **Overlap** — two Places `p` and `q` overlap iff `p` is a prefix
  of `q` or `q` is a prefix of `p`. Prefix is structural (same root
  AND same leading projections).
- **Borrow** — a record `{ place, kind, borrower?, at }`. `kind` is
  `Shared` or `Mut`. `borrower` is the local binding holding the
  `&T` / `&mut T` value (if any).
- **BorrowLedger** — multiset of live borrows, ordered by creation.
- **ProgramPoint** — a monotone integer assigned to each `Path` read
  in a fn body's source-order traversal.
- **LastUseMap** — for each local name, the highest `ProgramPoint`
  at which it is referenced.

## 2. Aliasing-XOR-mutation, Place-level

A new borrow `B = { place, kind }` is admissible iff:

```text
for each existing live borrow E in the ledger:
    if E.place overlaps B.place:
        (B.kind == Shared) and (E.kind == Shared)
```

Otherwise, emit:

| New kind | Existing has &mut? | Existing has &? | Diagnostic |
|----------|--------------------|-----------------|------------|
| Mut      | yes                | (any)           | SD3006     |
| Mut      | no                 | yes             | SD3004     |
| Shared   | yes                | (any)           | SD3005     |
| Shared   | no                 | yes             | (admissible) |

When the conflict spans projection paths (non-empty `projs`), the
diagnostic message uses the `Place::Display` form (e.g. `s.a`) instead
of the bare root name.

## 3. Place truncation (v0.3 conservative bound)

Before insertion into the ledger, every Place is **truncated** to
**at most one projection step**. Deeper paths are folded to their
length-1 prefix. Concretely, `&mut s.a.b` is recorded as `&mut s.a`,
and therefore conflicts with `&mut s.a.x` (correctly) and with `&mut s`
(correctly) and ALSO with `&mut s.a.y` (over-conservative — a sound
v0.4 extension will accept).

Rationale: depth-1 fields cover the dominant disjoint-field idiom
without committing to a full Place lattice. Truncation is monotone
(always shortens), preserving soundness.

`Index` collapses to a single edge — `arr[0]` and `arr[1]` conflict.
v0.3 doesn't attempt integer-aware disjointness.

## 4. Move ownership semantics

A move out of `Place(root, [])` (a bare local) transitions
`root.state` from `Owned` to `Moved`. Subsequent reads/borrows of
`root` error:

| Operation            | On Owned    | On Moved          |
|----------------------|-------------|-------------------|
| Use                  | OK          | SD3001            |
| Move                 | →Moved      | SD3001            |
| BorrowShared         | →ledger Shared | SD3003         |
| BorrowMut            | →ledger Mut OR SD3013 | SD3003 |
| AssignTarget         | (no-op for Owned) → re-init Owned | →Owned (re-init) |

A move out of `Place(root, projs)` with non-empty projs is more subtle.
v0.3 treats it identically to a move out of `root` (the whole local),
because partial-move tracking adds substantial bookkeeping (Rust's
`MovePathIndex`). This is sound (more restrictive) and avoids
soundness gaps; v0.4 will partial-move.

## 5. Borrow deactivation: NLL last-use (A55)

A borrow `B = { place, kind, borrower, at }` deactivates when:

- (Scope-end) The borrower binding `borrower` exits its lexical
  scope, OR
- (Last-use) The walker reads `borrower` at program point `p` and
  `p >= LastUseMap[borrower]`.

When deactivation fires, `B` is removed from the ledger and
`root.state` is recomputed from the surviving records:

```text
shared_count = | { E in ledger : E.place.root == root, E.kind == Shared } |
has_mut      = exists E in ledger : E.place.root == root, E.kind == Mut

if has_mut:           root.state = BorrowedMut
elif shared_count>0:  root.state = Borrowed { count: shared_count }
else:                 root.state = Owned    (if not Moved / Uninit)
```

A borrow with `borrower = None` (anonymous, e.g. `use_ref(&x)` where
`&x` is a temporary arg) only deactivates on scope-end.

## 6. Pre-pass: computing LastUseMap

Per fn body, a syntax-directed visitor walks every expression in
source order, advancing a monotone counter on each Path / PathGeneric
expression. Each visit records `name → max(point)`.

The main walker mimics the pre-pass exactly: it advances the same
counter on the same Path expressions in the same order. This ensures
the walker's `current_point - 1` after a Path read equals the
`ProgramPoint` the pre-pass assigned to that read, so the
`>= LastUseMap[name]` comparison is well-defined.

## 7. Branch handling

For `if`/`if let`/`match`, before each arm we snapshot
`(locals, ledger)`. After each arm we capture
`(locals_arm_i, ledger_arm_i)`. After all arms, we **join**:

- `locals` join: per-variable state-intersection (see
  `state::join_states`). Moved wins; Uninit second; BorrowedMut third;
  Borrowed{n} takes max; otherwise Owned.
- `ledger` join: UNION of all arm ledgers, de-duplicated by
  `(place, kind, borrower)`. Conservative — a borrow held on ONE arm
  remains live after the join.

The conservative ledger join is a documented over-restriction. v0.4
will refine via control-flow-sensitive last-use within branches.

## 8. SD3009 — move out of reference (A56)

For each occurrence of `*ref` where `ref` is a path expression with
`expr_ty[ref] = Ref { inner: T, .. }`:

- If T is Copy (per `is_copy(T, ...)`), `*ref` is a load → admissible.
- If T is non-Copy, `*ref` in `Position::Use` or as the operand of
  `move` → emit SD3009.

The diagnostic names the reference (`*r`) so the user can find the
exact deref site. SD3009 is distinct from SD3001 (use-after-move on
a moved local) and SD3008 (move out of a borrowed local).

## 9. Soundness gaps deferred to v0.4

- Two-phase borrows (`vec.push(vec.len())`).
- Place truncation: deeper field paths fold to depth-1.
- Index-aware disjointness.
- Conditional borrows on one branch only (ledger over-joins).
- Loop back-edge borrows.
- Partial moves (`let User { a, b } = u; use(a); use(u.b)`).
- Cross-fn lifetime parameters (no `'a` syntax yet).

Each is a *known over-restriction*, not a *soundness hole* —
the v0.3 checker errs on the side of rejecting safe programs.
The single known soundness consideration is the `move *ref` case,
which v0.3 fixes (A56).

## 10. Source map

| Concern                       | File                                       |
|-------------------------------|--------------------------------------------|
| Place algebra                 | `crates/sdust-borrow/src/place.rs`         |
| NLL pre-pass / LastUseMap     | `crates/sdust-borrow/src/nll.rs`           |
| Borrow ledger                 | `crates/sdust-borrow/src/state.rs`         |
| Conflict detection            | `flow.rs::try_place_borrow`                |
| SD3009 detection              | `flow.rs::check_deref_move`                |
| Decay hook                    | `flow.rs::maybe_decay_after_use`           |
| Branch / loop joins           | `flow.rs::join_ledgers`                    |
| Conformance cases             | `tests/conformance/borrow_checking/05..09` |
