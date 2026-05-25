"""Pure-Python Mighty lexer.

Spec source: ``docs/spec/v1.0-rc.md`` §3 (Lexical structure).

Output: a list of :class:`Token` instances. Whitespace and comments are
emitted as their own token kinds (a downstream parser may filter them);
this matches the spirit of a CST-style token stream where every byte of
source is accounted for.

Design notes (Python-impl decisions where the spec is silent or
implementation-defined):

* We classify keywords vs identifiers at lex time. The spec (§3.3)
  fixes the keyword set.
* Numeric literals support the spec's underscore separators between
  digit groups (``1_000_000``). The grammar in §3.4.1 places the
  separator inside the digit group only -- we accept it after the first
  digit and forbid two underscores in a row to avoid ambiguity. This
  is documented as a deliberate interpretation in PYTHON_IMPL_V0_11_NOTES.md.
* Duration and size literals collide with integer-suffix parsing
  (``1024k`` could be size-literal or ``1024`` followed by identifier
  ``k``). Spec §3.4.4/§3.4.5 are unambiguous when the suffix is one of
  the listed tokens (B/KiB/MiB/GiB/k/M for size, ns/us/ms/s/m/h for
  duration). We greedily match the longest valid suffix.
* The integer-suffix grammar uses ``u``/``i`` + width. We require the
  suffix to NOT be followed by an identifier-continue character so
  ``42u8`` lexes as one token but ``42uall`` would not absorb ``uall``.
* String escapes mirror Rust convention (``\\n``, ``\\t``, ``\\r``,
  ``\\\\``, ``\\"``, ``\\'``, ``\\0``, ``\\xNN``, ``\\u{...}``).
  Spec §3.4.3 calls these "the usual escape grammar" without
  enumerating; we adopt the Rust-style escape set as the most
  conservative interpretation.
* HTML literal (``html"..."``) is treated as a tagged-template form
  (§3.4.3, §22.2). We lex the surrounding ``html"..."`` as a single
  token kind ``HTML_LITERAL``; interpolation parsing (``{name}``) is
  the parser's job.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, auto
from typing import Iterable, Optional

from .diagnostics import (
    CODE_LEX_BAD_CHAR_LITERAL,
    CODE_LEX_BAD_NUMBER,
    CODE_LEX_BOM_REJECTED,
    CODE_LEX_UNKNOWN_CHAR,
    CODE_LEX_UNTERMINATED_BLOCK_COMMENT,
    CODE_LEX_UNTERMINATED_STRING,
    Diagnostic,
    Severity,
)


# ---------------------------------------------------------------------------
# Token kinds
# ---------------------------------------------------------------------------


class TokenKind(Enum):
    # Trivia
    WHITESPACE = auto()
    NEWLINE = auto()
    LINE_COMMENT = auto()
    BLOCK_COMMENT = auto()
    DOC_COMMENT = auto()

    # Literals
    INT_LITERAL = auto()
    FLOAT_LITERAL = auto()
    STRING_LITERAL = auto()
    RAW_STRING_LITERAL = auto()
    BYTE_STRING_LITERAL = auto()
    CHAR_LITERAL = auto()
    HTML_LITERAL = auto()
    SIZE_LITERAL = auto()
    DURATION_LITERAL = auto()

    # Words
    IDENT = auto()
    KEYWORD = auto()

    # Punctuation
    LPAREN = auto()       # (
    RPAREN = auto()       # )
    LBRACE = auto()       # {
    RBRACE = auto()       # }
    LBRACKET = auto()     # [
    RBRACKET = auto()     # ]
    COMMA = auto()        # ,
    SEMI = auto()         # ;
    COLON = auto()        # :
    COLON_COLON = auto()  # ::
    COLON_COLON_LBRACKET = auto()  # ::[  (turbofish open, A2)
    DOT = auto()          # .
    DOT_DOT = auto()      # ..
    DOT_DOT_EQ = auto()   # ..=
    DOT_DOT_DOT = auto()  # ...
    ARROW = auto()        # ->
    FAT_ARROW = auto()    # =>
    AT = auto()           # @
    HASH = auto()         # #
    DOLLAR = auto()       # $
    QUESTION = auto()     # ?
    BANG = auto()         # !
    AMP = auto()          # &
    AMP_AMP = auto()      # &&
    AMP_MUT = auto()      # &mut  -- not a token; handled at parser
    PIPE = auto()         # |
    PIPE_PIPE = auto()    # ||
    CARET = auto()        # ^
    TILDE = auto()        # ~

    EQ = auto()           # =
    EQ_EQ = auto()        # ==
    NE = auto()           # !=
    LT = auto()           # <
    LE = auto()           # <=
    GT = auto()           # >
    GE = auto()           # >=
    LSHIFT = auto()       # <<
    RSHIFT = auto()       # >>

    PLUS = auto()         # +
    PLUS_EQ = auto()      # +=
    MINUS = auto()        # -
    MINUS_EQ = auto()     # -=
    STAR = auto()         # *
    STAR_EQ = auto()      # *=
    SLASH = auto()        # /
    SLASH_EQ = auto()     # /=
    PERCENT = auto()      # %
    PERCENT_EQ = auto()   # %=
    AMP_EQ = auto()       # &=
    PIPE_EQ = auto()      # |=
    CARET_EQ = auto()     # ^=

    UNDERSCORE = auto()   # bare _

    # End-of-file sentinel
    EOF = auto()


# ---------------------------------------------------------------------------
# Token
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Token:
    """A single lexed token.

    ``text`` is the exact source slice (so concatenating all tokens
    reproduces the file byte-for-byte, modulo the LF normalisation
    described in §3.1). ``keyword`` is non-empty only when ``kind`` is
    ``KEYWORD``.
    """

    kind: TokenKind
    text: str
    start: int
    end: int
    keyword: str = ""

    def __repr__(self) -> str:  # pragma: no cover - cosmetic
        body = self.text.replace("\n", "\\n")
        if len(body) > 24:
            body = body[:21] + "..."
        kw = f" kw={self.keyword!r}" if self.keyword else ""
        return f"Token({self.kind.name} {body!r}{kw} @{self.start}..{self.end})"

    @property
    def is_trivia(self) -> bool:
        return self.kind in _TRIVIA


_TRIVIA = {
    TokenKind.WHITESPACE,
    TokenKind.NEWLINE,
    TokenKind.LINE_COMMENT,
    TokenKind.BLOCK_COMMENT,
    TokenKind.DOC_COMMENT,
}


# ---------------------------------------------------------------------------
# Keyword set (§3.3, frozen for v1.0)
# ---------------------------------------------------------------------------


KEYWORDS: frozenset[str] = frozenset(
    """
    agent and arena as async await
    break budget cap const continue detach
    deinit derive dyn effect else enum
    extern fn for if impl import
    in init let loop match mod
    move mut on or panic
    protocol pub return run sandbox self
    Self spawn static struct supervisor task
    trait true false type union unsafe
    use where while with
    """.split()
)

# Two-token construct: ``proc macro`` is parsed as a bigram (§3.3 / §20.3).
# ``proc`` alone is not a keyword.

# Size suffixes (§3.4.4). Note ``K`` is reserved, only ``k`` is decimal.
# Longest-first ordering for greedy match.
_SIZE_SUFFIXES = ("KiB", "MiB", "GiB", "B", "k", "M")
# Duration suffixes (§3.4.5).
_DURATION_SUFFIXES = ("ns", "us", "ms", "s", "m", "h")

# Integer width suffixes (§3.4.1).
_INT_SUFFIX_WIDTHS = ("128", "size", "64", "32", "16", "8")


# ---------------------------------------------------------------------------
# Character classification helpers
# ---------------------------------------------------------------------------


def _is_ident_start(ch: str) -> bool:
    # XID_Start approximation: ASCII letter, underscore, or non-ASCII
    # whose Unicode category indicates a letter. The full XID_Start
    # tables ship with the unicodedata module via category lookups.
    if not ch:
        return False
    if ch == "_":
        return True
    if "A" <= ch <= "Z" or "a" <= ch <= "z":
        return True
    if ord(ch) < 0x80:
        return False
    import unicodedata
    cat = unicodedata.category(ch)
    # Lu Ll Lt Lm Lo Nl all qualify as identifier-start per XID_Start.
    return cat in ("Lu", "Ll", "Lt", "Lm", "Lo", "Nl")


def _is_ident_continue(ch: str) -> bool:
    if not ch:
        return False
    if ch == "_":
        return True
    if "A" <= ch <= "Z" or "a" <= ch <= "z" or "0" <= ch <= "9":
        return True
    if ord(ch) < 0x80:
        return False
    import unicodedata
    cat = unicodedata.category(ch)
    return cat in (
        "Lu", "Ll", "Lt", "Lm", "Lo", "Nl",
        "Mn", "Mc", "Nd", "Pc",
    )


def _is_dec_digit(ch: str) -> bool:
    return "0" <= ch <= "9"


def _is_hex_digit(ch: str) -> bool:
    return _is_dec_digit(ch) or ("a" <= ch <= "f") or ("A" <= ch <= "F")


def _is_oct_digit(ch: str) -> bool:
    return "0" <= ch <= "7"


def _is_bin_digit(ch: str) -> bool:
    return ch in ("0", "1")


# ---------------------------------------------------------------------------
# Lexer
# ---------------------------------------------------------------------------


class _Lexer:
    """One-shot lexer; do not reuse across calls."""

    def __init__(self, source: str) -> None:
        # §3.1: reject leading BOM, normalise CRLF -> LF.
        self.diags: list[Diagnostic] = []
        if source.startswith("﻿"):
            self.diags.append(
                Diagnostic(
                    code=CODE_LEX_BOM_REJECTED,
                    message="UTF-8 BOM at start of file is rejected (§3.1)",
                    start=0,
                    end=1,
                )
            )
            source = source[1:]
        # CRLF normalisation (§3.1) -- done before tokenisation so we
        # work in normalised offsets only. We DO NOT preserve original
        # CRLF offsets; the spec says "the lexer normalises CRLF to LF
        # before tokenisation" so spans are in normalised space.
        self.source = source.replace("\r\n", "\n")
        self.pos = 0
        self.n = len(self.source)
        self.tokens: list[Token] = []

    # ----- primitive cursor ops -----

    def _peek(self, off: int = 0) -> str:
        p = self.pos + off
        if 0 <= p < self.n:
            return self.source[p]
        return ""

    def _starts_with(self, s: str) -> bool:
        return self.source.startswith(s, self.pos)

    def _advance(self, count: int = 1) -> str:
        s = self.source[self.pos : self.pos + count]
        self.pos += count
        return s

    def _emit(self, kind: TokenKind, start: int, *, keyword: str = "") -> None:
        self.tokens.append(
            Token(kind, self.source[start : self.pos], start, self.pos, keyword)
        )

    # ----- top-level pump -----

    def run(self) -> tuple[list[Token], list[Diagnostic]]:
        while self.pos < self.n:
            self._step()
        self.tokens.append(Token(TokenKind.EOF, "", self.pos, self.pos))
        return self.tokens, self.diags

    def _step(self) -> None:
        ch = self._peek()

        # Whitespace (non-newline).
        if ch in (" ", "\t"):
            self._lex_horizontal_ws()
            return
        if ch == "\n":
            start = self.pos
            self._advance()
            self._emit(TokenKind.NEWLINE, start)
            return

        # Comments.
        if ch == "/" and self._peek(1) == "/":
            self._lex_line_comment()
            return
        if ch == "/" and self._peek(1) == "*":
            self._lex_block_comment()
            return

        # Raw / byte / html strings (prefixed forms).
        if ch == "r" and self._peek(1) in ('"', "#"):
            if self._try_lex_raw_string():
                return
        if ch == "b" and self._peek(1) == '"':
            self._lex_byte_string()
            return
        if (
            ch == "h"
            and self._peek(1) == "t"
            and self._peek(2) == "m"
            and self._peek(3) == "l"
            and self._peek(4) == '"'
        ):
            self._lex_html_literal()
            return

        # Strings and char literals.
        if ch == '"':
            self._lex_string()
            return
        if ch == "'":
            self._lex_char()
            return

        # Numeric.
        if _is_dec_digit(ch):
            self._lex_number()
            return

        # Identifier / keyword.
        if _is_ident_start(ch):
            self._lex_ident_or_keyword()
            return

        # Punctuation.
        if self._lex_punct():
            return

        # Unknown.
        start = self.pos
        bad = self._advance()
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_UNKNOWN_CHAR,
                message=f"unrecognised character {bad!r} (U+{ord(bad):04X})",
                start=start,
                end=self.pos,
            )
        )

    # ----- whitespace / comments -----

    def _lex_horizontal_ws(self) -> None:
        start = self.pos
        while self._peek() in (" ", "\t"):
            self._advance()
        self._emit(TokenKind.WHITESPACE, start)

    def _lex_line_comment(self) -> None:
        start = self.pos
        # ``///`` is a doc comment (§3.2). ``////`` is still a doc comment
        # in the spirit of Rust; the spec is silent on the boundary --
        # we treat any prefix of 3+ slashes as DOC_COMMENT.
        # ``//!`` (inner doc) is NOT mentioned in §3.2 so we leave it
        # as a regular line comment.
        is_doc = self._peek(2) == "/" and self._peek(3) != "/"
        # Consume ``//`` or ``///``.
        self._advance(3 if is_doc else 2)
        while self.pos < self.n and self._peek() != "\n":
            self._advance()
        self._emit(
            TokenKind.DOC_COMMENT if is_doc else TokenKind.LINE_COMMENT, start
        )

    def _lex_block_comment(self) -> None:
        # §3.2: block comments nest.
        start = self.pos
        self._advance(2)  # /*
        depth = 1
        while self.pos < self.n and depth > 0:
            if self._peek() == "/" and self._peek(1) == "*":
                self._advance(2)
                depth += 1
            elif self._peek() == "*" and self._peek(1) == "/":
                self._advance(2)
                depth -= 1
            else:
                self._advance()
        if depth != 0:
            self.diags.append(
                Diagnostic(
                    code=CODE_LEX_UNTERMINATED_BLOCK_COMMENT,
                    message=f"unterminated block comment (still {depth} levels open)",
                    start=start,
                    end=self.pos,
                )
            )
        self._emit(TokenKind.BLOCK_COMMENT, start)

    # ----- strings -----

    def _lex_string(self) -> None:
        start = self.pos
        self._advance()  # opening "
        while self.pos < self.n:
            ch = self._peek()
            if ch == "\\":
                # Consume an escape: skip the backslash + one (or more) chars.
                self._advance()
                if self.pos < self.n:
                    esc = self._peek()
                    self._advance()
                    if esc == "x":
                        # \xNN -- two hex digits expected; we tolerate.
                        for _ in range(2):
                            if _is_hex_digit(self._peek()):
                                self._advance()
                    elif esc == "u":
                        # \u{...}
                        if self._peek() == "{":
                            self._advance()
                            while self.pos < self.n and self._peek() != "}":
                                self._advance()
                            if self._peek() == "}":
                                self._advance()
                continue
            if ch == '"':
                self._advance()
                self._emit(TokenKind.STRING_LITERAL, start)
                return
            if ch == "\n":
                # Multi-line strings are accepted (the spec is silent;
                # Mighty examples include html"..." spanning lines).
                self._advance()
                continue
            self._advance()
        # Unterminated.
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_UNTERMINATED_STRING,
                message="unterminated string literal",
                start=start,
                end=self.pos,
            )
        )
        self._emit(TokenKind.STRING_LITERAL, start)

    def _try_lex_raw_string(self) -> bool:
        # r"..." or r#"..."#  with N >= 0 hashes.
        start = self.pos
        save = self.pos
        self._advance()  # r
        hashes = 0
        while self._peek() == "#":
            hashes += 1
            self._advance()
        if self._peek() != '"':
            # Not a raw string -- back out.
            self.pos = save
            return False
        self._advance()  # opening "
        terminator = '"' + ("#" * hashes)
        while self.pos < self.n:
            if self._starts_with(terminator):
                self._advance(len(terminator))
                self._emit(TokenKind.RAW_STRING_LITERAL, start)
                return True
            self._advance()
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_UNTERMINATED_STRING,
                message="unterminated raw string literal",
                start=start,
                end=self.pos,
            )
        )
        self._emit(TokenKind.RAW_STRING_LITERAL, start)
        return True

    def _lex_byte_string(self) -> None:
        start = self.pos
        self._advance()  # b
        # Reuse string lexer body, but emit as BYTE_STRING_LITERAL.
        # Open quote will be re-consumed by _lex_string; we have to
        # inline a copy here so the kind is right.
        self._advance()  # "
        while self.pos < self.n:
            ch = self._peek()
            if ch == "\\":
                self._advance()
                if self.pos < self.n:
                    self._advance()
                continue
            if ch == '"':
                self._advance()
                self._emit(TokenKind.BYTE_STRING_LITERAL, start)
                return
            self._advance()
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_UNTERMINATED_STRING,
                message="unterminated byte-string literal",
                start=start,
                end=self.pos,
            )
        )
        self._emit(TokenKind.BYTE_STRING_LITERAL, start)

    def _lex_html_literal(self) -> None:
        # ``html"..."`` -- spec §3.4.3 lists this as a tagged template
        # form. We lex it as a single HTML_LITERAL token; interpolation
        # placeholders (``{name}``) and the structured DOM lowering
        # described in §22.2 are the parser/HIR layer's job.
        start = self.pos
        self._advance(4)  # html
        self._advance()    # "
        depth_brace = 0
        while self.pos < self.n:
            ch = self._peek()
            if ch == "\\":
                self._advance()
                if self.pos < self.n:
                    self._advance()
                continue
            if ch == "{":
                depth_brace += 1
                self._advance()
                continue
            if ch == "}":
                if depth_brace > 0:
                    depth_brace -= 1
                self._advance()
                continue
            if ch == '"' and depth_brace == 0:
                self._advance()
                self._emit(TokenKind.HTML_LITERAL, start)
                return
            self._advance()
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_UNTERMINATED_STRING,
                message="unterminated html literal",
                start=start,
                end=self.pos,
            )
        )
        self._emit(TokenKind.HTML_LITERAL, start)

    def _lex_char(self) -> None:
        start = self.pos
        self._advance()  # opening '
        # Body: either one escaped sequence or a single non-quote char.
        if self._peek() == "\\":
            self._advance()
            if self.pos < self.n:
                self._advance()
                # Tolerate \xNN, \u{...} bodies.
                if self.source[self.pos - 1] == "x":
                    for _ in range(2):
                        if _is_hex_digit(self._peek()):
                            self._advance()
                elif self.source[self.pos - 1] == "u" and self._peek() == "{":
                    self._advance()
                    while self.pos < self.n and self._peek() != "}":
                        self._advance()
                    if self._peek() == "}":
                        self._advance()
        elif self.pos < self.n and self._peek() != "'":
            self._advance()

        if self._peek() == "'":
            self._advance()
            self._emit(TokenKind.CHAR_LITERAL, start)
            return
        # Lifetime label like ``'a`` would land here in some grammars;
        # Mighty v1.0 has no lifetime labels (per §7 / §11.2 -- labelled
        # break is deferred). So we report a char-literal error.
        self.diags.append(
            Diagnostic(
                code=CODE_LEX_BAD_CHAR_LITERAL,
                message="malformed character literal (missing closing quote)",
                start=start,
                end=self.pos,
            )
        )
        self._emit(TokenKind.CHAR_LITERAL, start)

    # ----- numbers -----

    def _lex_number(self) -> None:
        start = self.pos
        ch = self._peek()
        # Detect base.
        if ch == "0" and self._peek(1) in ("x", "X"):
            self._advance(2)
            self._consume_digits(_is_hex_digit)
            self._maybe_int_suffix()
            self._emit(TokenKind.INT_LITERAL, start)
            return
        if ch == "0" and self._peek(1) in ("o", "O"):
            self._advance(2)
            self._consume_digits(_is_oct_digit)
            self._maybe_int_suffix()
            self._emit(TokenKind.INT_LITERAL, start)
            return
        if ch == "0" and self._peek(1) in ("b", "B"):
            self._advance(2)
            self._consume_digits(_is_bin_digit)
            self._maybe_int_suffix()
            self._emit(TokenKind.INT_LITERAL, start)
            return

        # Decimal integer / float / size / duration.
        self._consume_digits(_is_dec_digit)

        is_float = False
        # Float: ``.`` followed by a digit -- ``1..hi`` is a range.
        if self._peek() == "." and _is_dec_digit(self._peek(1)):
            is_float = True
            self._advance()  # .
            self._consume_digits(_is_dec_digit)
        # Exponent ``e+N``/``E-N``.
        if self._peek() in ("e", "E"):
            # Tentative: must be followed by digit or sign+digit.
            save = self.pos
            self._advance()
            if self._peek() in ("+", "-"):
                self._advance()
            if _is_dec_digit(self._peek()):
                is_float = True
                self._consume_digits(_is_dec_digit)
            else:
                self.pos = save

        if is_float:
            # Optional f32/f64 suffix.
            if self._peek() == "f":
                save = self.pos
                self._advance()
                w = ""
                while _is_dec_digit(self._peek()):
                    w += self._advance()
                if w in ("32", "64"):
                    pass  # accepted
                else:
                    self.pos = save
            self._emit(TokenKind.FLOAT_LITERAL, start)
            return

        # Try size suffix (§3.4.4).
        for suf in _SIZE_SUFFIXES:
            if self._starts_with(suf) and not _is_ident_continue(self._peek(len(suf))):
                self._advance(len(suf))
                self._emit(TokenKind.SIZE_LITERAL, start)
                return

        # Try duration suffix (§3.4.5).
        for suf in _DURATION_SUFFIXES:
            if self._starts_with(suf) and not _is_ident_continue(self._peek(len(suf))):
                self._advance(len(suf))
                self._emit(TokenKind.DURATION_LITERAL, start)
                return

        # Plain integer with optional u/i suffix.
        self._maybe_int_suffix()
        self._emit(TokenKind.INT_LITERAL, start)

    def _consume_digits(self, pred) -> None:
        # Allow underscores between digits but never two in a row and
        # not at the very end.
        any_digit = False
        last_was_us = False
        while True:
            ch = self._peek()
            if pred(ch):
                self._advance()
                any_digit = True
                last_was_us = False
            elif ch == "_" and any_digit and not last_was_us:
                self._advance()
                last_was_us = True
            else:
                break
        if last_was_us:
            self.diags.append(
                Diagnostic(
                    code=CODE_LEX_BAD_NUMBER,
                    message="numeric literal ends with an underscore separator",
                    start=self.pos - 1,
                    end=self.pos,
                )
            )

    def _maybe_int_suffix(self) -> None:
        ch = self._peek()
        if ch not in ("u", "i"):
            return
        save = self.pos
        self._advance()
        for w in _INT_SUFFIX_WIDTHS:
            if self._starts_with(w) and not _is_ident_continue(self._peek(len(w))):
                self._advance(len(w))
                return
        # Suffix didn't match -- back out.
        self.pos = save

    # ----- ident / keyword -----

    def _lex_ident_or_keyword(self) -> None:
        start = self.pos
        self._advance()
        while _is_ident_continue(self._peek()):
            self._advance()
        text = self.source[start : self.pos]
        if text == "_":
            self._emit(TokenKind.UNDERSCORE, start)
            return
        if text in KEYWORDS:
            self._emit(TokenKind.KEYWORD, start, keyword=text)
            return
        self._emit(TokenKind.IDENT, start)

    # ----- punctuation -----

    def _lex_punct(self) -> bool:
        # Longest-first sequence of greedy matches.
        s = self.source
        p = self.pos

        def emit(kind: TokenKind, n: int) -> bool:
            start = self.pos
            self._advance(n)
            self._emit(kind, start)
            return True

        # 3-char tokens first.
        three = s[p : p + 3]
        if three == "...":
            return emit(TokenKind.DOT_DOT_DOT, 3)
        if three == "..=":
            return emit(TokenKind.DOT_DOT_EQ, 3)
        if three == "::[":
            # Turbofish opener (A2). We synthesise this trigraph so the
            # parser can distinguish ``Path::[T]`` from a plain ``::``
            # followed by ``[``. The spec §3.5 / §6.6 calls turbofish
            # the value-position form ``Path::[T1, T2]``.
            return emit(TokenKind.COLON_COLON_LBRACKET, 3)

        # 2-char tokens.
        two = s[p : p + 2]
        twomap = {
            "::": TokenKind.COLON_COLON,
            "..": TokenKind.DOT_DOT,
            "->": TokenKind.ARROW,
            "=>": TokenKind.FAT_ARROW,
            "==": TokenKind.EQ_EQ,
            "!=": TokenKind.NE,
            "<=": TokenKind.LE,
            ">=": TokenKind.GE,
            "<<": TokenKind.LSHIFT,
            ">>": TokenKind.RSHIFT,
            "&&": TokenKind.AMP_AMP,
            "||": TokenKind.PIPE_PIPE,
            "+=": TokenKind.PLUS_EQ,
            "-=": TokenKind.MINUS_EQ,
            "*=": TokenKind.STAR_EQ,
            "/=": TokenKind.SLASH_EQ,
            "%=": TokenKind.PERCENT_EQ,
            "&=": TokenKind.AMP_EQ,
            "|=": TokenKind.PIPE_EQ,
            "^=": TokenKind.CARET_EQ,
        }
        if two in twomap:
            return emit(twomap[two], 2)

        # 1-char tokens.
        onemap = {
            "(": TokenKind.LPAREN, ")": TokenKind.RPAREN,
            "{": TokenKind.LBRACE, "}": TokenKind.RBRACE,
            "[": TokenKind.LBRACKET, "]": TokenKind.RBRACKET,
            ",": TokenKind.COMMA, ";": TokenKind.SEMI,
            ":": TokenKind.COLON, ".": TokenKind.DOT,
            "@": TokenKind.AT, "#": TokenKind.HASH,
            "$": TokenKind.DOLLAR, "?": TokenKind.QUESTION,
            "!": TokenKind.BANG, "&": TokenKind.AMP,
            "|": TokenKind.PIPE, "^": TokenKind.CARET,
            "~": TokenKind.TILDE, "=": TokenKind.EQ,
            "<": TokenKind.LT, ">": TokenKind.GT,
            "+": TokenKind.PLUS, "-": TokenKind.MINUS,
            "*": TokenKind.STAR, "/": TokenKind.SLASH,
            "%": TokenKind.PERCENT,
        }
        if p < self.n and s[p] in onemap:
            return emit(onemap[s[p]], 1)

        return False


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def lex(source: str) -> tuple[list[Token], list[Diagnostic]]:
    """Tokenise a Mighty source string.

    Returns a ``(tokens, diagnostics)`` tuple. ``tokens`` always ends
    with an :attr:`TokenKind.EOF` token. ``diagnostics`` may be empty.
    """
    return _Lexer(source).run()


def strip_trivia(tokens: Iterable[Token]) -> list[Token]:
    """Return a copy of ``tokens`` with whitespace/comments removed.

    The EOF token is preserved.
    """
    return [t for t in tokens if not t.is_trivia]
