# Borrow checker v0.3 — interpretation notes

This file captures the design choices made while hardening
`sdust-borrow` from the slice-4 lexical/whole-local checker to v0.3's
NLL-lite + field-level + precise-MT3009 implementation. New amendments:
**A54 (field places), A55 (NLL last-use), A56 (MT3009 precise)**.

## What's in v0.3

### A54 — Place algebra (field-level borrows)

A `Place` is a rooted projection path:

```text
Place := { root: String, projs: Vec<Proj> }
Proj  := Field(name) | Index | Deref
```

Two places **overlap** iff one is a prefix of the other. The
`BorrowLedger` (`state::BorrowLedger`) holds active `BorrowRecord`s
keyed by Place. A new borrow conflicts iff its Place overlaps any
existing record's Place. Disjoint fields (`s.a` vs `s.b`) coexist.

**Truncation rule.** v0.3 keeps at most ONE projection step in any
Place. Deeper paths (`s.a.b.c`) are truncated to `s.a` for conflict
purposes. This is conservative (rejects programs Rust would accept)
but sound. v0.4 will lift the truncation.

**Indices.** `arr[i]` and `arr[j]` are treated as the same place
(`Place::root(arr).with_index()` — single `Index` edge). Slicewise
disjointness needs the integer-index analysis that's out of scope
for v0.3.

### A55 — NLL last-use

Per-fn we run a `nll::Pre` pre-pass that walks the typed HIR in source
order, assigning a monotone `ProgramPoint` to every `Path` expression
and recording the **highest** point at which each local name appears
(`LastUseMap`).

The main walker maintains `current_point` in lockstep with the pre-pass
(both visit Path expressions in identical order with the same counter
increments). After each Path use of `name`, the walker calls
`maybe_decay_after_use(name)`: if `pt_just_used >= last_use[name]`, all
ledger records whose `borrower == name` are removed, and the root
local's `Ownership::Borrowed*` state is recomputed from the surviving
records.

**Approximation level.** This is a hand-rolled "linear NLL" — *not*
polonius. It correctly handles the canonical `let r = &x; use(r); let
m = &mut x` pattern and chains. It does NOT handle:

- Two-phase borrows (`vec.push(vec.len())`).
- Borrow that ends only on one branch of a diamond (the join is
  conservative — see `join_ledgers`).
- Conditional borrows flowing through a loop back-edge.

These are flagged for v0.4.

**Branch joins.** `if`/`if let`/`match` snapshot the ledger before each
arm and union the per-arm ledgers afterwards (`join_ledgers`). This is
conservative: a borrow held on only one arm is conservatively still
live after the join. Refining this requires control-flow-sensitive
reasoning and is post-v0.3.

### A56 — Precise MT3009 (move out of reference)

`*ref` of a non-Copy type is fundamentally unsound — references don't
own their pointee. v0.3 implements:

- `HirExpr::Unary { op: Deref, rhs }` in `Position::Use`: if
  `expr_ty[rhs] = Ref { inner }` and `inner` is non-Copy, emit MT3009.
- `HirExpr::Move(Unary { op: Deref, rhs })`: same check.
- If `inner` IS Copy, the deref is a load (no MT3009).

The message names the reference (`cannot move out of *r: ...`) and
the diagnostic distinguishes MT3009 from MT3001 (use-after-move) and
MT3008 (move-of-borrowed).

## Backwards-compat with slice 4

Existing slice-4 conformance cases (`01..04`) still hold:

- `01_mut_while_shared`: `r` is borrower of `&a`; `r`'s last-use is
  at `use_ref(r)` (after the conflicting `&mut a`), so the borrow is
  STILL live at the conflict point → MT3004 fires.
- `02_shared_while_mut`: symmetric — MT3005 fires.
- `03_two_mut_borrows`: symmetric — MT3006 fires.
- `04_mut_borrow_of_immut_local`: MT3013 fires unconditionally in
  `try_place_borrow`.

No expected_diagnostics changes were needed for the existing 4 cases.

## Soundness gaps deferred to v0.4

The following are *known-permissive* in v0.3. Documented here rather
than silently accepted:

- **Two-phase borrows**: `vec.push(vec.len())` is overly conservative
  (rejects what Rust accepts). Not a soundness gap, just over-restriction.
- **Deeper field paths**: `s.a.b` truncates to `s.a`; we lose the
  fine-grained disjointness of nested fields.
- **Index-aware disjointness**: `arr[0]` and `arr[1]` are conflated.
- **Loop back-edge borrows**: a borrow taken in iteration N and used
  in iteration N+1 isn't modelled; the conservative scope-end decay
  catches obvious leaks but not the subtle ones.
- **Cross-fn region inference**: there are no explicit lifetime
  parameters in Stardust yet; fn signatures with two `&T` params that
  must outlive each other can't be expressed. Likely punted to v0.5+.
- **Move-out-of-deref-of-deref**: `move **ref_ref` — the deeper deref
  case is detected as deref-of-something-typed-`&&T`, and v0.3's
  `check_deref_move` looks one step. Tightening needs proper Place
  ownership analysis.

## Conservative ledger join (over-restriction)

The ledger join after `if`/`match` unions records from all arms. This
means a borrow held on only ONE arm becomes "potentially still live"
after the join. Example:

```rust
let r = if cond { Some(&a) } else { None };
let m = &mut a; // v0.3 errors: shared borrow possibly live
```

Rust's NLL would accept this if `r` isn't used in the `Some` arm
afterwards. v0.4 ticket.

## Where to look in the source

| Concern               | File                                       |
|-----------------------|--------------------------------------------|
| Place algebra         | `crates/sdust-borrow/src/place.rs`         |
| Last-use pre-pass     | `crates/sdust-borrow/src/nll.rs`           |
| Borrow ledger         | `crates/sdust-borrow/src/state.rs` (`BorrowLedger`) |
| Place-aware borrow check | `flow.rs::try_place_borrow`             |
| MT3009 detector       | `flow.rs::check_deref_move`                |
| NLL decay hook        | `flow.rs::maybe_decay_after_use`           |
| Ledger join (branches) | `flow.rs::join_ledgers`                   |

## Tests added

- `crates/sdust-borrow/tests/nll_last_use.rs` (3 cases)
- `crates/sdust-borrow/tests/field_disjoint.rs` (2 cases)
- `crates/sdust-borrow/tests/field_overlap.rs` (2 cases)
- `crates/sdust-borrow/tests/sd3009_move_via_ref.rs` (3 cases)
- `tests/conformance/borrow_checking/05_nll_last_use/`
- `tests/conformance/borrow_checking/06_field_disjoint/`
- `tests/conformance/borrow_checking/07_field_overlap/`
- `tests/conformance/borrow_checking/08_move_via_ref/`
- `tests/conformance/borrow_checking/09_move_via_ref_copy/`

Plus `place.rs` and `state.rs` carry unit tests for the new types.

## Amendments

Amendments A54/A55/A56 in `docs/spec/v0.1-amendments.md`. Internal
algorithm doc in `docs/internals/borrowck.md` §17–19. Formal-ish spec
in `docs/spec/borrow-model-v0.3.md`.

## Concurrency notes

This change set was authored by the borrow-checker swarm agent. It
touches ONLY:

- `crates/sdust-borrow/**`
- `docs/internals/borrowck.md`
- `docs/tour/14-ownership.md`
- `docs/spec/borrow-model-v0.3.md` (new)
- `docs/spec/v0.1-amendments.md` (appended A54/A55/A56)
- `tests/conformance/borrow_checking/**` (new cases only)
- This file (`BORROW_V0_3_NOTES.md`)

Per the swarm protocol, no other crates were modified.
