"""Unit tests for the HIR types and the parser→HIR lowerer.

The lowerer is the bridge from the JSON-tree the parser emits to the
typed-dataclass HIR the type checker consumes. These tests pin the
shape of each lowering case independently of the example sweep, so a
regression in (say) ``let``-pattern binding can't hide behind a green
example test.
"""

from __future__ import annotations

import pytest

from mty.hir import (
    HirArray,
    HirBinOp,
    HirBlock,
    HirCall,
    HirEnum,
    HirFor,
    HirFn,
    HirIdent,
    HirIf,
    HirLit,
    HirMatch,
    HirModule,
    HirOpaque,
    HirParam,
    HirPath,
    HirPropagate,
    HirReturn,
    HirStmt,
    HirStruct,
    HirStructLit,
    HirTuple,
    HirTy,
    HirTypeAlias,
    HirUnaryOp,
    HirWhile,
    Resolution,
)
from mty.lower import lower
from mty.parser import parse


# ---------------------------------------------------------------------------
# HIR dataclass smoke tests.
# ---------------------------------------------------------------------------


def test_hirty_unit_constructor():
    t = HirTy.unit()
    assert t.name == "Unit"
    assert t.kind == "path"
    assert t.args == []


def test_module_find_helpers_empty():
    m = HirModule(items=[])
    assert m.find_fn("anything") is None
    assert m.find_struct("anything") is None
    assert m.find_enum("anything") is None


def test_module_find_helpers_present():
    fn = HirFn(name="main")
    st = HirStruct(name="Point")
    en = HirEnum(name="Color")
    m = HirModule(items=[fn, st, en])
    assert m.find_fn("main") is fn
    assert m.find_struct("Point") is st
    assert m.find_enum("Color") is en
    assert m.find_fn("Point") is None
    assert m.find_struct("main") is None


def test_resolution_defaults_unknown():
    r = Resolution(kind="unknown")
    assert r.binding_id is None
    assert r.item_name is None
    assert r.variant_name is None


# ---------------------------------------------------------------------------
# Parser → HIR lowering tests.
# ---------------------------------------------------------------------------


def _lower(src: str):
    tree, parse_diags = parse(src)
    assert not parse_diags, f"unexpected parser diags: {parse_diags}"
    mod, lower_diags = lower(tree, source=src)
    return mod, lower_diags


def test_lower_empty_fn():
    mod, diags = _lower("fn main() {}")
    assert diags == []
    assert len(mod.items) == 1
    fn = mod.items[0]
    assert isinstance(fn, HirFn)
    assert fn.name == "main"
    assert fn.params == []
    assert fn.return_ty.name == "Unit"
    assert fn.body is not None


def test_lower_fn_with_params_and_return():
    src = "fn add(a: I32, b: I32) -> I32 { a + b }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    assert isinstance(fn, HirFn)
    assert [p.name for p in fn.params] == ["a", "b"]
    assert all(p.ty.name == "I32" for p in fn.params)
    assert fn.return_ty.name == "I32"
    assert fn.body is not None
    # Tail expression is a binop.
    tail = fn.body.tail
    assert isinstance(tail, HirBinOp)
    assert tail.op == "+"


def test_lower_struct_record_shape():
    src = "struct Point { x: I32 y: I32 }"
    mod, diags = _lower(src)
    assert diags == []
    st = mod.items[0]
    assert isinstance(st, HirStruct)
    assert st.name == "Point"
    assert st.shape == "record"
    assert [f.name for f in st.fields] == ["x", "y"]


def test_lower_enum_with_variants():
    src = "enum Shape { Circle(F64) Rect(F64, F64) Square }"
    mod, diags = _lower(src)
    assert diags == []
    en = mod.items[0]
    assert isinstance(en, HirEnum)
    assert en.name == "Shape"
    assert [v.name for v in en.variants] == ["Circle", "Rect", "Square"]
    # Circle and Rect are tuple-shaped (carry payload); Square is unit.
    assert en.variants[0].shape == "tuple"
    assert en.variants[2].shape == "unit"


def test_lower_type_alias():
    src = "type UserId = U64"
    mod, diags = _lower(src)
    assert diags == []
    ta = mod.items[0]
    assert isinstance(ta, HirTypeAlias)
    assert ta.name == "UserId"
    assert ta.ty.name == "U64"


def test_lower_let_binding_resolves_local():
    src = "fn main() { let x = 1 let _y = x }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    assert isinstance(fn, HirFn)
    assert fn.body is not None
    stmts = fn.body.stmts
    assert len(stmts) == 2
    # The second let's value is an ident ref to `x` (a local).
    second = stmts[1]
    assert isinstance(second, HirStmt)
    assert second.kind == "let"
    val = second.value
    assert isinstance(val, HirIdent)
    assert val.name == "x"
    assert val.res.kind == "local"
    assert val.res.binding_id is not None


def test_lower_unresolved_ident_marked_unknown():
    src = "fn main() { let _y = nonesuch }"
    mod, diags = _lower(src)
    # No diagnostic per current policy (we mute to avoid noise) but the
    # resolution must be ``unknown``.
    fn = mod.items[0]
    val = fn.body.stmts[0].value
    assert isinstance(val, HirIdent)
    assert val.res.kind == "unknown"


def test_lower_match_arm_lowers_to_hirmatch():
    src = """
fn classify(n: I32) -> Str {
  match n {
    0 => "zero"
    _ => "other"
  }
}
"""
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    assert fn.body is not None
    m = fn.body.tail
    assert isinstance(m, HirMatch)
    assert len(m.arms) == 2


def test_lower_if_else_branches():
    src = "fn pick(b: Bool) -> I32 { if b { 1 } else { 2 } }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    ifx = fn.body.tail
    assert isinstance(ifx, HirIf)
    assert isinstance(ifx.then, HirBlock)
    assert ifx.else_ is not None


def test_lower_struct_literal_resolves_path():
    src = """
struct P { x: I32 y: I32 }
fn make() -> P { P { x: 1, y: 2 } }
"""
    mod, diags = _lower(src)
    assert diags == []
    fn = [i for i in mod.items if isinstance(i, HirFn)][0]
    lit = fn.body.tail
    assert isinstance(lit, HirStructLit)
    assert lit.path == ["P"]
    assert [n for n, _ in lit.fields] == ["x", "y"]
    assert lit.res.kind == "item"


def test_lower_for_loop_introduces_pat_binding():
    src = "fn _walk(xs: &[I32]) { for x in xs { log(x) } }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    # Find the for inside the body.
    body = fn.body
    assert body is not None
    # The body is a single statement: an expr_stmt holding the for.
    fr = body.stmts[0].value if body.stmts else body.tail
    assert isinstance(fr, HirFor)
    assert fr.pat.kind == "ident"
    assert fr.pat.name == "x"


def test_lower_while_loop():
    src = "fn _wait() { while ready() { _step() } }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    body = fn.body
    wh = body.stmts[0].value if body.stmts else body.tail
    assert isinstance(wh, HirWhile)


def test_lower_propagate_operator():
    src = "fn _go() { let _x = thing()? }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    let_stmt = fn.body.stmts[0]
    assert isinstance(let_stmt.value, HirPropagate)


def test_lower_tuple_and_array_literals():
    src = """
fn _things() {
  let _t = (1, 2, 3)
  let _a = [10, 20, 30]
}
"""
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    t_let = fn.body.stmts[0]
    a_let = fn.body.stmts[1]
    assert isinstance(t_let.value, HirTuple)
    assert len(t_let.value.elems) == 3
    assert isinstance(a_let.value, HirArray)
    assert len(a_let.value.elems) == 3


def test_lower_return_stmt():
    src = "fn _early() -> I32 { return 42 }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    body = fn.body
    ret = body.stmts[0].value if body.stmts else body.tail
    assert isinstance(ret, HirReturn)


def test_lower_duplicate_item_emits_diagnostic():
    src = """
fn dup() {}
fn dup() {}
"""
    mod, diags = _lower(src)
    assert any(d.code == "MT1503" for d in diags), \
        f"expected MT1503 in diags, got {[d.code for d in diags]}"


def test_lower_opaque_for_agent_block():
    # Agents lower as no-item; the module just has the protocol fn if any.
    src = """
protocol P {
  Ping(s: Str) -> Str
}
agent Echoer: P {
  on Ping(s) -> s
}
"""
    mod, diags = _lower(src)
    # No items lowered — both protocol and agent are dropped.
    assert mod.items == []


def test_lower_call_expr_carries_callee_and_args():
    # A bare `log(...)` at the end of a block is parsed as the tail
    # expression (not a stmt). Lowering preserves that shape.
    src = "fn _go() { log(\"hi\") }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    call = fn.body.tail
    assert isinstance(call, HirCall)
    assert isinstance(call.callee, HirIdent)
    assert call.callee.name == "log"
    assert len(call.args) == 1


def test_lower_unary_negation_and_ref():
    src = "fn _u() { let _a = -1 let _b = &1 }"
    mod, diags = _lower(src)
    assert diags == []
    fn = mod.items[0]
    a = fn.body.stmts[0].value
    b = fn.body.stmts[1].value
    assert isinstance(a, HirUnaryOp) and a.op == "-"
    assert isinstance(b, HirUnaryOp) and b.op == "&"


def test_lower_enum_variant_via_field_access():
    # The parser shapes ``Shape.Circle`` as a field-access on a
    # single-segment path (``HirField(target=HirIdent("Shape"),
    # name="Circle")``). Lowering preserves this; the typeck pass treats
    # known enum field accesses as variant constructors.
    from mty.hir import HirField
    src = """
enum Shape { Circle Rect }
fn _pick() -> Shape { Shape.Circle }
"""
    mod, diags = _lower(src)
    assert diags == []
    fn = [i for i in mod.items if isinstance(i, HirFn)][0]
    tail = fn.body.tail
    assert isinstance(tail, HirField)
    assert tail.name == "Circle"
    assert isinstance(tail.target, HirIdent)
    assert tail.target.name == "Shape"
    assert tail.target.res.kind == "item"
    assert tail.target.res.item_name == "Shape"
