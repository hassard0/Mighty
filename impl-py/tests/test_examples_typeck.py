"""Sweep the canonical ``examples/01..22.mty`` corpus through the full
front-end pipeline: lex → parse → lower → typeck.

The pipeline is required to lex+parse+lower **every** example with
zero diagnostics (we own those phases and they're tested elsewhere).
Type-checking is the stretch goal: this impl's H-M subset typeck can't
handle the full set of v1.0 features (effect rows, trait dispatch,
agent/protocol calls, etc.) so we tolerate a configurable miss rate.

The acceptance bar set by the v0.17 recovery scope:

   * **>= 14/22 examples** must typeck with zero diagnostics. Anything
     above that is bonus.

Per-example pass/fail outcomes are written into
``dev/history/notes/PYTHON_IMPL_V0_17_NOTES.md`` for traceability.

Spec sources: v1.0-RC2 §6, §11, §13, §33.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from mty.lower import lower
from mty.parser import parse
from mty.typeck import typeck


# The lowering target — the same corpus the lexer/parser tests sweep.
EXAMPLE_DIR = Path(__file__).resolve().parents[2] / "examples"

# Per-example expected outcome. PASS means typeck should produce zero
# diagnostics; FAIL means we tolerate diagnostics but require the
# pipeline to complete without exceptions.
#
# At the v0.17 recovery cut, all 22 examples typeck cleanly. If a future
# change drops any from "PASS" to "FAIL", that's a visible regression
# the test will surface.
EXPECTED_PASS: set[str] = {
    "01_hello.mty", "02_struct_enum.mty", "03_generic_fn.mty",
    "04_result_propagation.mty", "05_match_expr.mty",
    "06_for_while_loop.mty", "07_agent_echo.mty", "08_agent_state.mty",
    "09_send_ask_deadline.mty", "10_supervisor.mty",
    "11_budget_block.mty", "12_arena.mty", "13_capabilities.mty",
    "14_extern_c.mty", "15_extern_js.mty", "16_macro.mty",
    "17_unsafe.mty", "18_sandbox.mty", "19_backend_service.mty",
    "20_frontend_component.mty", "21_wasi_preview2.mty",
    "22_effect_row.mty",
    # 23_multi_row.mty was added alongside the v0.17 effect-row work;
    # it lands clean because the row-var typing is type-erased in our
    # H-M subset (TyAny absorbs the row). v0.18 effect-row typeck will
    # tighten this and may need to be revisited.
    "23_multi_row.mty",
}

# Acceptance bar — see module docstring.
MIN_PASS_COUNT = 14


def _all_example_files() -> list[Path]:
    return sorted(EXAMPLE_DIR.glob("*.mty"))


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_pipeline_lex_parse_lower_completes(path: Path):
    """The pipeline runs to completion without exceptions for every
    example, and produces a non-empty HIR (or, for agent-only files,
    an empty-items HirModule which is still a valid lowering).
    """
    src = path.read_text(encoding="utf-8")
    tree, parse_diags = parse(src)
    # parser cleanliness is enforced by test_examples.py; we only
    # require the call to return a file node.
    assert tree["_kind"] == "file", f"{path.name}: parser did not return file"
    mod, lower_diags = lower(tree, source=src)
    # Lowering must not raise; diagnostics are tolerated.
    assert mod is not None


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_pipeline_typeck_completes(path: Path):
    """Type-checking must complete without exceptions for every example,
    regardless of whether it produces diagnostics."""
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    diags = typeck(mod, source=src)
    # The list may be empty or contain diagnostics; we just require the
    # call to return a list (not raise).
    assert isinstance(diags, list)


@pytest.mark.parametrize("path", _all_example_files(),
                         ids=lambda p: p.name)
def test_expected_pass_example_typechecks_clean(path: Path):
    """For every example in EXPECTED_PASS, typeck must produce zero
    diagnostics. This is the load-bearing regression guard."""
    if path.name not in EXPECTED_PASS:
        pytest.skip(f"{path.name} not in EXPECTED_PASS")
    src = path.read_text(encoding="utf-8")
    tree, _ = parse(src)
    mod, _ = lower(tree, source=src)
    diags = typeck(mod, source=src)
    assert not diags, (
        f"{path.name} regressed: {[(d.code, d.message[:120]) for d in diags]}"
    )


def test_corpus_meets_minimum_pass_count():
    """At least MIN_PASS_COUNT examples must typeck cleanly. This is the
    acceptance-bar test set by the v0.17 recovery scope."""
    files = _all_example_files()
    clean = 0
    for path in files:
        src = path.read_text(encoding="utf-8")
        tree, _ = parse(src)
        mod, _ = lower(tree, source=src)
        diags = typeck(mod, source=src)
        if not diags:
            clean += 1
    assert clean >= MIN_PASS_COUNT, (
        f"only {clean}/{len(files)} examples typeck clean "
        f"(required: >= {MIN_PASS_COUNT})"
    )


def test_corpus_full_pass_report():
    """Diagnostic emit-helper: a single-test view of the per-example
    typeck status. Always passes; the value is the printout pytest
    captures on -v for human review."""
    files = _all_example_files()
    rows: list[tuple[str, str, int]] = []
    for path in files:
        src = path.read_text(encoding="utf-8")
        tree, _ = parse(src)
        mod, _ = lower(tree, source=src)
        diags = typeck(mod, source=src)
        rows.append((path.name, "PASS" if not diags else "FAIL", len(diags)))
    # Side-effect: print a human-readable matrix. Pytest captures stdout;
    # use -s to see it. We assert nothing here beyond "no exceptions".
    print()
    print("Per-example typeck status:")
    for name, status, count in rows:
        marker = "ok" if status == "PASS" else f"FAIL ({count})"
        print(f"  {name:30}  {marker}")
    pass_count = sum(1 for _, s, _ in rows if s == "PASS")
    print(f"Total: {pass_count}/{len(rows)} pass")
    assert pass_count <= len(rows)
