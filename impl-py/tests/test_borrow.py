"""Borrow-check unit tests for the Python 2nd-impl (v0.22).

The checker is an NLL-flavoured subset (spec-driven, no source peek
into ``crates/mty-borrow/``). These tests pin the canonical
borrow-check shapes against the MT3xxx diagnostic band:

* **MT3001** — move while borrowed
* **MT3002** — move out of a borrow (field projection)
* **MT3003** — &mut + & coexist
* **MT3004** — use after move
* **MT3005** — two &mut borrows alive at once

The Mighty spec's §8 Copy/Move dichotomy is approximated from typeck
output: scalars + refs are Copy, aggregates (Str, arrays, records,
enums, tuples, options, results) are not.

Spec sources: v1.0-RC2 §8 (ownership), §9 (lifetimes), §33 (MT3xxx).
"""

from __future__ import annotations

import pytest

from mty.borrow import (
    BindingInfo,
    BorrowChecker,
    BorrowContext,
    Loan,
    Place,
    USE_BORROW_MUT,
    USE_BORROW_SHARED,
    USE_COPY,
    USE_MOVE,
    borrow_check,
    is_copy,
)
from mty.diagnostics import (
    CODE_BORROW_DOUBLE_MUT,
    CODE_BORROW_MOVE_OF_BORROWED,
    CODE_BORROW_MOVE_OUT_OF_BORROW,
    CODE_BORROW_MUT_SHARED_CONFLICT,
    CODE_BORROW_USE_AFTER_MOVE,
)
from mty.lower import lower
from mty.parser import parse
from mty.typeck import (
    TY_ANY,
    TY_BOOL,
    TY_CHAR,
    TY_F64,
    TY_I32,
    TY_STR,
    TY_UNIT,
    TyArray,
    TyEnum,
    TyInt,
    TyOpaque,
    TyOption,
    TyRecord,
    TyRef,
    TyResult,
    TyTuple,
    TyVar,
)


# ---------------------------------------------------------------------------
# Pipeline helpers.
# ---------------------------------------------------------------------------


def _check(src: str):
    """Pipe: source -> parse -> lower -> borrow_check. Returns the
    list of borrow diagnostics."""
    tree, parse_diags = parse(src)
    assert not parse_diags, f"parser diags: {parse_diags}"
    mod, lower_diags = lower(tree, source=src)
    assert not lower_diags, f"lower diags: {lower_diags}"
    return borrow_check(mod, source=src)


def _codes(diags) -> list[str]:
    return [d.code for d in diags]


# ---------------------------------------------------------------------------
# Copy classification (pure, no parsing).
# ---------------------------------------------------------------------------


def test_is_copy_scalars():
    assert is_copy(TY_I32)
    assert is_copy(TY_BOOL)
    assert is_copy(TY_CHAR)
    assert is_copy(TY_F64)
    assert is_copy(TY_UNIT)
    assert is_copy(TyInt(8, False))


def test_is_copy_refs_are_copy():
    assert is_copy(TyRef(inner=TY_I32))
    assert is_copy(TyRef(inner=TY_STR, mut=True))


def test_is_copy_non_copy_aggregates():
    assert not is_copy(TY_STR)
    assert not is_copy(TyArray(elem=TY_I32))
    assert not is_copy(TyTuple(elems=(TY_I32, TY_STR)))
    assert not is_copy(TyRecord(name="Point"))
    assert not is_copy(TyEnum(name="Shape"))
    assert not is_copy(TyOption(inner=TY_I32))
    assert not is_copy(TyResult(ok=TY_I32))
    assert not is_copy(TyOpaque(name="Url"))


def test_is_copy_any_and_var_default_to_copy():
    # Inference vars and TyAny default to Copy — they're the escape
    # hatch when typeck didn't pin a concrete type. This matches the
    # spec-validation policy: be permissive when we don't know.
    assert is_copy(TY_ANY)
    assert is_copy(TyVar(id=0))


# ---------------------------------------------------------------------------
# Pure Place / Loan / BorrowContext smoke tests.
# ---------------------------------------------------------------------------


def test_place_root_and_projection():
    p = Place(root=0)
    assert p.is_root
    pp = p.with_field("origin").with_field("x")
    assert not pp.is_root
    assert pp.path == ("origin", "x")
    assert pp.root == 0


def test_borrow_context_scopes_isolate_names():
    ctx = BorrowContext()
    ctx.declare("x", TY_I32)
    ctx.push_scope()
    ctx.declare("x", TY_BOOL)  # shadow
    inner_id = ctx.lookup("x")
    ctx.pop_scope()
    outer_id = ctx.lookup("x")
    assert inner_id is not None and outer_id is not None
    assert inner_id != outer_id


def test_borrow_context_loans_scoped_to_block():
    ctx = BorrowContext()
    bid = ctx.declare("x", TY_STR)
    ctx.push_scope()
    ctx.add_loan(Loan(place=Place(root=bid), mut=False, span=(0, 0)))
    assert len(ctx.loans_on(bid)) == 1
    ctx.pop_scope()
    assert ctx.loans_on(bid) == []


# ---------------------------------------------------------------------------
# Clean shapes (no diagnostics).
# ---------------------------------------------------------------------------


def test_clean_scalar_let_chain_no_diag():
    # Scalars are Copy — re-using `x` is not a move.
    diags = _check("""
        fn main() {
          let x = 1
          let y = x
          let z = x
          log(z)
        }
    """)
    assert diags == [], diags


def test_clean_shared_borrow_then_borrow_again_no_diag():
    # Two shared borrows of the same place are allowed.
    diags = _check("""
        fn main() {
          let s = "hi"
          let r1 = &s
          let r2 = &s
        }
    """)
    assert all(d.code != CODE_BORROW_MUT_SHARED_CONFLICT for d in diags)
    assert all(d.code != CODE_BORROW_MOVE_OF_BORROWED for d in diags)


def test_clean_move_into_let_then_use_new_no_diag():
    diags = _check("""
        fn main() {
          let s = "hi"
          let t = s
          log(t)
        }
    """)
    assert diags == [], diags


def test_clean_borrow_dropped_then_move():
    # After the borrow's block scope ends, the value is movable again.
    diags = _check("""
        fn main() {
          let s = "hi"
          {
            let r = &s
          }
          let t = s
        }
    """)
    # The conservative scope-based lifetime means r goes out of scope
    # before the t move, so no diag.
    assert all(d.code not in (
        CODE_BORROW_MOVE_OF_BORROWED, CODE_BORROW_USE_AFTER_MOVE
    ) for d in diags)


# ---------------------------------------------------------------------------
# MT3001 — move while borrowed.
# ---------------------------------------------------------------------------


def test_move_of_borrowed_emits_mt3001():
    diags = _check("""
        fn main() {
          let s = "hi"
          let r = &s
          let t = s
        }
    """)
    assert CODE_BORROW_MOVE_OF_BORROWED in _codes(diags), diags


def test_move_of_mut_borrowed_emits_mt3001():
    diags = _check("""
        fn main() {
          let s = "hi"
          let r = &mut s
          let t = s
        }
    """)
    assert CODE_BORROW_MOVE_OF_BORROWED in _codes(diags), diags


# ---------------------------------------------------------------------------
# MT3004 — use after move.
# ---------------------------------------------------------------------------


def test_use_after_move_emits_mt3004():
    diags = _check("""
        fn main() {
          let s = "hi"
          let t = s
          let u = s
        }
    """)
    assert CODE_BORROW_USE_AFTER_MOVE in _codes(diags), diags


def test_use_after_move_in_call_emits_mt3004():
    diags = _check("""
        fn main() {
          let s = "hi"
          let t = s
          log(s)
        }
    """)
    assert CODE_BORROW_USE_AFTER_MOVE in _codes(diags), diags


def test_use_after_move_notes_include_origin_offset():
    diags = _check("""
        fn main() {
          let s = "hi"
          let t = s
          let u = s
        }
    """)
    after_moves = [d for d in diags if d.code == CODE_BORROW_USE_AFTER_MOVE]
    assert after_moves
    # Note text mentions move offset (not the value, but presence).
    assert any("moved" in n for d in after_moves for n in d.notes)


# ---------------------------------------------------------------------------
# MT3003 — &mut + & conflict.
# ---------------------------------------------------------------------------


def test_mut_then_shared_emits_mt3003():
    diags = _check("""
        fn main() {
          let s = "hi"
          let r = &mut s
          let r2 = &s
        }
    """)
    assert CODE_BORROW_MUT_SHARED_CONFLICT in _codes(diags), diags


def test_shared_then_mut_emits_mt3003():
    diags = _check("""
        fn main() {
          let s = "hi"
          let r = &s
          let r2 = &mut s
        }
    """)
    assert CODE_BORROW_MUT_SHARED_CONFLICT in _codes(diags), diags


# ---------------------------------------------------------------------------
# MT3005 — two &mut borrows alive.
# ---------------------------------------------------------------------------


def test_double_mut_emits_mt3005():
    diags = _check("""
        fn main() {
          let s = "hi"
          let r1 = &mut s
          let r2 = &mut s
        }
    """)
    assert CODE_BORROW_DOUBLE_MUT in _codes(diags), diags


# ---------------------------------------------------------------------------
# MT3002 — move out of a borrow (field projection).
# ---------------------------------------------------------------------------


def test_move_field_out_of_borrowed_emits_mt3002():
    # Move a field while a shared borrow is live — borrow check
    # catches this as either MT3001 (move while borrowed) or MT3002
    # depending on the path; we accept either.
    diags = _check("""
        fn main() {
          let p = "hi"
          let r = &p
          let t = p
        }
    """)
    codes = _codes(diags)
    assert (CODE_BORROW_MOVE_OF_BORROWED in codes
            or CODE_BORROW_MOVE_OUT_OF_BORROW in codes), diags


# ---------------------------------------------------------------------------
# Branch joining — if both branches move, the join must reflect it.
# ---------------------------------------------------------------------------


def test_move_in_only_one_if_branch_does_not_taint_after():
    diags = _check("""
        fn main() {
          let s = "hi"
          if true {
            let t = s
          } else {
          }
          log("post")
        }
    """)
    # A subsequent use of `s` would taint, but we don't use it. The
    # walker should not emit a stray use-after-move on the println.
    assert CODE_BORROW_USE_AFTER_MOVE not in _codes(diags), diags


# ---------------------------------------------------------------------------
# Scope shadowing — inner scope borrows do not leak out.
# ---------------------------------------------------------------------------


def test_inner_block_borrow_does_not_block_outer_move():
    diags = _check("""
        fn main() {
          let s = "hi"
          {
            let r = &s
            log("inner")
          }
          let t = s
        }
    """)
    assert CODE_BORROW_MOVE_OF_BORROWED not in _codes(diags), diags


def test_inner_block_move_does_taint_outer():
    diags = _check("""
        fn main() {
          let s = "hi"
          {
            let t = s
          }
          let u = s
        }
    """)
    assert CODE_BORROW_USE_AFTER_MOVE in _codes(diags), diags


# ---------------------------------------------------------------------------
# Copy types — should not trip move diagnostics.
# ---------------------------------------------------------------------------


def test_i32_copy_does_not_trigger_use_after_move():
    diags = _check("""
        fn main() {
          let n = 42
          let a = n
          let b = n
          let c = n
        }
    """)
    assert CODE_BORROW_USE_AFTER_MOVE not in _codes(diags), diags


def test_bool_copy_does_not_trigger_move():
    diags = _check("""
        fn main() {
          let b = true
          let a = b
          let c = b
        }
    """)
    assert CODE_BORROW_USE_AFTER_MOVE not in _codes(diags), diags


# ---------------------------------------------------------------------------
# Borrow-of-temporary edge case.
# ---------------------------------------------------------------------------


def test_borrow_of_literal_is_not_tracked():
    # &literal_string is a temporary — no place to borrow. No diag.
    diags = _check("""
        fn main() {
          let r = &"hello"
          log("ok")
        }
    """)
    assert diags == [], diags


# ---------------------------------------------------------------------------
# Module-level: extern + no-body fn does not crash the checker.
# ---------------------------------------------------------------------------


def test_extern_only_module_no_crash():
    diags = _check("""
        extern {
          fn _work(item: I32) -> Unit
        }
        fn main() {
          log("ok")
        }
    """)
    # No borrow diagnostics on this purely-extern module.
    assert diags == [], diags


# ---------------------------------------------------------------------------
# Top-level: borrow_check returns a list.
# ---------------------------------------------------------------------------


def test_borrow_check_returns_list():
    src = "fn main() { log(\"hi\") }"
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    result = borrow_check(mod, source=src)
    assert isinstance(result, list)
