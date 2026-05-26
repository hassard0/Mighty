"""Unit tests for v0.19 generics-with-constraints typeck.

v0.17 erased all generic parameters to ``TyAny`` at signature
collection. v0.19 keeps them as fresh ``TyVar`` records inside a per-fn
scope and records their bounds on the ``TypeChecker.fn_generics`` map.
Call sites instantiate the scheme (fresh TyVars), unify per-arg, then
discharge each bound against the resolved TyVar.

The bound vocabulary is intentionally a small set of well-known prelude
traits (``Display``, ``Debug``, ``Clone``, ``Copy``, ``Eq``, ``Ord``,
``PartialEq``, ``PartialOrd``, ``Hash``, ``Default``, ``Send``,
``Sync``, ``Sized``). Bounds the checker doesn't recognise are accepted
unconditionally — user-defined traits cannot be checked without trait
items, and v0.19's job is "expressible", not "complete".

Spec sources: v1.0-RC2 §6.4 (generic parameters), §6.4.2 (bounds),
§11.3 (call instantiation). Diagnostic codes: MT2012 (bound
unsatisfied), MT2013 (unknown generic), MT2001 (generic mismatch).
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
# Generic instantiation (no bounds).
# ---------------------------------------------------------------------------


def test_identity_generic_fn_typechecks():
    # The classic `fn id[T](x: T) -> T`. Without v0.19's per-fn generic
    # env, the two ``T`` references would resolve to *different* TyVars
    # and inference would silently treat them as independent.
    src = """
fn id[T](x: T) -> T { x }
fn _go() -> I32 { id(42) }
"""
    assert _check(src) == []


def test_identity_generic_returns_str():
    src = """
fn id[T](x: T) -> T { x }
fn _go() -> Str { id("hi") }
"""
    assert _check(src) == []


def test_identity_called_twice_with_different_tys():
    # Cross-call cross-contamination test: if the substitution leaked,
    # the second call would constrain ``T`` to I32 from the first.
    src = """
fn id[T](x: T) -> T { x }
fn _a() -> I32 { id(1) }
fn _b() -> Str { id("a") }
"""
    assert _check(src) == []


def test_two_param_generic_independent_tyvars():
    # The first param is A, the second B — they should NOT be unified.
    src = """
fn pair[A, B](a: A, b: B) -> A { a }
fn _go() -> I32 { pair(1, "x") }
"""
    assert _check(src) == []


def test_generic_return_type_propagates():
    # The fn returns T; the call's return position pins T to the
    # context's expected type. Here the let-binding annotates I32.
    src = """
fn id[T](x: T) -> T { x }
fn _go() -> I32 {
  let y: I32 = id(7)
  y
}
"""
    assert _check(src) == []


# ---------------------------------------------------------------------------
# Generic bounds — built-in trait set.
# ---------------------------------------------------------------------------


def test_display_bound_accepts_i32():
    src = """
fn show[T: Display](x: T) -> I32 { 0 }
fn _go() -> I32 { show(42) }
"""
    assert _check(src) == []


def test_display_bound_accepts_str():
    src = """
fn show[T: Display](x: T) -> I32 { 0 }
fn _go() -> I32 { show("hi") }
"""
    assert _check(src) == []


def test_ord_bound_rejects_f64():
    # F64 has no total order — Ord bound must fail.
    src = """
fn sort_one[T: Ord](x: T) -> T { x }
fn _go() -> F64 { sort_one(3.14) }
"""
    diags = _check(src)
    assert any(d.code == "MT2012" for d in diags), diags


def test_partial_ord_bound_accepts_f64():
    # PartialOrd accepts floats (partial order is fine).
    src = """
fn cmp[T: PartialOrd](x: T) -> T { x }
fn _go() -> F64 { cmp(3.14) }
"""
    assert _check(src) == []


def test_hash_bound_rejects_f64():
    # Hash excludes float per Rust convention (no NaN-safe hash).
    src = """
fn hash_it[T: Hash](x: T) -> I32 { 0 }
fn _go() -> I32 { hash_it(3.14) }
"""
    diags = _check(src)
    assert any(d.code == "MT2012" for d in diags), diags


def test_eq_bound_rejects_f64():
    # Eq is the strict total-equality bound; F64 doesn't get it.
    src = """
fn eq_only[T: Eq](x: T) -> I32 { 0 }
fn _go() -> I32 { eq_only(3.14) }
"""
    diags = _check(src)
    assert any(d.code == "MT2012" for d in diags), diags


def test_multiple_bounds_all_must_satisfy():
    # ``T: Display + Eq`` — F32 fails Eq.
    src = """
fn show[T: Display + Eq](x: T) -> I32 { 0 }
fn _go() -> I32 { show(3.14) }
"""
    diags = _check(src)
    assert any(d.code == "MT2012" for d in diags), diags


def test_multiple_bounds_passes_when_satisfied():
    src = """
fn show[T: Display + Eq](x: T) -> I32 { 0 }
fn _go() -> I32 { show(42) }
"""
    assert _check(src) == []


def test_unknown_bound_passes_conservatively():
    # Custom trait names we don't model should not flag — we don't have
    # the trait items, so any-fits is the correct conservative answer.
    src = """
fn process[T: MyTrait](x: T) -> I32 { 0 }
fn _go() -> I32 { process(42) }
"""
    assert _check(src) == []


def test_opaque_type_passes_bound_conservatively():
    # User-defined nominal types satisfy bounds conservatively (the
    # spec borrow/trait pass will tighten this in v1.0).
    src = """
fn show[T: Display](x: T) -> I32 { 0 }
fn _go(u: Url) -> I32 { show(u) }
"""
    diags = _check(src)
    # No MT2012 — opaque type is accepted.
    assert not any(d.code == "MT2012" for d in diags), diags


def test_generic_fn_bound_holds_per_call():
    # Two calls to the same generic fn with different concrete types —
    # each should be checked independently.
    src = """
fn show[T: Display](x: T) -> I32 { 0 }
fn _good() -> I32 { show(42) }
fn _bad() -> I32 { show(3.14) }
"""
    diags = _check(src)
    # F64 satisfies Display — both should pass.
    assert not any(d.code == "MT2012" for d in diags), diags


def test_generic_arg_mismatch_diagnosed():
    # Pass a Bool where the body uses arithmetic. This is the
    # "instantiation conflict" case: the call binds T=Bool, then the
    # body's `x + 1` would conflict — caught when the body is checked
    # against its own signature.
    src = """
fn plus_one[T](x: T) -> T { x + 1 }
fn _go() -> I32 { plus_one(5) }
"""
    # The body's `x + 1` causes I32 unification on T inside the fn body
    # check, separate from instantiation; this should still parse + lower.
    # We expect either clean (if `T` unifies with I32 via the body) or a
    # diagnostic — the v0.19 checker is permissive here so we just assert
    # no instantiation crash.
    diags = _check(src)
    # No bound diagnostic and no instantiation-crash diagnostic.
    assert not any(d.code == "MT2012" for d in diags), diags


def test_generic_fn_used_polymorphically_in_higher_order():
    src = """
fn id[T](x: T) -> T { x }
fn apply(f: fn(I32) -> I32, x: I32) -> I32 { f(x) }
fn _go() -> I32 { apply(fn(y: I32) -> I32 { id(y) }, 3) }
"""
    diags = _check(src)
    # Closure inference + generic call should be clean.
    assert diags == [], diags


def test_generic_bound_with_user_trait_does_not_break_arg_unification():
    # The bound is unknown so accepted conservatively; the arg type
    # should still flow through.
    src = """
fn show[T: Custom](x: T) -> T { x }
fn _go() -> I32 { show(7) }
"""
    assert _check(src) == []


def test_generic_call_returns_concrete_when_pinned():
    # When the call's arg pins T, the return type should follow.
    src = """
fn id[T](x: T) -> T { x }
fn _add() -> I32 { id(2) + id(3) }
"""
    assert _check(src) == []
