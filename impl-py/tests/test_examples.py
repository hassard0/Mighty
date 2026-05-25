"""Sweep the canonical ``examples/01..20.mty`` corpus through the lexer
and parser.

These tests are the load-bearing integration: every official Mighty
example must lex with zero diagnostics, and parse with zero
diagnostics. If either fails, either the spec has drifted from the
examples or this Python impl has misread the spec. Both outcomes are
recorded as findings in ``dev/history/notes/PYTHON_IMPL_V0_11_NOTES.md``.

Per the swarm scope the parser is REQUIRED to cover examples 01-05
clean; everything else is bonus but tested here for completeness.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from mty.lexer import lex, strip_trivia
from mty.parser import parse


REQUIRED_LEXER_CLEAN = {
    "01_hello.mty", "02_struct_enum.mty", "03_generic_fn.mty",
    "04_result_propagation.mty", "05_match_expr.mty",
    "06_for_while_loop.mty", "07_agent_echo.mty", "08_agent_state.mty",
    "09_send_ask_deadline.mty", "10_supervisor.mty",
    "11_budget_block.mty", "12_arena.mty", "13_capabilities.mty",
    "14_extern_c.mty", "15_extern_js.mty", "16_macro.mty",
    "17_unsafe.mty", "18_sandbox.mty", "19_backend_service.mty",
    "20_frontend_component.mty",
}

REQUIRED_PARSER_CLEAN = {
    # Bare minimum the swarm scope mandates.
    "01_hello.mty", "02_struct_enum.mty", "03_generic_fn.mty",
    "04_result_propagation.mty", "05_match_expr.mty",
    # Stretch goals -- all currently passing, lock them in so future
    # parser changes can't silently regress.
    "06_for_while_loop.mty", "07_agent_echo.mty", "08_agent_state.mty",
    "09_send_ask_deadline.mty", "10_supervisor.mty",
    "11_budget_block.mty", "12_arena.mty", "13_capabilities.mty",
    "14_extern_c.mty", "15_extern_js.mty", "16_macro.mty",
    "17_unsafe.mty", "18_sandbox.mty", "19_backend_service.mty",
    "20_frontend_component.mty",
}


def _all_example_files() -> list[Path]:
    workspace = Path(__file__).resolve().parents[2]
    examples = workspace / "examples"
    return sorted(examples.glob("*.mty"))


@pytest.mark.parametrize("path", _all_example_files(), ids=lambda p: p.name)
def test_example_lexes_cleanly(path: Path):
    src = path.read_text(encoding="utf-8")
    tokens, diags = lex(src)
    # Every example must end with an EOF sentinel.
    assert tokens[-1].kind.name == "EOF"
    if path.name in REQUIRED_LEXER_CLEAN:
        assert not diags, (
            f"{path.name}: unexpected lexer diagnostics: "
            f"{[(d.code, d.message) for d in diags]}"
        )


@pytest.mark.parametrize("path", _all_example_files(), ids=lambda p: p.name)
def test_example_parses_cleanly(path: Path):
    src = path.read_text(encoding="utf-8")
    tree, diags = parse(src)
    assert tree["_kind"] == "file"
    assert len(tree["items"]) > 0, f"{path.name}: parsed zero top-level items"
    if path.name in REQUIRED_PARSER_CLEAN:
        assert not diags, (
            f"{path.name}: unexpected parser diagnostics: "
            f"{[(d.code, d.message[:120]) for d in diags]}"
        )


def test_every_example_has_at_least_one_item():
    # Belt-and-braces: ensures the iteration above is actually running
    # over a non-empty set of files (catches a botched glob).
    files = _all_example_files()
    assert len(files) >= 20, f"expected >= 20 examples, found {len(files)}"


def test_round_trip_token_text_matches_source():
    # The concatenation of every token's text (including trivia) must
    # equal the CRLF-normalised source. This is the byte-level
    # round-trip invariant the spec implies in §3.1 ("the lexer
    # normalises CRLF to LF before tokenisation").
    for path in _all_example_files():
        src = path.read_text(encoding="utf-8")
        normalised = src.replace("\r\n", "\n").lstrip("﻿")
        tokens, _ = lex(src)
        rebuilt = "".join(t.text for t in tokens)
        assert rebuilt == normalised, f"{path.name}: round-trip mismatch"
