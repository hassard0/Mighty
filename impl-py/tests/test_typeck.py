"""Unit tests for the type checker.

These pin the inference rules independently of the example sweep: a
regression in (say) ``if``-branch unification needs to surface here
before it can hide behind a green example test.

The checker is intentionally permissive (TyAny absorbs anywhere we
don't model — agents, traits, effect rows) so the negative tests below
focus on the rules we **do** model:

* literal inference (int → I32 default, float → F64, str → Str, bool)
* fn-call arity + per-arg unification
* if/match branch agreement
* let-binding annotation matching
* struct-literal field-set matching
* binary-operator operand consistency

Spec sources: v1.0-RC2 §6 (types), §11 (expressions), §33 (MT2xxx
diagnostic codes).
"""

from __future__ import annotations

import pytest

from mty.lower import lower
from mty.parser import parse
from mty.typeck import (
    TY_ANY,
    TY_BOOL,
    TY_F64,
    TY_I32,
    TY_STR,
    TY_UNIT,
    Subst,
    TyArray,
    TyFn,
    TyInt,
    TyOption,
    TyRef,
    TyResult,
    TyTuple,
    TyVar,
    UnifyError,
    apply,
    ty_show,
    typeck,
    unify,
)


# ---------------------------------------------------------------------------
# Pipeline helper: source string -> diagnostics list.
# ---------------------------------------------------------------------------


def _check(src: str):
    tree, parse_diags = parse(src)
    assert not parse_diags, f"parser diags: {parse_diags}"
    mod, lower_diags = lower(tree, source=src)
    assert not lower_diags, f"lower diags: {lower_diags}"
    return typeck(mod, source=src)


# ---------------------------------------------------------------------------
# Unification / substitution unit tests (pure, no parsing involved).
# ---------------------------------------------------------------------------


def test_unify_same_int_no_op():
    s: Subst = {}
    unify(TY_I32, TY_I32, s)
    assert s == {}


def test_unify_int_with_diff_width_raises():
    s: Subst = {}
    with pytest.raises(UnifyError):
        unify(TyInt(32, True), TyInt(64, True), s)


def test_unify_var_bind():
    s: Subst = {}
    v = TyVar(0)
    unify(v, TY_BOOL, s)
    assert apply(s, v) == TY_BOOL


def test_unify_two_vars_chain():
    s: Subst = {}
    v0, v1 = TyVar(0), TyVar(1)
    unify(v0, v1, s)
    unify(v1, TY_STR, s)
    assert apply(s, v0) == TY_STR


def test_unify_fn_arity_mismatch_raises():
    s: Subst = {}
    f1 = TyFn(params=(TY_I32,), ret=TY_UNIT)
    f2 = TyFn(params=(TY_I32, TY_I32), ret=TY_UNIT)
    with pytest.raises(UnifyError):
        unify(f1, f2, s)


def test_unify_tyany_absorbs():
    s: Subst = {}
    # TyAny absorbs without recording substitutions.
    unify(TY_ANY, TY_I32, s)
    unify(TY_I32, TY_ANY, s)
    assert s == {}


def test_unify_occurs_check_blocks_infinite_type():
    s: Subst = {}
    v = TyVar(0)
    # v = fn() -> v -> infinite. Should raise.
    with pytest.raises(UnifyError):
        unify(v, TyFn(params=(), ret=v), s)


def test_ty_show_renders_int_and_fn():
    assert ty_show(TyInt(32, True)) == "I32"
    assert ty_show(TyInt(64, False)) == "U64"
    assert ty_show(TyFn(params=(TY_I32,), ret=TY_BOOL)) == "fn(I32) -> Bool"
    assert ty_show(TyTuple(elems=(TY_I32, TY_BOOL))) == "(I32, Bool)"
    assert ty_show(TyOption(inner=TY_I32)) == "Option[I32]"
    assert ty_show(TyArray(elem=TY_I32)) == "[I32]"
    assert ty_show(TyRef(inner=TY_I32, mut=True)) == "&mut I32"


# ---------------------------------------------------------------------------
# End-to-end typeck tests over tiny source snippets.
# ---------------------------------------------------------------------------


def test_empty_fn_typeck_clean():
    assert _check("fn main() {}") == []


def test_literal_inference_int_default_i32():
    # 42 is inferred as I32 and matches a -> I32 return.
    assert _check("fn ans() -> I32 { 42 }") == []


def test_literal_inference_returning_string():
    assert _check('fn greet() -> Str { "hi" }') == []


def test_literal_inference_bool():
    assert _check("fn ok() -> Bool { true }") == []


def test_return_type_mismatch_diagnosed():
    diags = _check("fn wrong() -> I32 { true }")
    assert any(d.code == "MT2008" for d in diags), diags


def test_let_annotation_matches_value():
    assert _check("fn _go() { let _x: I32 = 1 }") == []


def test_let_annotation_mismatch_diagnosed():
    diags = _check("fn _go() { let _x: I32 = true }")
    assert any(d.code == "MT2001" for d in diags), diags


def test_if_branches_must_agree():
    diags = _check("fn _pick(b: Bool) -> I32 { if b { 1 } else { 2 } }")
    assert diags == []


def test_if_branches_mismatch_diagnosed():
    # Branches yield different types; should diagnose.
    diags = _check("fn _bad(b: Bool) -> I32 { if b { 1 } else { true } }")
    # Either MT2007 (branch) or MT2001 (mismatch via the unify on return).
    assert any(d.code in ("MT2007", "MT2001") for d in diags), diags


def test_if_condition_must_be_bool():
    diags = _check("fn _bad() -> I32 { if 1 { 1 } else { 2 } }")
    assert any(d.code == "MT2001" for d in diags), diags


def test_fn_call_arity_diagnosed():
    diags = _check("""
fn add(a: I32, b: I32) -> I32 { a + b }
fn _use() -> I32 { add(1) }
""")
    assert any(d.code == "MT2003" for d in diags), diags


def test_fn_call_arg_mismatch_diagnosed():
    diags = _check("""
fn _take(a: I32) {}
fn _use() { _take(true) }
""")
    assert any(d.code == "MT2001" for d in diags), diags


def test_fn_call_well_typed():
    src = """
fn add(a: I32, b: I32) -> I32 { a + b }
fn _use() -> I32 { add(1, 2) }
"""
    assert _check(src) == []


def test_match_arms_unify():
    src = """
fn _classify(n: I32) -> Str {
  match n {
    0 => "zero"
    1 => "one"
    _ => "many"
  }
}
"""
    assert _check(src) == []


def test_match_arm_mismatch_diagnosed():
    # Arms produce I32 vs Str -> diagnose.
    src = """
fn _bad(n: I32) -> I32 {
  match n {
    0 => 0
    _ => "no"
  }
}
"""
    diags = _check(src)
    assert any(d.code in ("MT2007", "MT2001") for d in diags), diags


def test_struct_lit_field_set_complete():
    src = """
struct Point { x: I32 y: I32 }
fn _go() -> Point { Point { x: 1, y: 2 } }
"""
    assert _check(src) == []


def test_struct_lit_missing_field_diagnosed():
    src = """
struct Point { x: I32 y: I32 }
fn _go() -> Point { Point { x: 1 } }
"""
    diags = _check(src)
    assert any(d.code == "MT2004" for d in diags), diags


def test_struct_lit_extra_field_diagnosed():
    src = """
struct Point { x: I32 y: I32 }
fn _go() -> Point { Point { x: 1, y: 2, z: 3 } }
"""
    diags = _check(src)
    assert any(d.code == "MT2004" for d in diags), diags


def test_struct_field_type_mismatch_diagnosed():
    src = """
struct Point { x: I32 y: I32 }
fn _go() -> Point { Point { x: 1, y: true } }
"""
    diags = _check(src)
    assert any(d.code == "MT2001" for d in diags), diags


def test_binop_plus_consistent_operands():
    assert _check("fn _add() -> I32 { 1 + 2 }") == []


def test_binop_plus_operand_mismatch_diagnosed():
    diags = _check("fn _bad() -> I32 { 1 + true }")
    assert any(d.code == "MT2009" for d in diags), diags


def test_comparison_returns_bool():
    assert _check("fn _eq() -> Bool { 1 == 2 }") == []


def test_logical_op_requires_bool_operands():
    diags = _check("fn _bad() -> Bool { 1 && true }")
    assert any(d.code == "MT2009" for d in diags), diags


def test_let_then_use_propagates_type():
    # `x` is inferred as I32 from the literal; `_x + 1` then returns I32.
    src = "fn _r() -> I32 { let x = 1; let _y = x + 2; _y }"
    # Walrus-style sep `;` may not parse — fall back to newlines.
    src = "fn _r() -> I32 { let x = 1\nlet _y = x + 2\n_y }"
    assert _check(src) == []


def test_propagate_unwraps_result():
    src = """
fn _producer() -> I32!E { Ok(1) }
fn _go() -> I32!E { let x = _producer()?; Ok(x) }
"""
    src = src.replace(";", "\n")
    # This is permissive: the checker treats the !E effect as erased.
    diags = _check(src)
    # Should not produce any MT2xxx errors for the propagate itself.
    bad = [d for d in diags if d.code.startswith("MT2") and "propagate" in d.message]
    assert bad == []


def test_typeck_runs_clean_on_classic_main():
    assert _check('fn main() { log("hi") }') == []


def test_typeck_unknown_capital_name_suppressed():
    # `Url`/`Page` are domain types we don't model — must not flag.
    src = """
fn _route(u: Url) -> Page { Page {} }
"""
    diags = _check(src)
    # Currently struct fields differ -> we DO emit MT2004 for `Page {}`
    # when no struct decl. Filter that out and confirm no MT2002.
    assert not any(d.code == "MT2002" for d in diags)


def test_typeck_unknown_underscore_helper_suppressed():
    # Underscore-prefixed names mirror the example-corpus convention for
    # private helpers / extern stubs. They must not flag MT2002.
    src = """
fn main() { _helper() }
"""
    diags = _check(src)
    assert not any(d.code == "MT2002" for d in diags)


def test_extern_block_lowers_fn_signatures():
    # Extern fns should be callable without MT2002. Per the lowerer,
    # extern bodies are dropped but their signatures land in fn_sigs.
    src = """
extern {
  fn _do(x: I32) -> I32
}
fn main() { let _y = _do(1) }
"""
    assert _check(src) == []


def test_match_guard_must_be_bool():
    src = """
fn _classify(n: I32) -> Str {
  match n {
    x if 1 => "weird"
    _ => "ok"
  }
}
"""
    diags = _check(src)
    # The guard `1` is I32, not Bool — should diagnose.
    assert any(d.code == "MT2001" for d in diags), diags
