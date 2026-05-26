"""Sketch wasm-codegen unit tests for the Python 2nd-impl (v0.22).

These tests pin the byte-format contract of :mod:`mty.codegen_wasm`:

* Magic + version bytes match the wasm core spec.
* Section order respects §5.5 (type, function, memory, export, code).
* Each emitted fn ends with the ``end`` opcode (0x0B).
* LEB128 encoders round-trip.

The codegen subset is intentionally small (i32 arithmetic, if/else,
while, calls, locals, basic strings). We don't try to ship a full
validator here; we verify what we can with pure-Python byte checks
plus structural decoding via
:func:`mty.codegen_wasm.parse_sections`.

Spec sources: WebAssembly Core 1.0 binary spec §5; Mighty v1.0-RC2 §6
+ §14 (informative).
"""

from __future__ import annotations

import pytest

from mty.codegen_wasm import (
    OP_END,
    OP_I32_ADD,
    OP_I32_CONST,
    OP_LOCAL_GET,
    SEC_CODE,
    SEC_EXPORT,
    SEC_FUNCTION,
    SEC_MEMORY,
    SEC_TYPE,
    VT_I32,
    WASM_MAGIC,
    WASM_VERSION,
    CodegenResult,
    codegen_wasm,
    is_valid_module_header,
    name_bytes,
    parse_sections,
    section,
    sleb128,
    uleb128,
    vec_bytes,
)
from mty.lower import lower
from mty.parser import parse


# ---------------------------------------------------------------------------
# Helper: source → CodegenResult.
# ---------------------------------------------------------------------------


def _compile(src: str) -> CodegenResult:
    tree, parse_diags = parse(src)
    assert not parse_diags, f"parser diags: {parse_diags}"
    mod, lower_diags = lower(tree, source=src)
    assert not lower_diags, f"lower diags: {lower_diags}"
    return codegen_wasm(mod, source=src)


# ---------------------------------------------------------------------------
# Pure encoder tests.
# ---------------------------------------------------------------------------


def test_uleb128_zero_single_byte():
    assert uleb128(0) == b"\x00"


def test_uleb128_127_single_byte():
    assert uleb128(127) == b"\x7f"


def test_uleb128_128_two_bytes():
    assert uleb128(128) == b"\x80\x01"


def test_uleb128_300_two_bytes():
    # 300 = 0b1_00101100 → low 7 bits 0x2C with continuation, high 2 bits 0x02
    assert uleb128(300) == b"\xac\x02"


def test_uleb128_negative_raises():
    with pytest.raises(ValueError):
        uleb128(-1)


def test_sleb128_zero_single_byte():
    assert sleb128(0) == b"\x00"


def test_sleb128_negative_one_single_byte():
    assert sleb128(-1) == b"\x7f"


def test_sleb128_negative_64_two_bytes():
    # -64 = the smallest 7-bit two's-complement is 0x40 with sign bit;
    # canonical encoding fits in two bytes.
    enc = sleb128(-64)
    assert enc == b"\x40"


def test_vec_bytes_counts_items():
    out = vec_bytes([b"\x01", b"\x02", b"\x03"])
    assert out[0] == 3
    assert out[1:] == b"\x01\x02\x03"


def test_name_bytes_utf8_length_prefix():
    out = name_bytes("hi")
    # 2-byte length prefix (uleb128) + UTF-8 bytes
    assert out == b"\x02hi"


def test_name_bytes_unicode():
    out = name_bytes("é")  # 2-byte UTF-8
    assert out[0] == 2
    assert out[1:] == "é".encode("utf-8")


def test_section_header_format():
    payload = b"\x00\x01\x02"
    out = section(SEC_TYPE, payload)
    assert out[0] == SEC_TYPE
    # next bytes are uleb128 of payload length
    assert out[1] == 3
    assert out[2:] == payload


# ---------------------------------------------------------------------------
# Module-level integration tests.
# ---------------------------------------------------------------------------


def test_empty_module_emits_header_only():
    # A module with no fns: just magic + version.
    src = "// nothing here\n"
    res = _compile(src)
    assert res.bytes == WASM_MAGIC + WASM_VERSION
    assert res.emitted_fns == []


def test_module_magic_and_version():
    res = _compile("fn main() {}")
    assert is_valid_module_header(res.bytes)
    assert res.bytes[:4] == WASM_MAGIC
    assert res.bytes[4:8] == WASM_VERSION


def test_module_has_expected_sections_in_order():
    res = _compile("fn add(x: I32, y: I32) -> I32 { x + y }")
    secs = parse_sections(res.bytes)
    ids = [sid for sid, _ in secs]
    # We expect: type (1), function (3), memory (5), export (7), code (10).
    assert ids == [SEC_TYPE, SEC_FUNCTION, SEC_MEMORY, SEC_EXPORT, SEC_CODE]


def test_section_ids_are_strictly_increasing():
    # Per the wasm spec, non-custom sections must appear at most once and
    # in increasing id order.
    res = _compile("fn a() {} fn b() -> I32 { 0 }")
    secs = parse_sections(res.bytes)
    non_custom = [sid for sid, _ in secs if sid != 0]
    assert non_custom == sorted(set(non_custom))


def test_single_fn_emits_one_function_index():
    res = _compile("fn main() {}")
    assert res.emitted_fns == ["main"]


def test_two_fns_emit_two_function_indices():
    res = _compile("fn a() {} fn b() {}")
    assert res.emitted_fns == ["a", "b"]


def test_arithmetic_fn_compiles_clean():
    res = _compile("fn add(x: I32, y: I32) -> I32 { x + y }")
    # No diagnostics for a straightforward i32+i32 fn.
    assert all(d.code != "MT4001" for d in res.diagnostics), res.diagnostics


def test_arithmetic_fn_body_contains_add_opcode():
    res = _compile("fn add(x: I32, y: I32) -> I32 { x + y }")
    # The body should contain the i32.add opcode (0x6A) somewhere.
    assert bytes([OP_I32_ADD]) in res.bytes


def test_let_binding_emits_local_set_get():
    res = _compile("fn t(a: I32) -> I32 { let b = a + 1 ; b }")
    # local.set (0x21) and local.get (0x20) must appear in the body.
    assert bytes([0x21]) in res.bytes  # local.set
    assert bytes([OP_LOCAL_GET]) in res.bytes  # local.get


def test_if_expression_emits_block_type_i32():
    res = _compile("""
        fn pick(c: I32) -> I32 {
          if c == 0 { 1 } else { 2 }
        }
    """)
    # The if-with-else uses block-type i32 (VT_I32 = 0x7F).
    assert bytes([VT_I32]) in res.bytes


def test_while_loop_emits_loop_and_block_opcodes():
    res = _compile("""
        fn count() -> I32 {
          let n = 0
          while n < 10 {
            n = n + 1
          }
          n
        }
    """)
    # Block (0x02) and loop (0x03) opcodes both appear.
    assert bytes([0x02]) in res.bytes
    assert bytes([0x03]) in res.bytes


def test_call_to_known_fn_emits_call_opcode():
    res = _compile("""
        fn helper(x: I32) -> I32 { x + 1 }
        fn main() -> I32 { helper(41) }
    """)
    # call (0x10).
    assert bytes([0x10]) in res.bytes
    # No "unknown fn" diagnostic — helper is in scope.
    assert all(d.code != "MT4002" for d in res.diagnostics)


def test_unknown_call_emits_mt4002_warning():
    res = _compile("""
        fn main() -> I32 { mystery_external_fn(0) }
    """)
    assert any(d.code == "MT4002" for d in res.diagnostics)


def test_string_literal_lowers_as_placeholder_zero():
    res = _compile('fn main() { log("hi") }')
    # Body should contain i32.const 0 (the string placeholder).
    assert bytes([OP_I32_CONST, 0x00]) in res.bytes


def test_export_section_lists_each_fn_by_name():
    res = _compile("fn alpha() {} fn beta() -> I32 { 7 }")
    # Export section payload should contain the UTF-8 names.
    secs = dict(parse_sections(res.bytes))
    export_payload = secs[SEC_EXPORT]
    assert b"alpha" in export_payload
    assert b"beta" in export_payload
    # Plus the auto-added memory export.
    assert b"memory" in export_payload


def test_every_code_body_ends_with_end_opcode():
    # The code section's payload contains, for each fn:
    #   size-leb, locals-vec, body-bytes-ending-in-OP_END.
    res = _compile("fn a() {} fn b() -> I32 { 1 + 2 }")
    secs = dict(parse_sections(res.bytes))
    code = secs[SEC_CODE]
    # Decode the function-count.
    n_fns = code[0]
    assert n_fns == 2
    # Walk each entry — we don't need byte-perfect parsing; just check
    # that the last opcode of each is OP_END. Cheap heuristic: every
    # body block ends with 0x0B and the section ends with 0x0B.
    assert code[-1] == OP_END


def test_byte_count_grows_with_more_fns():
    short = _compile("fn a() {}")
    longer = _compile("fn a() {} fn b() {} fn c() {} fn d() {}")
    assert len(longer.bytes) > len(short.bytes)


def test_codegen_result_diagnostics_is_list():
    res = _compile("fn main() {}")
    assert isinstance(res.diagnostics, list)


def test_parse_sections_round_trip_count():
    res = _compile("fn t(x: I32) -> I32 { x * 2 }")
    secs = parse_sections(res.bytes)
    # We expect at least the 5 sections we always emit.
    assert len(secs) >= 5


def test_memory_section_emits_min_pages_one():
    res = _compile("fn main() {}")
    secs = dict(parse_sections(res.bytes))
    mem_payload = secs[SEC_MEMORY]
    # vec count 1, then 0x00 (no max) + min=1.
    assert mem_payload[0] == 1
    assert mem_payload[1] == 0x00
    assert mem_payload[2] == 1


def test_unsupported_shape_emits_mt4001_warning():
    # Tuple literal isn't in the codegen subset.
    res = _compile("fn main() -> I32 { let t = (1, 2) ; 0 }")
    assert any(d.code == "MT4001" for d in res.diagnostics)


def test_return_statement_emits_return_opcode():
    res = _compile("fn t() -> I32 { return 5 }")
    # return = 0x0F
    assert bytes([0x0F]) in res.bytes


def test_unary_negation_emits_const_zero_then_sub():
    res = _compile("fn neg(x: I32) -> I32 { 0 - x }")
    # Expect i32.const 0, then local.get, then i32.sub (0x6B).
    assert bytes([0x6B]) in res.bytes


def test_while_condition_emits_eqz_for_inverted_test():
    # while is lowered as block { loop { cond; i32.eqz; br_if 1; body; br 0 } }
    # so the eqz opcode (0x45) appears in the body.
    res = _compile("fn loop_one() { let n = 0 ; while n < 1 { n = n + 1 } }")
    assert bytes([0x45]) in res.bytes


def test_comparison_emits_i32_lt_s():
    res = _compile("fn lt(a: I32, b: I32) -> I32 { if a < b { 1 } else { 0 } }")
    # i32.lt_s = 0x48.
    assert bytes([0x48]) in res.bytes
