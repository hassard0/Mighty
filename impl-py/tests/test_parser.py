"""Unit tests for ``mty.parser``.

Spec source: ``docs/spec/v1.0-rc.md`` §4 onwards.
"""

from __future__ import annotations

import pytest

from mty.parser import parse


def parse_ok(src: str):
    tree, diags = parse(src)
    assert not diags, f"unexpected diagnostics: {[(d.code, d.message) for d in diags]}"
    return tree


def first_expr_in_body(body: dict) -> dict:
    """Pull the first expression out of a block, regardless of whether
    it sits in the trailing-tail slot or as a leading expression
    statement. The spec (§11) treats both as the same surface."""
    if body["tail"] is not None:
        return body["tail"]
    for stmt in body["stmts"]:
        if stmt["_kind"] == "expr_stmt":
            return stmt["expr"]
        if stmt["_kind"] == "let":
            return stmt
    raise AssertionError(f"no expression-or-let stmt in block: {body}")


def test_hello_world():
    tree = parse_ok("fn main() {\n  log(\"hello\")\n}\n")
    assert tree["_kind"] == "file"
    assert len(tree["items"]) == 1
    fn = tree["items"][0]
    assert fn["_kind"] == "fn"
    assert fn["name"] == "main"
    assert fn["params"] == []
    assert fn["body"]["_kind"] == "block"


def test_struct_record():
    tree = parse_ok("struct Point { x: F64, y: F64 }")
    s = tree["items"][0]
    assert s["_kind"] == "struct"
    assert s["shape"] == "record"
    assert [f["name"] for f in s["fields"]] == ["x", "y"]


def test_enum_with_tuple_variants():
    src = """
    enum Shape {
      Circle(F64)
      Rect(F64, F64)
    }
    """
    tree = parse_ok(src)
    e = tree["items"][0]
    assert e["_kind"] == "enum"
    names = [v["name"] for v in e["variants"]]
    assert names == ["Circle", "Rect"]
    assert e["variants"][0]["shape"] == "tuple"


def test_type_alias():
    tree = parse_ok("type UserId = U64")
    a = tree["items"][0]
    assert a["_kind"] == "type_alias"
    assert a["name"] == "UserId"


def test_fn_with_generic_params():
    tree = parse_ok("fn first[T](xs: T) -> T { xs }")
    fn = tree["items"][0]
    assert fn["generics"][0]["name"] == "T"
    assert fn["ret_type"]["_kind"] == "ty_path"
    assert fn["ret_type"]["segments"] == ["T"]


def test_match_arms_and_underscore():
    src = "fn k(n: I32) -> Str { match n { 0 => \"zero\", 1 => \"one\", _ => \"big\" } }"
    tree = parse_ok(src)
    fn = tree["items"][0]
    match_expr = fn["body"]["tail"]
    assert match_expr["_kind"] == "match"
    assert len(match_expr["arms"]) == 3
    assert match_expr["arms"][-1]["pat"]["_kind"] == "pat_wild"


def test_if_let():
    src = "fn k(x: Option[I32]) { if let Some(v) = x { log(\"hit\") } else { log(\"miss\") } }"
    tree = parse_ok(src)
    body = tree["items"][0]["body"]
    expr = first_expr_in_body(body)
    assert expr["_kind"] == "if_let"
    assert expr["pat"]["_kind"] == "pat_tuple_struct"
    assert expr["pat"]["path"] == ["Some"]


def test_question_propagate():
    # A7: ``?`` postfix produces a propagate node.
    src = "fn k() -> I32!ErrTy { let x = work()?\n x }"
    tree = parse_ok(src)
    let_stmt = tree["items"][0]["body"]["stmts"][0]
    assert let_stmt["_kind"] == "let"
    assert let_stmt["value"]["_kind"] == "propagate"


def test_ask_sugar():
    # A12: ``expr?Msg(args)`` same-line ask sugar.
    src = "fn k(c: Counter) -> I64 { c?Inc() }"
    tree = parse_ok(src)
    tail = tree["items"][0]["body"]["tail"]
    assert tail["_kind"] == "ask"
    assert tail["msg"] == "Inc"


def test_send_sugar():
    src = "fn k(c: Counter) { c!Inc(5) }"
    tree = parse_ok(src)
    expr = first_expr_in_body(tree["items"][0]["body"])
    assert expr["_kind"] == "send"
    assert expr["msg"] == "Inc"


def test_question_then_newline_is_propagate_only():
    # A12: a newline between postfix ``?`` and the next identifier
    # disambiguates as propagate.
    src = "fn k(c: Counter) -> I64!E { let x = c?\n Inc }"
    tree = parse_ok(src)
    let_stmt = tree["items"][0]["body"]["stmts"][0]
    assert let_stmt["value"]["_kind"] == "propagate"


def test_turbofish_value_position():
    # A2: ``Path::[T]`` in value position.
    src = "fn k() -> Vec[I32] { Vec::[I32]::new() }"
    tree = parse_ok(src)
    tail = tree["items"][0]["body"]["tail"]
    # `Vec::[I32]::new()` -> call(path_seg(turbofish(Vec, [I32]), 'new'))
    # We just verify the turbofish landed.
    found = []
    def walk(n):
        if isinstance(n, dict):
            if n.get("_kind") == "turbofish":
                found.append(n)
            for v in n.values():
                walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)
    walk(tree)
    assert found, "expected at least one turbofish node"


def test_result_sugar_in_return_type():
    # §6.3 ``T!E`` sugar.
    tree = parse_ok("fn k() -> I32!ParseErr { Ok(0) }")
    ret = tree["items"][0]["ret_type"]
    assert ret["_kind"] == "ty_result"


def test_anon_error_union_in_return_type():
    # §6.3 / §17.2 ``T!{A, B}`` anonymous error union.
    tree = parse_ok("fn k() -> Page!{NetErr, ParseErr} { todo() }")
    ret = tree["items"][0]["ret_type"]
    assert ret["_kind"] == "ty_result_union"
    err_names = [e["segments"] for e in ret["errs"]]
    assert err_names == [["NetErr"], ["ParseErr"]]


def test_block_with_for_loop():
    src = """
    fn _process(items: &[I32]) -> Unit {
      for item in items {
        _work(item)
      }
    }
    """
    tree = parse_ok(src)
    body = tree["items"][0]["body"]
    for_expr = first_expr_in_body(body)
    assert for_expr["_kind"] == "for"
    assert for_expr["pat"]["_kind"] == "pat_ident"
    assert for_expr["pat"]["name"] == "item"


def test_while_loop():
    tree = parse_ok("fn k() { while ready() { step() } }")
    expr = first_expr_in_body(tree["items"][0]["body"])
    assert expr["_kind"] == "while"


def test_loop_with_break_value():
    src = "fn k() -> I32 { loop { break 1 } }"
    tree = parse_ok(src)
    tail = tree["items"][0]["body"]["tail"]
    assert tail["_kind"] == "loop"


def test_use_dotted_path():
    tree = parse_ok("use std.http")
    u = tree["items"][0]
    assert u["_kind"] == "use"
    assert u["path"] == ["std", "http"]


def test_macro_call_marker():
    # A90: ``name!(args)`` macro marker.
    tree = parse_ok("fn k() { assert_eq!(1, 1) }")
    expr = first_expr_in_body(tree["items"][0]["body"])
    assert expr["_kind"] == "macro_call"


def test_keyword_after_dot_accepted():
    # A3: keyword tokens accepted after ``.`` as method/field names.
    tree = parse_ok("fn k(d: Dom) { d.on(\"click\", h) }")
    expr = first_expr_in_body(tree["items"][0]["body"])
    assert expr["_kind"] == "method_call"
    assert expr["name"] == "on"


def test_unsafe_block_in_expression_position():
    # §21: ``unsafe { ... }`` is a block expression.
    tree = parse_ok("fn read() -> U8 { unsafe { 0 } }")
    tail = tree["items"][0]["body"]["tail"]
    assert tail["_kind"] == "unsafe"


def test_arena_block_in_expression_position():
    tree = parse_ok("fn k() -> I32 { arena { 0 } }")
    tail = tree["items"][0]["body"]["tail"]
    assert tail["_kind"] == "arena"


def test_attributes_attach_to_following_item():
    src = "#[derive(Copy)]\nstruct P { x: I32 }"
    tree = parse_ok(src)
    s = tree["items"][0]
    assert s["_kind"] == "struct"
    assert s["attrs"], "expected attribute to be attached"


def test_pub_visibility_modifier():
    tree = parse_ok("pub fn k() {}")
    fn = tree["items"][0]
    assert fn["visibility"] == "pub"


def test_extern_block():
    src = "extern { fn sqrt(x: F64) -> F64 }"
    tree = parse_ok(src)
    e = tree["items"][0]
    assert e["_kind"] == "extern_block"
    assert e["items"][0]["_kind"] == "fn"


def test_deferred_agent_item_parses_through():
    # We don't fully parse agents (deferred to v0.12) but we round-trip
    # them as a deferred node with their body tokens preserved.
    src = """
    agent Counter {
      state: I32 = 0
      on Inc(by: I32) {
        self.state = self.state + by
      }
    }
    """
    tree = parse_ok(src)
    a = tree["items"][0]
    assert a["_kind"] == "deferred_agent"
    assert len(a["body_tokens"]) > 0


def test_protocol_item_parses_through():
    src = """
    protocol CounterApi {
      msg Inc(by: I32)
    }
    """
    tree = parse_ok(src)
    p = tree["items"][0]
    assert p["_kind"] == "deferred_protocol"


def test_supervisor_item_parses_through():
    src = """
    supervisor TopSup strategy OneForOne {
      child counter = Counter()
    }
    """
    tree = parse_ok(src)
    s = tree["items"][0]
    assert s["_kind"] == "deferred_supervisor"


def test_diagnostic_on_garbage_at_top_level():
    tree, diags = parse("@@@ not a real top-level item")
    # We still produce a tree (even if items list is empty), and we
    # emit at least one parse diagnostic.
    assert tree["_kind"] == "file"
    assert diags, "expected at least one diagnostic"
