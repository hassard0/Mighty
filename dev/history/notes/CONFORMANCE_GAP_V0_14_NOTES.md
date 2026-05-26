# Conformance Gap Closure — v0.14

Closes the remaining open rows in `KNOWN_ISSUES.md` item #11 (six
FROZEN typeck codes whose explain text was registered but whose
emit sites never landed). v0.12 closed four of the six (MT2009,
MT2022, MT2024, MT2025); v0.14 closes MT2003 and MT2023.

This file also captures one **deferred** carry-forward — the
`borrow_checking/14_borrow_outlives_owner` red-shirt — which the
v0.14 swarm cycle investigated and bounced back to v0.15 because
the actual fix lives outside the swarm agent's owned crates.

---

## What shipped

### MT2003 (Cannot infer type)

**Where**: `crates/mty-types/src/check.rs::check_stmt(HirStmt::Let)`,
gated on `declared.is_none()` and a new `is_cannot_infer_shape`
helper.

**Detection shape (v0.14)**: empty array literal `[]` as the
initializer of a `let` with no type annotation. The synth path
already returns `[Var(N)]` (an array of fresh-var elements); the
new helper resolves the array's element through the substitution
and fires MT2003 when the head is still `Var(_)`.

```mty
fn main() -> I32 {
  let xs = []     // MT2003: cannot infer type for binding `xs`
  42
}
```

**Why empty-array first**: it's the cleanest deterministic shape —
no usage-site inference can possibly constrain it, so the
diagnostic is stable across pipelines. The v1.x emit-landing plan
in `KNOWN_ISSUES.md` calls this row the "trait-iterator + collect
chain"; the empty-literal is the same family.

**Future shapes** (to extend without re-touching the funnel):

- empty map literal `{:}` / `Map()` constructor with no entries
- `Default()` calls at let-init with no annotation
- generic constructor with no arg-positional info

Each can land as a new arm in `is_cannot_infer_shape`.

### MT2023 (Generic argument-kind mismatch)

**Where**: `crates/mty-types/src/resolve.rs::resolve_def_to_ty`,
in the generic-arg `.iter().map(...)` loop for `DefRef::Adt`.

**Detection shape (v0.14)**: a single-segment generic argument
whose name resolves to a value-kind def (`DefRef::Fn` or
`DefRef::Variant`). Pre-v0.14 this funnelled through MT2002
("unresolved type") which mis-described the failure — the name
DOES resolve, just to the wrong kind.

```mty
fn helper() -> I32 { 0 }
fn bad() -> Result[helper, I32] { Ok(1) }   // MT2023
```

**Spec deviation**: the explain text for MT2023 uses lifetimes as
the canonical example. Mighty's v1.0 surface syntax has no
explicit lifetimes (regions are inferred), so the v0.14 emit-site
refines the rule to "value-kind in type-arg position". This keeps
the code-point and explain text spec-compliant ("the type
argument's kind does not match the expected parameter kind") while
giving users a real positive-fire on a shape that exists today.

If a future Mighty cycle adds explicit lifetimes, MT2023 will
naturally cover the lifetime case too — the diag fn takes a free-
form `arg_kind: &str` parameter so the call site can supply
"lifetime", "function", "variant constructor", etc.

### Conformance suite delta

| Metric             | v0.13 baseline | v0.14 shipped |
|--------------------|----------------|---------------|
| Cases ran          | 89             | **91**        |
| Categories         | 16             | 16            |
| Ignored            | 3              | 3             |
| `type_checking/*`  | 18             | **20**        |

Two new fixtures under `tests/conformance/type_checking/`:

- `03_cannot_infer_type/` — MT2003 positive-fire
- `21_generic_arg_kind_mismatch/` — MT2023 positive-fire

Five new unit tests in `crates/mty-types/tests/`:

- `mt2003_cannot_infer.rs` (3 tests: positive, with-annotation,
  non-empty)
- `mt2023_generic_arg_kind.rs` (2 tests: positive, with-type-arg)

---

## What was deferred

### `borrow_checking/14_borrow_outlives_owner` — bounces to v0.15

The fixture exercises this shape:

```mty
fn main() -> I32 {
  let outer = String("base")
  let mut r_out = &outer
  {
    let inner = String("inner")
    r_out = &inner          // reassign — owner `inner` is inside, borrower `r_out` is outside
  }
  use_ref(r_out)             // dangling — should fire MT3007
}
```

v0.12 added the MT3007 emit-site in `mty-borrow::flow::pop_frame`
and v0.12 also extended the `pending_borrower` stamping to plain
assignments in the `BinOp::Assign` branch of `walk_expr`. Both
pieces of wiring exist correctly today — the case still fails to
fire MT3007.

**Root cause** (found during v0.14 investigation, not in the
swarm-agent's owned crates): the HIR lowering pass drops the
inner block entirely.

`crates/mty-hir/src/lower/exprs.rs::is_expr_node` is the predicate
used to find the expression child of an `EXPR_STMT`. The predicate
enumerates every expression syntax kind that can appear as a
statement — but it does **not** include `SyntaxKind::BLOCK`. So
when the inner `{ ... }` is parsed as `EXPR_STMT > BLOCK`, the
lowerer's `child.children().find(|c| is_expr_node(c.kind()))`
returns `None` and falls through to `ctx.alloc_expr(HirExpr::Error)`
(via the `.unwrap_or_else` at line 664 of the same file).

A debug walk confirms this: for `14_borrow_outlives_owner/input.mty`
the lowered `main` body has `stmts = [Let outer, Let r_out, Expr(Error)]`
followed by the trailing `use_ref(r_out)` tail — the entire inner
block (including `r_out = &inner`) is silently dropped.

**Fix shape** (v0.15): one-line addition of `BLOCK` to the
`matches!` arm in `is_expr_node`. This touches `mty-hir`, which is
outside the v0.14 swarm-agent's owned-files list. Per the working
agreement ("If `14_borrow_outlives_owner` fix requires touching
`mty-hir` or another out-of-scope crate, stop and document the
bigger refactor as v0.15") the v0.14 cycle bounces this back.

The red-shirt stays ignored in `crates/mty-driver/tests/conformance_full.rs`
with the existing v0.12 explanation; the `INTENTIONALLY_IGNORED`
entry is updated to reference this notes file and point at the
lowering fix as the v0.15 owner. Once the one-line lowering fix
lands, the v0.12 + v0.14 borrow wiring will work as designed and
the fixture should fire MT3007 without further changes.

### Other carry-forwards still open (post-v0.14)

- `capability_checking/03_narrow_to_ro` — still depends on the
  Slice-8 cap-narrowing impl in `mty-types`.
- `supervisor_restart/02_escalate` — still depends on `mty-syntax`
  accepting the `escalate` action.

Both remain `INTENTIONALLY_IGNORED` with their v0.2 / v0.3-era
explanations; no movement in v0.14.

---

## Test commands

```bash
cargo test -p mty-types --test mt2003_cannot_infer
cargo test -p mty-types --test mt2023_generic_arg_kind
cargo test -p mty-driver --test conformance_full
```

Expected `conformance_full` line: `91 cases ran across 16 categories
... 3 skipped`.

---

## Files touched

- `crates/mty-types/src/check.rs` — new helpers
  `is_cannot_infer_shape` + `pattern_first_binding_name`; new MT2003
  emit in `check_stmt`.
- `crates/mty-types/src/diag.rs` — new ctor `generic_arg_kind_mismatch`.
- `crates/mty-types/src/resolve.rs` — MT2023 emit inside the Adt
  generic-arg loop.
- `crates/mty-types/tests/mt2003_cannot_infer.rs` — new (3 tests).
- `crates/mty-types/tests/mt2023_generic_arg_kind.rs` — new (2 tests).
- `tests/conformance/type_checking/03_cannot_infer_type/` — new fixture.
- `tests/conformance/type_checking/21_generic_arg_kind_mismatch/` — new fixture.
- `KNOWN_ISSUES.md` — item #11 marked resolved with per-code closure history.
- `dev/history/notes/CONFORMANCE_GAP_V0_14_NOTES.md` — this file.

Not touched (per scope): `mty-hir`, `mty-syntax`, codegen crates,
`mty-macros`, `mty-runtime`, `mty-driver` (except potentially the
`INTENTIONALLY_IGNORED` annotation update), `mty-cli`, `mty-stdlib`,
`crates/mty-types/src/effects.rs`, `crates/mty-types/src/ty.rs`.

---

## Why item #13 doesn't appear by number

`KNOWN_ISSUES.md` currently catalogues issues #1..#12. The swarm
brief refers to "item #13" as shorthand for the
`14_borrow_outlives_owner` red-shirt; that case is tracked inside
`crates/mty-driver/tests/conformance_full.rs::INTENTIONALLY_IGNORED`
rather than as a numbered entry in `KNOWN_ISSUES.md`. The
disposition is documented above under "What was deferred".
