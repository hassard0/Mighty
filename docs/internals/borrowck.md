# Borrow Checker Internals (Slice 4 + v0.3 hardening)

Mighty's ownership / borrow / affine / arena analysis lives in the
**`mty-borrow`** crate. It runs after `mty-types` and consumes the
typed-HIR side tables produced by `sdust_types::check_package_typed`.

This document mirrors `docs/internals/typeck.md` and is the load-bearing
reference for slice 4's design choices. Sections 1–16 describe the
slice-4 baseline; sections 17–19 cover the v0.3 hardening (A54/A55/A56,
see also `docs/spec/borrow-model-v0.3.md`).

## 1. Place in the pipeline

```
mty-syntax → mty-ast → mty-hir → mty-types → mty-borrow
```

`mty-borrow` only runs if `mty-types` produced no Error-severity
diagnostics (warnings, such as MT2026 `protocol_msg_unknown`, are
tolerated). The driver wires this up via
`sdust_driver::type_and_borrow_check(pkg)`.

The CLI calls the combined stage:

```
mty check foo.sd  →  lex → parse → lower → typeck → borrowck
```

## 2. Inputs

`TypedPackage` carries:

- `def_map: DefMap` — name resolution table (ADTs, fns, modules, params).
- `ty_arena: TyArena` — interned types.
- `expr_ty: HashMap<ExprId, TyId>` — resolved type of every expression
  (post-defaulting; `IntInfer→I32`, `FloatInfer→F64`).
- `fn_params: HashMap<FnId, Vec<(String, TyId)>>` — per-fn parameter list.
- `fn_ret: HashMap<FnId, TyId>` — per-fn declared return type.
- `diagnostics: Vec<Diagnostic>` — typeck diagnostics.

The borrow checker reads `expr_ty` heavily (to classify calls' arg-pos
moves vs borrows) and `fn_params` (to seed the per-fn local table).

## 3. State machine

Each tracked local has an `Ownership` state:

| State          | Meaning                                                    |
|----------------|------------------------------------------------------------|
| `Owned`        | The local owns its value (Copy values stay Owned forever)  |
| `Moved { at }` | Value has been moved; reads/borrows error (MT3001/MT3003)  |
| `Borrowed{n}`  | `n` live shared borrows; reads OK, mut-borrow forbidden    |
| `BorrowedMut`  | One live mutable borrow; both reads and shared-borrow forbid|
| `Uninit`       | `let x;` without init; first read errors MT3015            |

State transitions are driven by the **position** the local appears in:

| Position         | On Owned    | On Moved | On Borrowed | On BorrowedMut |
|------------------|-------------|----------|-------------|-----------------|
| `Use`            | (no change) | MT3001   | (read OK)   | (read OK)       |
| `Move`           | →Moved (if !Copy) | MT3001 | MT3008    | MT3008          |
| `BorrowShared`   | →Borrowed{1}| MT3003   | →Borrowed{n+1} | MT3005      |
| `BorrowMut`      | →BorrowedMut OR MT3013 | MT3003 | MT3004 | MT3006        |
| `AssignTarget`   | →Owned OR MT3014 | →Owned OR MT3014 | (same) | (same) |

Copy values never transition out of Owned: `is_copy()` returning true
makes Move into a no-op.

## 4. Copy rule

`crates/mty-borrow/src/copy.rs::is_copy(ty, arena, defs) -> bool`:

- Primitives (Bool, Int*, Float*, Char, Unit, Duration, Size) — yes
- `&T` (shared ref) — yes; `&mut T` — no
- `*T` raw pointer — yes
- `Str` (string slice) — yes; `String`/`Bytes` (owning) — no
- Tuples / arrays — yes iff every element is Copy
- Function pointers — yes
- Opaque ADT (prelude `Url`, `Page`, agent types, etc.) — **yes**
  (BOLD slice-4 decision to keep examples compiling; will tighten via
  per-type `derive(Copy)` in slice 5)
- User struct/enum — no
- `Param(T)` / `Var(?n)` — no (conservative)
- `Never`, `Error`, `Module` — yes (degenerate)

## 5. Sendable rule (cross-agent)

`sendable.rs::is_sendable(ty, arena, defs) -> bool`:

- All references / raw pointers / fn pointers — **no** (rejected first,
  even though refs are Copy)
- Copy types (other than the above) — yes
- Owned `String` / `Bytes` — yes (move semantics across boundary)
- Tuples / arrays — yes iff every element is Sendable (recurse before
  Copy short-circuit)
- Opaque ADT — yes
- User struct/enum — yes iff every variant payload is Sendable
- `Param(T)` / `Var(?n)` — yes (conservative permissive; slice-5 trait
  bounds tighten)

A `!Msg(args)` / `?Msg(args)` call site checks every arg's expr type
against this predicate; failures emit MT3011.

## 6. Linear walk

The walker is **lexical** and **linear** — no CFG, no fixpoint. State
mutates as the walk proceeds:

- `walk_block` pushes a scope frame, walks every statement, walks the
  block's tail expression, pops the frame. On pop, every Owned non-Copy
  local in the frame emits a `DropEntry` into `DropPlan`.
- `walk_stmt(Let)` walks the init, reads its type from `expr_ty`, then
  binds pattern locals with that type. `let mut <pat>` flows the mutability
  through to bound locals.
- `walk_expr` is a giant match. Highlights:
  - `Path([name])` → consults `position` to do `do_use` / `do_move` /
    `do_borrow_shared` / `do_borrow_mut` / `do_assign`.
  - `Borrow { mutable, inner }` → walks inner with the corresponding
    borrow Position.
  - `Move(inner)` → walks inner with Position::Move.
  - `Call { callee, args }` → walks callee, then each arg with a Position
    derived from the callee's resolved `Fn { params, .. }` type:
    `Ref { mutable, .. }` parameters borrow, others move.
  - `If` / `IfLet` / `Match` → snapshot locals before the branch arms,
    join via `state::join_states` (intersection) after.
  - `Arena { body, .. }` → push a fresh `ArenaRegionId`; locals declared
    in the body carry the region; at body end, if the tail expression's
    root name is an arena-local non-Copy binding, emit MT3010.
  - `Unsafe` / `Sandbox` / `Budget` → just walk (the type checker's
    scope-aware tolerance is what relaxes name resolution).
  - `Spawn` / `Lambda` / `TaskScope` → walk children; lambdas open a fresh
    frame and the captured-locals snapshot is restored on exit (slice 4
    does not enforce affine capture).

## 7. Borrow region: lexical only

A borrow's region is the **innermost enclosing block** of the borrow
expression. When that block ends, every borrow it introduced "decays" —
in practice the walker pops the local state of the borrower (which was a
scope-local binding holding the `&T` / `&mut T` value), so the borrow
count on the source local naturally decrements via the scope-end clean-up.

This is conservative; some programs Rust's NLL accepts may error here.
NLL / Polonius is post-v0.1.

## 8. Arena region tracking

`arena_region.rs::ArenaCounter` issues monotone region ids. The walker's
`ScopeFrame::arena_region` holds the active region (if any). `bind_local`
captures the current arena region into the new local's `arena_region`
field.

`walk_expr(HirExpr::Arena)` does:

1. Push a frame with a fresh region id.
2. Walk the body. If the body is a `Block`, we walk its statements and
   tail directly (not through `walk_block`) so the arena frame is still
   active when we inspect the tail.
3. If the tail expression's "root name" (returned by `walk_expr`) names a
   local owned in the active region with `!is_copy`, emit MT3010.
4. Pop the frame.

The MVP only flags **direct naming** (`arena turn { let x = ...; x }`).
Transitive flow (e.g. a fn returning the arena-local through a deref) is
post-v0.1.

## 9. Drop intent

At each scope-pop, `pop_frame` walks the frame's local names and emits a
`DropEntry { local_name, span }` for each Owned non-Copy local. Slice 4
does not codegen the drops (no MtyIR yet); the `DropPlan` is held internally
and discarded by `flow::run`. Future codegen consumers can re-thread it.

## 10. Diagnostics

| Code   | Name                          | Severity | Origin                              |
|--------|-------------------------------|----------|-------------------------------------|
| MT3001 | use_after_move                | Error    | do_use / do_move on Moved           |
| MT3002 | move_out_of_borrow            | Error    | (reserved — slice 4 uses MT3008)    |
| MT3003 | borrow_after_move             | Error    | do_borrow on Moved                  |
| MT3004 | mut_borrow_while_shared       | Error    | do_borrow_mut on Borrowed{n}        |
| MT3005 | shared_borrow_while_mut       | Error    | do_borrow_shared on BorrowedMut     |
| MT3006 | two_mut_borrows               | Error    | do_borrow_mut on BorrowedMut        |
| MT3007 | borrow_outlives_owner         | Error    | (reserved; lexical regions can't trigger) |
| MT3008 | cannot_move_borrowed          | Error    | do_move on Borrowed / BorrowedMut   |
| MT3009 | move_out_of_ref               | Error    | (reserved; needs deref-move modelling) |
| MT3010 | arena_escape                  | Error    | Arena tail names arena-local        |
| MT3011 | non_sendable_message_arg      | Error    | Send/Ask arg fails is_sendable      |
| MT3012 | drop_in_const_context         | Error    | (reserved)                          |
| MT3013 | mut_borrow_of_immut_local     | Error    | do_borrow_mut on !mutable Owned     |
| MT3014 | assign_to_immut_local         | Error    | do_assign on !mutable               |
| MT3015 | use_of_uninitialized          | Error    | do_use on Uninit                    |

Plus the typeck warning added by slice 4:

| MT2026 | protocol_msg_unknown          | Warning  | Agent handler msg not in protocol   |

## 11. Scope-aware tolerance (slice-3 hardening)

The slice-3 permissive "any unknown name → fresh inference var" policy
was tightened. Now unresolved single-segment value names emit
`MT2021 unresolved_value` **unless** the name appears in the per-body
tolerance set, or the body is in `tolerance_open` mode:

- **Open** (tolerance_open=true): inside `unsafe` blocks, `sandbox … with`
  bodies, `budget` bodies, `arena` bodies, supervisor child expressions,
  and extern block contents.
- **Set** (tolerance contains name): inside agent bodies (handlers, state
  initializers, methods), the agent's state names, ctor-param names, and
  sibling method names are tolerated.

Top-level fn bodies have an empty tolerance set — they must resolve every
name through locals/params or the prelude.

## 12. Method dispatch (slice-3 hardening)

Method resolution on a `Adt(aid, _)` receiver:

1. If `(aid, method)` is in `def_map.impl_methods` (built from
   `HirImpl` blocks at def-map construction), use that impl's signature
   directly. Bind generic args via the receiver's `adt_args` plus fresh
   vars for method-level generics.
2. Else if the ADT is **opaque** (prelude `Url`/`Logger`/agent types,
   etc.), fall through to the built-in method table (permissive
   variadic / fresh-return).
3. Else (user struct/enum), emit `MT2007 unknown_method`.

Non-ADT receivers (primitives, refs, raw ptrs) keep using the slice-3
built-in table plus shape specials (`.len` on arrays / Str / String /
Bytes returns USize).

## 13. Protocol-aware handler param types (slice-3 hardening)

`build_def_map` indexes every protocol's messages into
`def_map.protocol_msgs: HashMap<(ProtocolName, MsgName), Vec<TyId>>`.

When checking an agent handler `on Msg(p1, p2)`, the type checker
searches every implemented protocol (from the agent's `protocols`
HirType list) for a matching message. If found, handler params bind to
the protocol-declared types; else the params fall back to fresh
inference variables and an `MT2026 protocol_msg_unknown` warning is
emitted.

## 14. Integer/float defaulting pass

After each fn body is checked, `items.rs::default_ty(t, subst, arena)`
walks the resolved type, rewriting `Int(IntInfer)` → `I32` and
`Float(FloatInfer)` → `F64`. The rewritten type is stored into the
side-table `expr_ty` / `fn_params` / `fn_ret`. Downstream consumers
(the borrow checker, future codegen) see concrete primitives.

## 15. Future work

Tracked as out-of-scope for slice 4:

- Non-lexical lifetimes / Polonius — post-v0.1
- Cross-function lifetime inference + explicit lifetime parameters —
  post-v0.1
- Field-level borrow tracking (splitting `&struct.field` from
  `&struct.other_field`) — post-v0.1
- Drop ordering across reordered scopes / drop-flag bookkeeping — slice 5+
- Manual `derive(Copy)` on user ADTs — slice 5
- Real serializable-shape audit for cross-agent messages — slice 6
- Trait coherence + dyn dispatch — slice 5
- Effect closure + capability narrowing — slice 5
- `move *ref` modelling for MT3009 — slice 5
- Tighter MT3002 vs MT3008 distinction — slice 5
- Real codegen of drop() calls + MtyIR consumption of DropPlan — slice 6+

## 16. Source map

- `crates/mty-borrow/src/lib.rs` — entry points (`check_package`,
  `type_and_borrow_check`)
- `crates/mty-borrow/src/state.rs` — `Ownership`, `LocalState`,
  `ScopeFrame`, `join_states`, `BorrowLedger`/`BorrowRecord` (v0.3)
- `crates/mty-borrow/src/place.rs` — `Place`/`Proj` algebra (v0.3)
- `crates/mty-borrow/src/nll.rs` — `LastUseMap`/`compute_last_use` (v0.3)
- `crates/mty-borrow/src/copy.rs` — `is_copy`
- `crates/mty-borrow/src/sendable.rs` — `is_sendable`
- `crates/mty-borrow/src/flow.rs` — `BorrowCx`, walker, per-position
  state updates, `try_place_borrow`, `check_deref_move`
- `crates/mty-borrow/src/arena_region.rs` — `ArenaCounter`
- `crates/mty-borrow/src/drop_plan.rs` — `DropPlan`, `DropEntry`
- `crates/mty-borrow/src/diag.rs` — SD3xxx constructors
- `crates/mty-diagnostics/src/codes.rs` — SD3xxx code + explain text
- `crates/mty-types/src/items.rs` — typed-side-table emission,
  defaulting pass, tolerance-set construction, protocol-aware handler
  param binding
- `crates/mty-types/src/check.rs` — scope-aware `synth_path`, real
  method dispatch
- `crates/mty-types/src/resolve.rs` — `impl_methods` + `protocol_msgs`
  indexing

## 17. v0.3 / A54 — Place algebra and field-level borrows

The slice-4 checker stored a single `Ownership` per local. v0.3 adds
`BorrowLedger` — a positional list of `BorrowRecord { place, kind,
borrower, at }` — and tracks borrows at **Place** granularity:

```text
Place := { root: String, projs: Vec<Proj> }
Proj  := Field(name) | Index | Deref
```

Two Places overlap iff one is a structural prefix of the other.
Disjoint fields (`s.a` and `s.b`) coexist; overlapping projections
conflict per the slice-4 MT3004/3005/3006 rules.

`flow.rs::expr_as_place` maps a `HirExpr` to an `Option<Place>`,
peeling `Path`, `Field`, `Unary{Deref}`, `Index`. v0.3 **truncates**
the Place to depth 1 (`Place::truncate_for_v0_3`) before insertion —
deeper paths fold to their first projection. Sound but
over-conservative; v0.4 will lift.

The borrow ledger maintains the legacy `Ownership::Borrowed*` state on
the root local in lockstep, so existing diagnostic pathways and the
slice-4 conformance cases still trigger.

## 18. v0.3 / A55 — NLL last-use deactivation

Each fn body runs a pre-pass (`nll::compute_last_use`) that walks the
typed HIR in source order, assigning a monotone `ProgramPoint` to
every `Path` expression and recording the highest point at which each
name is referenced.

The main walker advances the same counter in the same order. After
each Path read of `name`, it calls `maybe_decay_after_use(name)`: if
the just-used point `>=` `LastUseMap[name]`, every ledger record
whose `borrower == name` is removed, and the root local's
`Borrowed*` state is recomputed from the surviving records.

The borrower binding is captured via a `pending_borrower` slot set
in `walk_stmt(HirStmt::Let)` before walking the init. The `Borrow {
.. }` handler stamps the new record with `pending_borrower.clone()`.

**Branch handling**: `if`/`if let`/`match` snapshot the ledger before
each arm and union the per-arm ledgers afterwards via `join_ledgers`.
This is conservative — a borrow held on one arm remains live after
the join.

What v0.3 deliberately doesn't do (deferred to v0.4):
- Two-phase borrows
- Loop back-edge borrow rotation
- Borrow held on one branch only
- Partial-move tracking on Place(root, [field, ..])

See `docs/spec/borrow-model-v0.3.md` for the formal algorithm and
`BORROW_V0_3_NOTES.md` for the rationale per design call.

## 19. v0.3 / A56 — Precise MT3009

`*ref` of a non-Copy type is fundamentally unsound (references don't
own). `flow.rs::check_deref_move` detects:

- `HirExpr::Unary { op: Deref, rhs }` in `Position::Use`
- `HirExpr::Move(Unary { op: Deref, rhs })`

If `expr_ty[rhs] = Ref { inner: T, .. }` and T is non-Copy, MT3009
fires with a named diagnostic (`cannot move out of *r: ...`). If T
is Copy, the deref is a load and no diagnostic is produced.

Trichotomy with neighbouring codes:

| Code   | Cause                                       |
|--------|---------------------------------------------|
| MT3001 | Use of a value already moved                |
| MT3008 | Move out of a currently-borrowed value      |
| MT3009 | Move via deref of a reference (non-Copy)    |

## 20. v0.5 / A82 — Loop back-edges

Slice 4 walked each loop body exactly once, leaving the borrow ledger
at the bottom of the body. That state is wrong for any borrow whose
constraints have to hold across the back-edge: a borrow opened in the
body and dropped before the next iteration could falsely conflict
with a re-take, and a borrow held past the back-edge could escape the
checker entirely.

`BorrowCx::loop_fixed_point(body_walker)` (called by `Loop`, `While`,
and `For` arms of `walk_expr`):

1. Snapshot the pre-body `locals` map + `BorrowLedger`.
2. Walk the body. Conservatively join the post-body state into the
   pre-body baseline using the same `join_states` + `join_ledgers`
   primitives that handle `if` / `match`. The ledger join keeps
   records present in EITHER branch (slice 4 §17).
3. Convergence: when `self.ledger.records.len()` is stable across two
   passes, return. The join is monotonic in the records, so equal
   counts mean equal joined ledgers.
4. Bounded at 16 iterations (a fail-safe so a divergent body still
   terminates analysis). After the cap, do one last pass + join and
   exit.

The combination of (a) the fixed-point and (b) the conservative join
means a borrow taken inside the body is "live across the back-edge"
for the duration of every subsequent iteration that the analysis
examines — which is sound, if occasionally over-restrictive. The
v0.6 plan is to add a finer-grained NLL-style liveness for borrow
records taken inside the body; for v0.5 the bounded join is enough
to support real loops without spinning the checker.

`break <value>` flows the value at `Position::Move`; `continue` is a
no-op for the walker — its only role is to truncate the body's
remaining statements at the MtyIR level, which the borrow check doesn't
need to see because the join already covers every possible post-body
state.

## 21. v0.21 — Polonius-flavoured second-pass borrow checker

`crates/mty-borrow/src/polonius.rs` ships a small Datalog-style
borrow checker that runs as an **opt-in second pass** behind the
`polonius` cargo feature:

```sh
cargo build  -p mty-borrow --features polonius
cargo test   -p mty-borrow --features polonius --test polonius
```

With the feature OFF, `check_package` is identical to v0.20 (NLL
only). With the feature ON, the union of NLL findings AND any
Polonius-only rejections (code MT3020) are returned.

### Fact model

Each loan `L` (call site, let-binding) produces a fact set. The
solver iterates rules over the fact relations to fixpoint:

| Relation                  | Meaning                                  |
|---------------------------|------------------------------------------|
| `BorrowAt(L, P)`          | loan `L` is live at point `P`            |
| `LoanInvalidated(L, P)`   | loan `L` is killed by an event at `P`    |
| `Subset(L1, L2, P)`       | `L1` outlives `L2` at `P`                |
| `Conflict(L1, L2, P)`     | two borrows of `L1`/`L2` collide at `P`  |
| `Error(L, P)`             | terminal: `L` invalidated while in use   |

Rules applied to fixpoint (bounded at 32 iterations):

1. **Subset transitivity.**
   `Subset(A, B, P) ∧ Subset(B, C, P) ⇒ Subset(A, C, P)`
2. **Invalidation while live.**
   `BorrowAt(L, Pb) ∧ LoanInvalidated(L, Pi) ∧ Pi ≥ Pb ⇒ Error(L, Pi)`
3. **(disabled in v0.21)** forward-flow. `BorrowAt(L, P) ∧ ¬Killed(L,P+1) ⇒ BorrowAt(L, P+1)`.
   The forward-flow rule would catch the canonical "live until killed" Polonius pattern but also regress the NLL last-use refinement. v0.21 ships the datalog scaffolding without it; v0.22 will gate it behind a `polonius-strict` cargo flag.
4. **Concurrent incompatible borrows.**
   `BorrowAt(L1, P) ∧ BorrowAt(L2, P) ∧ incompatible(L1, L2) ⇒ Conflict ∧ Error(L1, P)`.

### Loan compatibility

| L1            | L2            | Conflict? |
|---------------|---------------|-----------|
| Shared        | Shared        | no        |
| TwoPhaseMut   | Shared        | **no**    |
| Mut           | anything      | yes       |
| TwoPhaseMut   | TwoPhaseMut   | yes       |

`TwoPhaseMut + Shared` is the canonical two-phase accept pattern
(`vec.push(vec.len())`).

### When to enable

Enable `--features polonius` when:

- You want stricter borrow checking that catches a few classes of
  patterns NLL accepts incorrectly (nested conflict, conditional
  control-flow with invalidating moves).
- You're writing safety-critical code and want defence-in-depth.

The feature is OFF by default so v0.20 builds stay unchanged.
