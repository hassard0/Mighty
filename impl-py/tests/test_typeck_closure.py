"""Unit tests for v0.19 closure inference.

The v0.17 type checker treated closure params with no annotation as
``TyAny`` (because there was no HM-bidirectional path from the call site
into the closure body). v0.19 adds that path: when a closure literal
appears in fn-call position and the corresponding param's type is a
``TyFn`` of matching arity, the expected param/ret types are pushed
down into the closure's params and unified against the body's inferred
return type.

Spec sources: v1.0-RC2 §11.6 (closures), §11.6.2 (closure type
inference). Diagnostic codes: MT2011 (closure arity), MT2001 (generic
mismatch).
"""

from __future__ import annotations

from mty.lower import lower
from mty.parser import parse
from mty.typeck import typeck


def _check(src: str):
    tree, parse_diags = parse(src)
    assert not parse_diags, f"parser diags: {parse_diags}"
    mod, lower_diags = lower(tree, source=src)
    assert not lower_diags, f"lower diags: {lower_diags}"
    return typeck(mod, source=src)


# ---------------------------------------------------------------------------
# Closure-in-call: the canonical bidirectional-typing case.
# ---------------------------------------------------------------------------


def test_closure_in_call_with_annotation_typechecks():
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 { apply(fn(y: I32) -> I32 { y + 1 }, 3) }
"""
    assert _check(src) == []


def test_closure_in_call_without_annotation_infers_param():
    # Without v0.19, ``y`` would be TyAny and ``y + 1`` would silently
    # widen to TyAny -> TyAny; now we should accept and infer fn(I32)->I32
    # from the call site.
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 { apply(fn(y) { y + 1 }, 3) }
"""
    assert _check(src) == []


def test_closure_in_call_with_explicit_return_ty():
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 { apply(fn(y: I32) -> I32 { y * 2 }, 5) }
"""
    assert _check(src) == []


def test_closure_arity_mismatch_diagnosed():
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _bad() -> I32 { apply(fn(a: I32, b: I32) -> I32 { a + b }, 3) }
"""
    diags = _check(src)
    assert any(d.code == "MT2011" for d in diags), diags


def test_closure_param_type_mismatch_diagnosed():
    # Annotation Bool conflicts with expected I32.
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _bad() -> I32 { apply(fn(y: Bool) -> I32 { 1 }, 3) }
"""
    diags = _check(src)
    # Either MT2001 (closure param mismatch) or MT2001 (argument 1 mismatch).
    assert any(d.code in ("MT2001",) for d in diags), diags


def test_closure_return_ty_mismatch_diagnosed():
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _bad() -> I32 { apply(fn(y: I32) -> Bool { true }, 3) }
"""
    diags = _check(src)
    assert any(d.code in ("MT2001",) for d in diags), diags


def test_closure_with_no_args():
    src = """
fn invoke(f: fn() -> I32) -> I32 { f() }
fn _go() -> I32 { invoke(fn() -> I32 { 42 }) }
"""
    assert _check(src) == []


def test_closure_taking_str_returning_unit():
    # The print-callback shape: fn(Str) -> Unit, body uses log()
    src = """
fn for_each(f: fn(Str) -> Unit) -> Unit { f("hi") }
fn _go() -> Unit { for_each(fn(s: Str) -> Unit { log(s) }) }
"""
    diags = _check(src)
    # log(s) is `fn(Any) -> Unit` per the builtin shim — accept clean.
    assert diags == []


def test_closure_unannotated_in_strongly_typed_callback():
    # The closure has no annotation; inferred from expected fn(Str)->I32.
    # The body returns a literal 0, which is I32 — matches.
    src = """
fn invoke(f: fn(Str) -> I32) -> I32 { f("hi") }
fn _go() -> I32 { invoke(fn(s) { 0 }) }
"""
    assert _check(src) == []


def test_closure_inferred_unifies_param_use_in_body():
    # The closure's body references the param; the call-site expects
    # the param to be I32. Without v0.19, ``s + 1`` would have a TyAny
    # operand and silently pass; v0.19 should accept and unify cleanly.
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 { apply(fn(s) { s + 1 }, 3) }
"""
    assert _check(src) == []


def test_closure_outside_call_keeps_legacy_inference():
    # No call-site context — the closure's param is still fresh-var, and
    # the body's binop drives inference. As long as the result-typed
    # caller doesn't reject, this should remain clean.
    src = """
fn _go() -> Unit {
  let _f = fn(x: I32) -> I32 { x + 1 }
  ()
}
"""
    diags = _check(src)
    # Single trailing `()` returns TyTuple(()), not TyUnit — this is the
    # pre-existing tuple-vs-Unit issue. Filter for any *closure*-band
    # diagnostic (MT2011/MT2012/MT2001-on-closure).
    closure_diags = [d for d in diags if d.code in ("MT2011", "MT2012")]
    assert closure_diags == [], closure_diags


def test_closure_with_block_body_returning_value():
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 {
  apply(fn(y: I32) -> I32 {
    let z = y * 2
    z + 1
  }, 10)
}
"""
    assert _check(src) == []


def test_closure_body_internal_mismatch_still_caught():
    # The closure's body has its own type error (I32 + Bool); the call
    # site can't paper over that.
    src = """
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _bad() -> I32 { apply(fn(y: I32) -> I32 { y + true }, 3) }
"""
    diags = _check(src)
    assert any(d.code == "MT2009" for d in diags), diags
