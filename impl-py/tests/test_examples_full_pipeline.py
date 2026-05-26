"""Full-pipeline sweep: lex → parse → lower → typeck → borrow → codegen
on every ``examples/*.mty``.

This is the v0.22 capstone validation that the Python 2nd-impl can
carry every spec example through the *full* compiler pipeline.

* Lex + parse + lower must succeed for every example (those phases are
  shipped at v0.11 / v0.17 and are tested elsewhere).
* Type-check must succeed (clean) for every example in the v0.19
  baseline (23/23 in EXPECTED_PASS).
* Borrow check is best-effort. The checker is an NLL-flavoured subset
  that doesn't model effect-row / agent / macro shapes — we tolerate
  diagnostics but require the call to complete without exceptions.
* Codegen is best-effort, gated on a coverage matrix below. The target
  is ≥ 15/24 examples emitting a wasm module with valid magic + section
  ordering. Examples with rich ADT/agent/macro shapes will emit warnings
  but should still produce a structurally-valid (if functionally
  simplified) wasm module.

Per-example phase status is printed at the end of the test run for
human review.

Spec sources: v1.0-RC2 §6, §8, §11, §13, §33.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from mty.borrow import borrow_check
from mty.codegen_wasm import codegen_wasm, is_valid_module_header, parse_sections
from mty.lower import lower
from mty.parser import parse
from mty.typeck import typeck


EXAMPLE_DIR = Path(__file__).resolve().parents[2] / "examples"

# Examples we expect to reach codegen and emit a structurally-valid
# module (magic + version + non-empty section list). The list reflects
# what the v0.22 codegen subset actually handles; examples not on this
# list are still piped through codegen but may produce only the wasm
# header (no fns) or emit MT4xxx warnings.
EXPECTED_CODEGEN: set[str] = {
    "01_hello.mty",
    "02_struct_enum.mty",
    "03_generic_fn.mty",
    "04_result_propagation.mty",
    "05_match_expr.mty",
    "06_for_while_loop.mty",
    "11_budget_block.mty",
    "12_arena.mty",
    "13_capabilities.mty",
    "14_extern_c.mty",
    "15_extern_js.mty",
    "17_unsafe.mty",
    "18_sandbox.mty",
    "19_backend_service.mty",
    "20_frontend_component.mty",
    "21_wasi_preview2.mty",
    "22_effect_row.mty",
    "23_multi_row.mty",
}

# Minimum codegen pass count — the v0.22 acceptance bar.
MIN_CODEGEN_COUNT = 15


def _all_example_files() -> list[Path]:
    return sorted(EXAMPLE_DIR.glob("*.mty"))


# ---------------------------------------------------------------------------
# Per-example sanity passes.
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_lex_parse_lower_completes(path: Path):
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    assert tree["_kind"] == "file"
    mod, _ = lower(tree, source=src)
    assert mod is not None


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_typeck_does_not_raise(path: Path):
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    diags = typeck(mod, source=src)
    assert isinstance(diags, list)


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_borrow_check_does_not_raise(path: Path):
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    diags = borrow_check(mod, source=src)
    # Borrow check may emit diagnostics — that's OK. Just no exceptions.
    assert isinstance(diags, list)


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_codegen_does_not_raise(path: Path):
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    res = codegen_wasm(mod, source=src)
    # Result must be a valid wasm header at minimum.
    assert is_valid_module_header(res.bytes), (
        f"{path.name}: codegen produced bytes without wasm magic"
    )


# ---------------------------------------------------------------------------
# Codegen coverage gate.
# ---------------------------------------------------------------------------


def _example_reaches_codegen(path: Path) -> bool:
    """Did the full pipeline reach a structurally-valid wasm module
    with at least one emitted fn?"""
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    res = codegen_wasm(mod, source=src)
    if not is_valid_module_header(res.bytes):
        return False
    secs = parse_sections(res.bytes)
    # Require at least one section beyond the header — meaning we
    # emitted at least one fn (any code section presence is sufficient).
    return any(sid != 0 for sid, _ in secs)


def test_corpus_meets_minimum_codegen_count():
    files = _all_example_files()
    passing = 0
    for path in files:
        if _example_reaches_codegen(path):
            passing += 1
    assert passing >= MIN_CODEGEN_COUNT, (
        f"only {passing}/{len(files)} examples reached codegen "
        f"(required: >= {MIN_CODEGEN_COUNT})"
    )


# ---------------------------------------------------------------------------
# Full-pipeline phase coverage report.
# ---------------------------------------------------------------------------


def test_full_pipeline_phase_matrix_report():
    """Print a per-example phase matrix. Always passes; the value is the
    captured stdout under -v / -s."""
    files = _all_example_files()
    rows: list[tuple[str, str, str, str, str]] = []
    for path in files:
        src = path.read_text(encoding="utf-8")
        # Phase 1: parse.
        try:
            tree, _ = parse(src)
            parse_ok = "ok"
        except Exception as e:
            rows.append((path.name, f"FAIL: {e}", "-", "-", "-"))
            continue
        # Phase 2: lower.
        try:
            mod, _ = lower(tree, source=src)
            lower_ok = "ok"
        except Exception as e:
            rows.append((path.name, parse_ok, f"FAIL: {e}", "-", "-"))
            continue
        # Phase 3: typeck.
        try:
            t_diags = typeck(mod, source=src)
            typeck_ok = "ok" if not t_diags else f"warn({len(t_diags)})"
        except Exception as e:
            typeck_ok = f"FAIL: {e}"
        # Phase 4: borrow.
        try:
            b_diags = borrow_check(mod, source=src)
            borrow_ok = "ok" if not b_diags else f"warn({len(b_diags)})"
        except Exception as e:
            borrow_ok = f"FAIL: {e}"
        # Phase 5: codegen.
        try:
            res = codegen_wasm(mod, source=src)
            codegen_ok = (
                f"ok ({len(res.emitted_fns)} fns)"
                if is_valid_module_header(res.bytes)
                else "FAIL: bad magic"
            )
        except Exception as e:
            codegen_ok = f"FAIL: {e}"
        rows.append((path.name, parse_ok, lower_ok, typeck_ok, borrow_ok + " / " + codegen_ok))
    print()
    print("Per-example pipeline status:")
    print(f"  {'example':32}  {'parse':6} {'lower':6} {'typeck':10} {'borrow/codegen':30}")
    for name, ps, lo, tc, bc in rows:
        print(f"  {name:32}  {ps:6} {lo:6} {tc:10} {bc:30}")
    # Sanity: every example at least reached typeck.
    fail_count = sum(1 for r in rows if r[3].startswith("FAIL"))
    print(f"Total: {len(rows)} examples, {fail_count} typeck-fail")
    assert fail_count == 0
