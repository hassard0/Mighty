"""Unit tests for ``mty.lexer``.

Spec source: ``docs/spec/v1.0-rc.md`` §3.
"""

from __future__ import annotations

import pytest

from mty.diagnostics import (
    CODE_LEX_BOM_REJECTED,
    CODE_LEX_UNKNOWN_CHAR,
    CODE_LEX_UNTERMINATED_BLOCK_COMMENT,
    CODE_LEX_UNTERMINATED_STRING,
)
from mty.lexer import KEYWORDS, Token, TokenKind, lex, strip_trivia


def kinds(src: str) -> list[TokenKind]:
    tokens, _ = lex(src)
    return [t.kind for t in strip_trivia(tokens)]


def texts(src: str) -> list[str]:
    tokens, _ = lex(src)
    return [t.text for t in strip_trivia(tokens) if t.kind != TokenKind.EOF]


# ---------------------------------------------------------------------------
# §3.1 encoding
# ---------------------------------------------------------------------------


def test_bom_rejected():
    src = "﻿fn main() {}"
    _, diags = lex(src)
    assert any(d.code == CODE_LEX_BOM_REJECTED for d in diags)


def test_crlf_normalised():
    # The lexer normalises CRLF to LF before tokenisation (§3.1). The
    # token stream contains a single NEWLINE per source line break.
    tokens, _ = lex("a\r\nb")
    nls = [t for t in tokens if t.kind == TokenKind.NEWLINE]
    assert len(nls) == 1


# ---------------------------------------------------------------------------
# §3.2 comments
# ---------------------------------------------------------------------------


def test_line_comment():
    tokens, _ = lex("// hello\nfn")
    kinds_ = [t.kind for t in tokens]
    assert TokenKind.LINE_COMMENT in kinds_


def test_doc_comment():
    tokens, _ = lex("/// docstring\nfn")
    assert any(t.kind == TokenKind.DOC_COMMENT for t in tokens)


def test_block_comment_nested():
    # §3.2: "nesting allowed".
    src = "/* outer /* inner */ still-outer */"
    tokens, diags = lex(src)
    assert diags == []
    assert any(t.kind == TokenKind.BLOCK_COMMENT for t in tokens)


def test_block_comment_unterminated():
    _, diags = lex("/* never closed")
    assert any(d.code == CODE_LEX_UNTERMINATED_BLOCK_COMMENT for d in diags)


# ---------------------------------------------------------------------------
# §3.3 identifiers and keywords
# ---------------------------------------------------------------------------


def test_keyword_set_size():
    # Spec §3.3 freezes a specific keyword set. We sanity-check our
    # KEYWORDS set is non-empty and contains the canonical ones.
    for kw in ("fn", "let", "if", "match", "agent", "protocol", "Self"):
        assert kw in KEYWORDS


def test_identifier_with_underscore_start():
    tokens, _ = lex("_classify")
    nonwhite = strip_trivia(tokens)
    assert nonwhite[0].kind == TokenKind.IDENT
    assert nonwhite[0].text == "_classify"


def test_lone_underscore_is_special():
    tokens, _ = lex("_")
    nonwhite = strip_trivia(tokens)
    assert nonwhite[0].kind == TokenKind.UNDERSCORE


def test_unicode_identifier_accepted():
    # §3.3: IDENT = (XID_Start | "_") (XID_Continue | "_")*. Greek alpha
    # is XID_Start so this should lex as an IDENT, not an unknown char.
    tokens, diags = lex("αβ = 1")
    assert diags == []
    nonwhite = strip_trivia(tokens)
    assert nonwhite[0].kind == TokenKind.IDENT
    assert nonwhite[0].text == "αβ"


# ---------------------------------------------------------------------------
# §3.4.1 integers
# ---------------------------------------------------------------------------


def test_decimal_int():
    assert kinds("42")[0] == TokenKind.INT_LITERAL


def test_int_with_underscore_separator():
    tokens, diags = lex("1_000_000")
    assert diags == []
    assert tokens[0].kind == TokenKind.INT_LITERAL
    assert tokens[0].text == "1_000_000"


def test_int_with_typed_suffix():
    for src in ("42i32", "42u8", "1_000u64", "0i128", "8usize"):
        tokens, _ = lex(src)
        assert tokens[0].kind == TokenKind.INT_LITERAL, (src, tokens[0])
        assert tokens[0].text == src


def test_hex_oct_bin_int():
    assert texts("0xff")[0] == "0xff"
    assert texts("0xFF_AB")[0] == "0xFF_AB"
    assert texts("0o755")[0] == "0o755"
    assert texts("0b1010")[0] == "0b1010"


# ---------------------------------------------------------------------------
# §3.4.2 floats
# ---------------------------------------------------------------------------


def test_simple_float():
    tokens, _ = lex("3.14")
    assert tokens[0].kind == TokenKind.FLOAT_LITERAL


def test_float_with_suffix():
    for src in ("3.14f32", "1.0f64"):
        tokens, _ = lex(src)
        assert tokens[0].kind == TokenKind.FLOAT_LITERAL, src
        assert tokens[0].text == src


def test_int_dotdot_int_is_range_not_float():
    # ``1..10`` is two integers + a ``..`` token, not ``1.``+`.10`.
    tokens, _ = lex("1..10")
    nonwhite = strip_trivia(tokens)
    assert nonwhite[0].kind == TokenKind.INT_LITERAL
    assert nonwhite[1].kind == TokenKind.DOT_DOT
    assert nonwhite[2].kind == TokenKind.INT_LITERAL


# ---------------------------------------------------------------------------
# §3.4.3 strings, chars, raw, byte, html
# ---------------------------------------------------------------------------


def test_string_with_escape():
    tokens, diags = lex(r'"hello\nworld"')
    assert diags == []
    assert tokens[0].kind == TokenKind.STRING_LITERAL


def test_unterminated_string():
    _, diags = lex('"oops')
    assert any(d.code == CODE_LEX_UNTERMINATED_STRING for d in diags)


def test_raw_string():
    tokens, diags = lex('r"raw \\n bytes"')
    assert diags == []
    assert tokens[0].kind == TokenKind.RAW_STRING_LITERAL


def test_raw_string_with_hashes():
    tokens, diags = lex('r#"contains "quotes" inside"#')
    assert diags == []
    assert tokens[0].kind == TokenKind.RAW_STRING_LITERAL


def test_byte_string():
    tokens, _ = lex('b"bytes"')
    assert tokens[0].kind == TokenKind.BYTE_STRING_LITERAL


def test_char_literal():
    tokens, _ = lex("'c'")
    assert tokens[0].kind == TokenKind.CHAR_LITERAL


def test_html_literal_simple():
    tokens, diags = lex('html"<div>hello</div>"')
    assert diags == []
    assert tokens[0].kind == TokenKind.HTML_LITERAL


def test_html_literal_multiline_with_interpolation():
    # §22.2: html"..." is a tagged template; we lex it as a single token.
    src = '''html"
    <div class='greeting'>
        Hello, {name}!
    </div>
"'''
    tokens, diags = lex(src)
    assert diags == []
    assert tokens[0].kind == TokenKind.HTML_LITERAL


# ---------------------------------------------------------------------------
# §3.4.4 / §3.4.5 size + duration literals
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("src", ["1B", "16KiB", "64MiB", "1GiB", "5k", "10M"])
def test_size_literal(src):
    tokens, _ = lex(src)
    assert tokens[0].kind == TokenKind.SIZE_LITERAL, src
    assert tokens[0].text == src


@pytest.mark.parametrize("src", ["1ns", "100us", "500ms", "2s", "5m", "1h"])
def test_duration_literal(src):
    tokens, _ = lex(src)
    assert tokens[0].kind == TokenKind.DURATION_LITERAL, src
    assert tokens[0].text == src


def test_size_k_vs_duration_m():
    # §3.4.4 A1: lowercase ``k`` is decimal-kilo, lowercase ``m`` is
    # the duration MINUTE suffix. ``2m`` therefore lexes as a
    # DURATION_LITERAL, not a SIZE_LITERAL.
    tokens, _ = lex("2m")
    assert tokens[0].kind == TokenKind.DURATION_LITERAL


def test_size_k_lowercase_kilo():
    tokens, _ = lex("5k")
    assert tokens[0].kind == TokenKind.SIZE_LITERAL


# ---------------------------------------------------------------------------
# §3.5 punctuation
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("src,kind", [
    ("(", TokenKind.LPAREN),
    (")", TokenKind.RPAREN),
    ("{", TokenKind.LBRACE),
    ("}", TokenKind.RBRACE),
    ("[", TokenKind.LBRACKET),
    ("]", TokenKind.RBRACKET),
    (",", TokenKind.COMMA),
    (";", TokenKind.SEMI),
    ("::", TokenKind.COLON_COLON),
    ("->", TokenKind.ARROW),
    ("=>", TokenKind.FAT_ARROW),
    ("==", TokenKind.EQ_EQ),
    ("!=", TokenKind.NE),
    ("..", TokenKind.DOT_DOT),
    ("..=", TokenKind.DOT_DOT_EQ),
    ("@", TokenKind.AT),
    ("#", TokenKind.HASH),
    ("?", TokenKind.QUESTION),
    ("!", TokenKind.BANG),
    ("&&", TokenKind.AMP_AMP),
    ("||", TokenKind.PIPE_PIPE),
])
def test_single_punct(src, kind):
    tokens, _ = lex(src)
    assert tokens[0].kind == kind, (src, tokens[0])


def test_turbofish_opener():
    # A2: ``::[`` is the turbofish opener.
    tokens, _ = lex("Map::[Str, Json]{}")
    nonwhite = strip_trivia(tokens)
    assert nonwhite[0].kind == TokenKind.IDENT  # Map
    assert nonwhite[1].kind == TokenKind.COLON_COLON_LBRACKET
    assert nonwhite[1].text == "::["


# ---------------------------------------------------------------------------
# Unknown character diagnostics
# ---------------------------------------------------------------------------


def test_unknown_char_reports():
    _, diags = lex("\x01")
    assert any(d.code == CODE_LEX_UNKNOWN_CHAR for d in diags)


# ---------------------------------------------------------------------------
# Round-trip: token texts concatenate to the (normalised) source.
# ---------------------------------------------------------------------------


def test_token_texts_reconstruct_source():
    src = "fn main() {\n  log(\"hello\")\n}\n"
    tokens, _ = lex(src)
    rebuilt = "".join(t.text for t in tokens)
    assert rebuilt == src


def test_keyword_text_matches_keyword_field():
    # Every KEYWORD token has its keyword field set to its source text.
    tokens, _ = lex("fn if let match agent protocol")
    for t in strip_trivia(tokens):
        if t.kind == TokenKind.KEYWORD:
            assert t.keyword == t.text
