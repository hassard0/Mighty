"""Pure-Python Mighty recursive-descent parser.

Spec source: ``docs/spec/v1.0-rc.md`` §4 onwards.

We produce a JSON-friendly tree of plain ``dict`` nodes (every node has
a ``"_kind"`` discriminator and a ``"span"`` ``(start, end)`` byte
range). This was chosen over Python dataclasses for two reasons:

1. The tree is trivially serialisable for ``--cst``-style diff
   harnesses (just :func:`json.dumps`).
2. The shape is exactly the same set of names a downstream consumer
   reading the spec would expect; no Python-specific class hierarchy
   stands between the spec text and the tree.

The grammar covered in this 2nd-impl pass:

* Top-level items: ``fn``, ``struct``, ``enum``, ``type`` alias,
  ``use``/``import``, ``mod``, ``const``, ``static``, ``trait``,
  ``impl``, ``extern { ... }`` blocks, ``package`` decl.
* Block expressions and the common expression grammar (literals,
  paths, binary ops, calls, ``if``, ``match``, ``loop``, ``while``,
  ``for``, ``return``, ``break``, ``continue``, ``let``).
* Patterns: literal, ident, ``_``, tuple, struct, enum-variant
  destructure.
* Types: paths with optional ``[T, U]`` argument lists, references
  ``&T``/``&mut T``, raw pointers ``*T``/``*mut T``, tuples,
  arrays ``[T; N]``, function pointers ``fn(...) -> T``, the
  ``T!E`` sugar and ``T!{...}`` anonymous error unions (§6.3, §17.2).

Deferred (will return a coarse ``_kind="unparsed"`` token-bag node):
agents, protocols, supervisors, sandbox bodies, budget bodies, macros,
proc macros, ``async``/``spawn`` expression forms, ``detach``,
``unsafe`` blocks, ``derive`` keyword shorthand bodies, the
``@deadline`` postfix, ask/send sugar ``?Msg``/``!Msg``. These are
called out under "Deferred" in PYTHON_IMPL_V0_11_NOTES.md.

Operator precedence follows the conventional ladder; the spec defers
exact precedence to ``docs/internals/parser.md`` which is **not** in
the consultable spec set per the no-source-peek mandate. We document
the assumed ladder in PYTHON_IMPL_V0_11_NOTES.md as an interpretation
call (and a finding).
"""

from __future__ import annotations

from typing import Any, Optional

from .diagnostics import (
    CODE_PARSE_BAD_EXPR,
    CODE_PARSE_BAD_ITEM,
    CODE_PARSE_EXPECTED,
    CODE_PARSE_UNEXPECTED_EOF,
    Diagnostic,
    Severity,
)
from .lexer import KEYWORDS, Token, TokenKind, lex, strip_trivia


Node = dict[str, Any]


# ---------------------------------------------------------------------------
# Operator precedence (interpretation call, see notes).
# Higher number = binds tighter. Right-associative ops marked rassoc=True.
# ---------------------------------------------------------------------------

_BIN_OPS: dict[str, tuple[int, bool]] = {
    "||": (1, False),
    "or": (1, False),
    "&&": (2, False),
    "and": (2, False),
    "==": (3, False), "!=": (3, False),
    "<":  (4, False), "<=": (4, False),
    ">":  (4, False), ">=": (4, False),
    "|":  (5, False),
    "^":  (6, False),
    "&":  (7, False),
    "<<": (8, False), ">>": (8, False),
    "+":  (9, False), "-":  (9, False),
    "*":  (10, False), "/": (10, False), "%": (10, False),
    "..": (0, False),  # range, lowest precedence we still admit
    "..=": (0, False),
}

# Assignment is an expression in spec §11.1 (block expression yields a
# value) but assignments themselves are statements; we accept them only
# at statement level. The ``+=``/``-=`` family is treated likewise.
_ASSIGN_OPS = {"=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^="}


# ---------------------------------------------------------------------------
# Parser
# ---------------------------------------------------------------------------


class _ParseError(Exception):
    """Internal control-flow exception; tests use Parser.diagnostics."""


class Parser:
    """Stateful recursive-descent parser; one instance per source file."""

    def __init__(self, tokens: list[Token], source: str = "") -> None:
        # We work on the non-trivia token stream; trivia is preserved
        # on tokens, but the structural parse operates on terminals.
        self.tokens = [t for t in tokens if not t.is_trivia]
        self.source = source
        self.pos = 0
        self.diagnostics: list[Diagnostic] = []

    # ----- low-level cursor -----

    def _peek(self, off: int = 0) -> Token:
        idx = self.pos + off
        if idx >= len(self.tokens):
            return self.tokens[-1]  # EOF
        return self.tokens[idx]

    def _eat(self) -> Token:
        t = self._peek()
        if t.kind != TokenKind.EOF:
            self.pos += 1
        return t

    def _at(self, kind: TokenKind, *, kw: Optional[str] = None) -> bool:
        t = self._peek()
        if t.kind != kind:
            return False
        if kw is not None and t.keyword != kw:
            return False
        return True

    def _at_kw(self, kw: str) -> bool:
        return self._at(TokenKind.KEYWORD, kw=kw)

    def _eat_if(self, kind: TokenKind, *, kw: Optional[str] = None) -> Optional[Token]:
        if self._at(kind, kw=kw):
            return self._eat()
        return None

    def _expect(self, kind: TokenKind, *, kw: Optional[str] = None) -> Token:
        if self._at(kind, kw=kw):
            return self._eat()
        got = self._peek()
        msg = (
            f"expected {kind.name}{' ' + kw if kw else ''}, "
            f"got {got.kind.name} {got.text!r}"
        )
        self.diagnostics.append(
            Diagnostic(
                code=CODE_PARSE_EXPECTED,
                message=msg,
                start=got.start,
                end=got.end,
            )
        )
        raise _ParseError(msg)

    # ----- entry -----

    def parse_file(self) -> Node:
        start = self._peek().start
        items: list[Node] = []
        # Optional ``package name`` declaration at the top.
        if self._at(TokenKind.IDENT) and self._peek().text == "package":
            # The spec doesn't reserve ``package`` as a keyword (§3.3)
            # but examples/19_backend_service.mty uses ``package
            # search_api`` at file top; we accept it permissively.
            kw_tok = self._eat()
            name = self._eat()
            items.append({
                "_kind": "package_decl",
                "name": name.text,
                "span": (kw_tok.start, name.end),
            })
        while not self._at(TokenKind.EOF):
            try:
                item = self._parse_item()
                if item is not None:
                    items.append(item)
            except _ParseError:
                # Resync: skip to next plausible item start (a top-level
                # keyword we recognise, or the end of file).
                self._resync_to_item_start()
        end = self._peek().end
        return {
            "_kind": "file",
            "items": items,
            "span": (start, end),
            "diagnostics": [d.code for d in self.diagnostics],
        }

    def _resync_to_item_start(self) -> None:
        starters = {
            "fn", "struct", "enum", "type", "use", "import", "mod",
            "const", "static", "trait", "impl", "extern", "pub",
            "agent", "protocol", "supervisor", "cap", "sandbox",
            "macro", "proc", "derive", "package", "export",
        }
        while not self._at(TokenKind.EOF):
            t = self._peek()
            if t.kind == TokenKind.KEYWORD and t.keyword in starters:
                return
            if t.kind == TokenKind.IDENT and t.text in starters:
                return
            if t.kind == TokenKind.HASH:  # #[derive(...)] attribute
                return
            self._eat()

    # ----- items -----

    def _parse_item(self) -> Optional[Node]:
        # Attributes: ``#[...]`` -- we treat them as opaque attribute
        # nodes and attach them to the next item.
        attrs: list[Node] = []
        while self._at(TokenKind.HASH):
            attrs.append(self._parse_attribute())

        # Visibility modifier ``pub`` (§4).
        visibility = "private"
        if self._at_kw("pub"):
            vt = self._eat()
            visibility = "pub"
            # ``pub(crate)`` form (§4).
            if self._at(TokenKind.LPAREN):
                self._eat()
                if self._at(TokenKind.IDENT):
                    visibility = "pub(" + self._eat().text + ")"
                self._eat_if(TokenKind.RPAREN)

        # ``export`` keyword used in examples (e.g. 20_frontend, 14_extern_c
        # ``export c fn ...``). Not reserved by §3.3 but appears as
        # ident at item position; we accept permissively.
        if self._at(TokenKind.IDENT) and self._peek().text == "export":
            ekw = self._eat()
            # Optional ABI tag ``c``.
            abi = None
            if self._at(TokenKind.IDENT) and self._peek().text in ("c", "js"):
                abi = self._eat().text
            # The fn that follows.
            fn = self._parse_item()
            if fn is None:
                return None
            fn["export"] = True
            if abi:
                fn["export_abi"] = abi
            fn["visibility"] = visibility
            if attrs:
                fn["attrs"] = attrs
            return fn

        # Procedural-macro bigram ``proc macro``.
        if (
            self._at(TokenKind.IDENT)
            and self._peek().text == "proc"
            and self._peek(1).kind == TokenKind.KEYWORD
            and self._peek(1).text == "macro"  # ``macro`` not in our KEYWORDS, see below
        ):
            return self._parse_proc_macro(attrs, visibility)
        # ``macro`` is not in the v1.0 reserved set (§3.3) but it's used
        # like a keyword in §20.2. Lex emits IDENT for it.
        if self._at(TokenKind.IDENT) and self._peek().text == "macro":
            return self._parse_macro_decl(attrs, visibility)
        if self._at(TokenKind.IDENT) and self._peek().text == "proc" and self._peek(1).text == "macro":
            return self._parse_proc_macro(attrs, visibility)

        if self._at_kw("fn"):
            return self._parse_fn(attrs, visibility)
        if self._at_kw("unsafe") and self._peek(1).keyword == "fn":
            # ``pub unsafe fn ...`` (example 17).
            self._eat()  # unsafe
            fn = self._parse_fn(attrs, visibility)
            if fn is not None:
                fn["unsafe"] = True
            return fn
        if self._at_kw("struct"):
            return self._parse_struct(attrs, visibility)
        if self._at_kw("enum"):
            return self._parse_enum(attrs, visibility)
        if self._at_kw("type"):
            return self._parse_type_alias(attrs, visibility)
        if self._at_kw("use") or self._at_kw("import"):
            return self._parse_use(attrs, visibility)
        if self._at_kw("mod"):
            return self._parse_mod(attrs, visibility)
        if self._at_kw("const"):
            return self._parse_const(attrs, visibility)
        if self._at_kw("static"):
            return self._parse_static(attrs, visibility)
        if self._at_kw("trait"):
            return self._parse_trait(attrs, visibility)
        if self._at_kw("impl"):
            return self._parse_impl(attrs, visibility)
        if self._at_kw("extern"):
            return self._parse_extern(attrs, visibility)
        # Deferred-but-detected agent/protocol/supervisor/sandbox items.
        # We balance the surrounding braces and emit a coarse node so
        # the file still parses end-to-end.
        if self._at(TokenKind.KEYWORD) and self._peek().keyword in (
            "agent", "protocol", "supervisor", "sandbox", "cap",
        ):
            return self._parse_deferred_braced_item(attrs, visibility)

        # Unknown item -- diagnostic and resync.
        t = self._peek()
        self.diagnostics.append(
            Diagnostic(
                code=CODE_PARSE_BAD_ITEM,
                message=f"unexpected token at item position: {t.kind.name} {t.text!r}",
                start=t.start,
                end=t.end,
            )
        )
        self._eat()
        return None

    def _parse_attribute(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.HASH)
        # ``#[...]`` -- swallow balanced brackets.
        self._expect(TokenKind.LBRACKET)
        depth = 1
        body_tokens: list[Token] = []
        while depth > 0 and not self._at(TokenKind.EOF):
            t = self._peek()
            if t.kind == TokenKind.LBRACKET:
                depth += 1
            elif t.kind == TokenKind.RBRACKET:
                depth -= 1
                if depth == 0:
                    self._eat()
                    break
            body_tokens.append(t)
            self._eat()
        return {
            "_kind": "attr",
            "tokens": [t.text for t in body_tokens],
            "span": (start, self._peek(-1).end),
        }

    # ----- items: fn, struct, enum, etc. -----

    def _parse_fn(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="fn")
        name_tok = self._expect(TokenKind.IDENT)
        generics = self._maybe_generic_params()
        params = self._parse_fn_params()
        ret_type: Optional[Node] = None
        if self._eat_if(TokenKind.ARROW):
            ret_type = self._parse_type()
        # Optional ``effect ...`` clause (§9.2).
        effects: list[str] = []
        if self._at_kw("effect"):
            self._eat()
            effects.append(self._parse_effect_name())
            while self._eat_if(TokenKind.COMMA):
                effects.append(self._parse_effect_name())
        # Optional ``requires`` / ``where`` / ``with`` clauses.
        clauses: list[Node] = []
        while True:
            if self._at(TokenKind.IDENT) and self._peek().text == "requires":
                kw = self._eat()
                expr = self._parse_expr()
                clauses.append({"_kind": "requires", "expr": expr,
                                "span": (kw.start, self._peek(-1).end)})
                continue
            if self._at_kw("where"):
                # Generic where clause.
                self._eat()
                # Permissive: walk until ``{`` or ``;``.
                buf: list[str] = []
                while not (self._at(TokenKind.LBRACE) or self._at(TokenKind.SEMI)
                           or self._at(TokenKind.EOF)):
                    buf.append(self._eat().text)
                clauses.append({"_kind": "where", "raw": " ".join(buf)})
                continue
            break
        body: Optional[Node] = None
        if self._at(TokenKind.LBRACE):
            body = self._parse_block()
        elif self._eat_if(TokenKind.EQ):
            # ``fn name(...) = expr`` (example 14: ``export c fn _add(...) = a + b``).
            expr = self._parse_expr()
            body = {"_kind": "expr_body", "expr": expr}
        elif self._eat_if(TokenKind.SEMI):
            body = None  # signature-only (example 17 `pub unsafe fn _from_raw(...)`)
        return {
            "_kind": "fn",
            "visibility": vis,
            "attrs": attrs,
            "name": name_tok.text,
            "generics": generics,
            "params": params,
            "ret_type": ret_type,
            "effects": effects,
            "clauses": clauses,
            "body": body,
            "span": (start, self._peek(-1).end),
        }

    def _parse_effect_name(self) -> str:
        # §3.3 / §9.2: keyword tokens accepted as effect names.
        t = self._peek()
        if t.kind in (TokenKind.IDENT, TokenKind.KEYWORD):
            return self._eat().text
        return self._eat().text

    def _maybe_generic_params(self) -> list[Node]:
        if not self._eat_if(TokenKind.LBRACKET):
            return []
        out: list[Node] = []
        while not self._at(TokenKind.RBRACKET) and not self._at(TokenKind.EOF):
            name = self._expect(TokenKind.IDENT).text
            bounds: list[str] = []
            if self._eat_if(TokenKind.COLON):
                bounds.append(self._eat().text)
                while self._eat_if(TokenKind.PLUS):
                    bounds.append(self._eat().text)
            out.append({"_kind": "generic_param", "name": name, "bounds": bounds})
            if not self._eat_if(TokenKind.COMMA):
                break
        self._expect(TokenKind.RBRACKET)
        return out

    def _parse_fn_params(self) -> list[Node]:
        self._expect(TokenKind.LPAREN)
        out: list[Node] = []
        while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
            # Allow leading ``self`` / ``&self`` / ``&mut self`` /
            # ``mut self`` shorthand without explicit type.
            start = self._peek().start
            if self._at_kw("self") or (
                self._at(TokenKind.AMP)
                and (self._peek(1).keyword == "self"
                     or (self._peek(1).keyword == "mut" and self._peek(2).keyword == "self"))
            ):
                # Consume the receiver token bag.
                bits: list[str] = []
                while True:
                    t = self._peek()
                    bits.append(t.text)
                    self._eat()
                    if t.keyword == "self":
                        break
                # Optional ``: Self`` ascription is also valid.
                ty: Optional[Node] = None
                if self._eat_if(TokenKind.COLON):
                    ty = self._parse_type()
                out.append({
                    "_kind": "self_param",
                    "raw": " ".join(bits),
                    "ty": ty,
                    "span": (start, self._peek(-1).end),
                })
            else:
                # ``mut?`` IDENT (: TYPE)?  (TYPE optional for closures
                # and for the agent ctor-param shorthand `(net, clock)`).
                is_mut = bool(self._eat_if(TokenKind.KEYWORD, kw="mut"))
                name = self._expect(TokenKind.IDENT).text
                ty = None
                if self._eat_if(TokenKind.COLON):
                    ty = self._parse_type()
                out.append({
                    "_kind": "param", "mut": is_mut, "name": name, "ty": ty,
                    "span": (start, self._peek(-1).end),
                })
            if not self._eat_if(TokenKind.COMMA):
                break
        self._expect(TokenKind.RPAREN)
        return out

    def _parse_struct(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="struct")
        name = self._expect(TokenKind.IDENT).text
        generics = self._maybe_generic_params()
        # Three shapes: braced, tuple, unit (`;`).
        if self._eat_if(TokenKind.LBRACE):
            fields: list[Node] = []
            while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                fstart = self._peek().start
                fvis = "private"
                if self._at_kw("pub"):
                    self._eat()
                    fvis = "pub"
                fname = self._expect(TokenKind.IDENT).text
                self._expect(TokenKind.COLON)
                fty = self._parse_type()
                fields.append({"_kind": "field", "vis": fvis, "name": fname,
                               "ty": fty, "span": (fstart, self._peek(-1).end)})
                # Separator may be ``,`` or just newline-implicit; we
                # accept either.
                self._eat_if(TokenKind.COMMA)
            self._expect(TokenKind.RBRACE)
            return {"_kind": "struct", "shape": "record", "visibility": vis,
                    "attrs": attrs, "name": name, "generics": generics,
                    "fields": fields, "span": (start, self._peek(-1).end)}
        if self._eat_if(TokenKind.LPAREN):
            tys: list[Node] = []
            while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                tys.append(self._parse_type())
                if not self._eat_if(TokenKind.COMMA):
                    break
            self._expect(TokenKind.RPAREN)
            self._eat_if(TokenKind.SEMI)
            return {"_kind": "struct", "shape": "tuple", "visibility": vis,
                    "attrs": attrs, "name": name, "generics": generics,
                    "fields": tys, "span": (start, self._peek(-1).end)}
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "struct", "shape": "unit", "visibility": vis,
                "attrs": attrs, "name": name, "generics": generics,
                "fields": [], "span": (start, self._peek(-1).end)}

    def _parse_enum(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="enum")
        name = self._expect(TokenKind.IDENT).text
        generics = self._maybe_generic_params()
        self._expect(TokenKind.LBRACE)
        variants: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            vstart = self._peek().start
            vname = self._expect(TokenKind.IDENT).text
            if self._eat_if(TokenKind.LPAREN):
                payload = []
                while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                    payload.append(self._parse_type())
                    if not self._eat_if(TokenKind.COMMA):
                        break
                self._expect(TokenKind.RPAREN)
                variants.append({"_kind": "variant", "name": vname,
                                 "shape": "tuple", "payload": payload,
                                 "span": (vstart, self._peek(-1).end)})
            elif self._eat_if(TokenKind.LBRACE):
                fields: list[Node] = []
                while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                    fn_ = self._expect(TokenKind.IDENT).text
                    self._expect(TokenKind.COLON)
                    fty = self._parse_type()
                    fields.append({"_kind": "field", "name": fn_, "ty": fty})
                    self._eat_if(TokenKind.COMMA)
                self._expect(TokenKind.RBRACE)
                variants.append({"_kind": "variant", "name": vname,
                                 "shape": "record", "fields": fields,
                                 "span": (vstart, self._peek(-1).end)})
            else:
                variants.append({"_kind": "variant", "name": vname,
                                 "shape": "unit",
                                 "span": (vstart, self._peek(-1).end)})
            self._eat_if(TokenKind.COMMA)
        self._expect(TokenKind.RBRACE)
        return {"_kind": "enum", "visibility": vis, "attrs": attrs,
                "name": name, "generics": generics, "variants": variants,
                "span": (start, self._peek(-1).end)}

    def _parse_type_alias(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="type")
        name = self._expect(TokenKind.IDENT).text
        generics = self._maybe_generic_params()
        self._expect(TokenKind.EQ)
        ty = self._parse_type()
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "type_alias", "visibility": vis, "attrs": attrs,
                "name": name, "generics": generics, "ty": ty,
                "span": (start, self._peek(-1).end)}

    def _parse_use(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._eat()  # use or import
        # Path with ``.`` separators (§4).
        parts: list[str] = []
        parts.append(self._eat().text)
        while self._eat_if(TokenKind.DOT):
            parts.append(self._eat().text)
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "use", "visibility": vis, "attrs": attrs,
                "path": parts, "span": (start, self._peek(-1).end)}

    def _parse_mod(self, attrs: list[Node], vis: str) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="mod")
        name = self._expect(TokenKind.IDENT).text
        if self._eat_if(TokenKind.LBRACE):
            items: list[Node] = []
            while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                it = self._parse_item()
                if it:
                    items.append(it)
            self._expect(TokenKind.RBRACE)
            return {"_kind": "mod", "visibility": vis, "attrs": attrs,
                    "name": name, "items": items,
                    "span": (start, self._peek(-1).end)}
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "mod_ext", "visibility": vis, "attrs": attrs,
                "name": name, "span": (start, self._peek(-1).end)}

    def _parse_const(self, attrs, vis):
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="const")
        name = self._expect(TokenKind.IDENT).text
        self._expect(TokenKind.COLON)
        ty = self._parse_type()
        self._expect(TokenKind.EQ)
        expr = self._parse_expr()
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "const", "visibility": vis, "attrs": attrs,
                "name": name, "ty": ty, "value": expr,
                "span": (start, self._peek(-1).end)}

    def _parse_static(self, attrs, vis):
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="static")
        is_mut = bool(self._eat_if(TokenKind.KEYWORD, kw="mut"))
        name = self._expect(TokenKind.IDENT).text
        self._expect(TokenKind.COLON)
        ty = self._parse_type()
        self._expect(TokenKind.EQ)
        expr = self._parse_expr()
        self._eat_if(TokenKind.SEMI)
        return {"_kind": "static", "visibility": vis, "attrs": attrs,
                "mut": is_mut, "name": name, "ty": ty, "value": expr,
                "span": (start, self._peek(-1).end)}

    def _parse_trait(self, attrs, vis):
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="trait")
        name = self._expect(TokenKind.IDENT).text
        generics = self._maybe_generic_params()
        self._expect(TokenKind.LBRACE)
        items: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            it = self._parse_item()
            if it:
                items.append(it)
        self._expect(TokenKind.RBRACE)
        return {"_kind": "trait", "visibility": vis, "attrs": attrs,
                "name": name, "generics": generics, "items": items,
                "span": (start, self._peek(-1).end)}

    def _parse_impl(self, attrs, vis):
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="impl")
        generics = self._maybe_generic_params()
        first = self._parse_type()
        # ``impl Trait for Type`` form vs ``impl Type``.
        target = None
        trait = None
        if self._at_kw("for"):
            self._eat()
            target = self._parse_type()
            trait = first
        else:
            target = first
        self._expect(TokenKind.LBRACE)
        items: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            it = self._parse_item()
            if it:
                items.append(it)
        self._expect(TokenKind.RBRACE)
        return {"_kind": "impl", "visibility": vis, "attrs": attrs,
                "generics": generics, "trait": trait, "ty": target,
                "items": items, "span": (start, self._peek(-1).end)}

    def _parse_extern(self, attrs, vis):
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="extern")
        # Optional ABI tag: bare ident like ``c`` / ``js`` (examples 14, 15).
        abi = None
        if self._at(TokenKind.IDENT):
            abi = self._eat().text
        self._expect(TokenKind.LBRACE)
        items: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            it = self._parse_item()
            if it:
                items.append(it)
        self._expect(TokenKind.RBRACE)
        return {"_kind": "extern_block", "visibility": vis, "attrs": attrs,
                "abi": abi, "items": items,
                "span": (start, self._peek(-1).end)}

    def _parse_macro_decl(self, attrs, vis):
        start = self._peek().start
        self._eat()  # macro
        name = self._expect(TokenKind.IDENT).text
        # Param list (a, b)
        params: list[str] = []
        if self._eat_if(TokenKind.LPAREN):
            while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                if self._at(TokenKind.IDENT):
                    params.append(self._eat().text)
                if not self._eat_if(TokenKind.COMMA):
                    break
            self._expect(TokenKind.RPAREN)
        # Optional ``=>`` separator (example 16).
        self._eat_if(TokenKind.FAT_ARROW)
        body = self._parse_token_block()
        return {"_kind": "macro_decl", "visibility": vis, "attrs": attrs,
                "name": name, "params": params, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_proc_macro(self, attrs, vis):
        start = self._peek().start
        self._eat()  # proc
        self._eat()  # macro
        name = self._expect(TokenKind.IDENT).text
        params = self._parse_fn_params()
        ret = None
        if self._eat_if(TokenKind.ARROW):
            ret = self._parse_type()
        body = self._parse_token_block()
        return {"_kind": "proc_macro", "visibility": vis, "attrs": attrs,
                "name": name, "params": params, "ret_type": ret,
                "body": body, "span": (start, self._peek(-1).end)}

    def _parse_deferred_braced_item(self, attrs: list[Node], vis: str) -> Node:
        # Walk a braced item, balancing braces. Used for agents,
        # protocols, supervisors, sandboxes -- the "deferred" pile per
        # the swarm scope.
        kind_tok = self._eat()  # the leading keyword
        start = kind_tok.start
        # Header: tokens until the opening ``{``.
        header_tokens: list[str] = []
        while not self._at(TokenKind.LBRACE) and not self._at(TokenKind.EOF):
            header_tokens.append(self._eat().text)
        self._eat_if(TokenKind.LBRACE)
        depth = 1
        body_tokens: list[str] = []
        while depth > 0 and not self._at(TokenKind.EOF):
            t = self._peek()
            if t.kind == TokenKind.LBRACE:
                depth += 1
            elif t.kind == TokenKind.RBRACE:
                depth -= 1
                if depth == 0:
                    self._eat()
                    break
            body_tokens.append(t.text)
            self._eat()
        return {
            "_kind": "deferred_" + kind_tok.keyword,
            "visibility": vis,
            "attrs": attrs,
            "header_tokens": header_tokens,
            "body_tokens": body_tokens,
            "span": (start, self._peek(-1).end),
        }

    def _parse_token_block(self) -> list[str]:
        # Brace-balanced token slurp. Used for macros, where we don't
        # parse the body as Mighty (the body is a token-substitution
        # template).
        self._expect(TokenKind.LBRACE)
        depth = 1
        out: list[str] = []
        while depth > 0 and not self._at(TokenKind.EOF):
            t = self._peek()
            if t.kind == TokenKind.LBRACE:
                depth += 1
            elif t.kind == TokenKind.RBRACE:
                depth -= 1
                if depth == 0:
                    self._eat()
                    break
            out.append(t.text)
            self._eat()
        return out

    # ----- types -----

    def _parse_type(self) -> Node:
        # Result-sugar postfix: parse a primary and then check for ``!``.
        ty = self._parse_type_primary()
        # ``T!E`` sugar (§6.3) -- the ``!`` is a token here. The spec
        # says: ``T!E`` is sugar for ``Result[T, E]``; ``T!{A, B}`` is
        # the anonymous-union form. We accept both.
        while self._at(TokenKind.BANG):
            self._eat()
            if self._at(TokenKind.LBRACE):
                # T!{A, B, C}
                self._eat()
                errs: list[Node] = []
                while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                    errs.append(self._parse_type())
                    if not self._eat_if(TokenKind.COMMA):
                        break
                self._expect(TokenKind.RBRACE)
                ty = {"_kind": "ty_result_union", "ok": ty, "errs": errs}
            else:
                err = self._parse_type_primary()
                ty = {"_kind": "ty_result", "ok": ty, "err": err}
        return ty

    def _parse_type_primary(self) -> Node:
        t = self._peek()
        start = t.start
        # Reference types.
        if self._eat_if(TokenKind.AMP):
            is_mut = bool(self._eat_if(TokenKind.KEYWORD, kw="mut"))
            inner = self._parse_type_primary()
            return {"_kind": "ty_ref", "mut": is_mut, "inner": inner,
                    "span": (start, self._peek(-1).end)}
        # Raw pointer types.
        if self._eat_if(TokenKind.STAR):
            is_mut = bool(self._eat_if(TokenKind.KEYWORD, kw="mut"))
            # ``*const T`` form: the v1.0 spec uses ``*T``/``*mut T``
            # only (§6.2); we tolerate ``*const T`` from example 14.
            if self._at(TokenKind.KEYWORD) and self._peek().keyword == "const":
                self._eat()
            inner = self._parse_type_primary()
            return {"_kind": "ty_ptr", "mut": is_mut, "inner": inner,
                    "span": (start, self._peek(-1).end)}
        # Tuple / unit / paren.
        if self._eat_if(TokenKind.LPAREN):
            elems: list[Node] = []
            if not self._at(TokenKind.RPAREN):
                elems.append(self._parse_type())
                while self._eat_if(TokenKind.COMMA):
                    if self._at(TokenKind.RPAREN):
                        break
                    elems.append(self._parse_type())
            self._expect(TokenKind.RPAREN)
            if len(elems) == 1:
                return elems[0]
            return {"_kind": "ty_tuple", "elems": elems,
                    "span": (start, self._peek(-1).end)}
        # Array ``[T; N]`` or slice ``[T]``.
        if self._eat_if(TokenKind.LBRACKET):
            inner = self._parse_type()
            if self._eat_if(TokenKind.SEMI):
                size_expr = self._parse_expr()
                self._expect(TokenKind.RBRACKET)
                return {"_kind": "ty_array", "elem": inner, "size": size_expr,
                        "span": (start, self._peek(-1).end)}
            self._expect(TokenKind.RBRACKET)
            return {"_kind": "ty_slice", "elem": inner,
                    "span": (start, self._peek(-1).end)}
        # ``fn(...) -> T`` function pointer.
        if self._at_kw("fn"):
            self._eat()
            self._expect(TokenKind.LPAREN)
            params: list[Node] = []
            while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                params.append(self._parse_type())
                if not self._eat_if(TokenKind.COMMA):
                    break
            self._expect(TokenKind.RPAREN)
            ret = None
            if self._eat_if(TokenKind.ARROW):
                ret = self._parse_type()
            return {"_kind": "ty_fn", "params": params, "ret": ret,
                    "span": (start, self._peek(-1).end)}
        # ``dyn Trait`` (§19.4).
        if self._at_kw("dyn"):
            self._eat()
            inner = self._parse_type_primary()
            return {"_kind": "ty_dyn", "inner": inner,
                    "span": (start, self._peek(-1).end)}
        # Path: IDENT (.IDENT)* with optional [TypeArgs].
        # Special: bare ``Self`` keyword as a type.
        if self._at_kw("Self") or self._at_kw("self"):
            kw = self._eat().text
            return {"_kind": "ty_path", "segments": [kw], "args": [],
                    "span": (start, self._peek(-1).end)}
        if self._at(TokenKind.IDENT):
            segs = [self._eat().text]
            while self._eat_if(TokenKind.DOT):
                segs.append(self._eat().text)
            args: list[Node] = []
            if self._eat_if(TokenKind.LBRACKET):
                while not self._at(TokenKind.RBRACKET) and not self._at(TokenKind.EOF):
                    args.append(self._parse_type())
                    if not self._eat_if(TokenKind.COMMA):
                        break
                self._expect(TokenKind.RBRACKET)
            return {"_kind": "ty_path", "segments": segs, "args": args,
                    "span": (start, self._peek(-1).end)}
        # Fallback: unrecognised -- diagnostic + skip.
        self.diagnostics.append(
            Diagnostic(code=CODE_PARSE_BAD_EXPR,
                       message=f"expected a type, got {t.kind.name} {t.text!r}",
                       start=t.start, end=t.end))
        self._eat()
        return {"_kind": "ty_error", "span": (start, self._peek(-1).end)}

    # ----- blocks & statements -----

    def _parse_block(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.LBRACE)
        stmts: list[Node] = []
        tail: Optional[Node] = None
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            # ``let`` is a statement.
            if self._at_kw("let"):
                stmts.append(self._parse_let())
                self._eat_if(TokenKind.SEMI)
                continue
            # Top-level fn-item inside a block (example 20 has nested
            # `fn draw()` inside agent body which we already deferred,
            # but some local fns may appear too).
            if self._at_kw("fn"):
                stmts.append(self._parse_fn([], "private"))
                continue
            # Statement vs tail expression: parse an expression then
            # check for ``;`` or block-end.
            expr = self._parse_expr()
            # Assignment statement?
            if self._at(TokenKind.EQ) or (
                self._peek().kind in (TokenKind.PLUS_EQ, TokenKind.MINUS_EQ,
                                       TokenKind.STAR_EQ, TokenKind.SLASH_EQ,
                                       TokenKind.PERCENT_EQ, TokenKind.AMP_EQ,
                                       TokenKind.PIPE_EQ, TokenKind.CARET_EQ)
            ):
                op_tok = self._eat()
                rhs = self._parse_expr()
                stmts.append({"_kind": "assign", "op": op_tok.text,
                              "target": expr, "value": rhs})
                self._eat_if(TokenKind.SEMI)
                continue
            if self._eat_if(TokenKind.SEMI):
                stmts.append({"_kind": "expr_stmt", "expr": expr})
                continue
            # No trailing ``;`` -- this is either the tail expression
            # or there are more statements separated by whitespace.
            if self._at(TokenKind.RBRACE):
                tail = expr
                break
            # Treat as statement and continue (looser than spec).
            stmts.append({"_kind": "expr_stmt", "expr": expr})
        self._expect(TokenKind.RBRACE)
        return {"_kind": "block", "stmts": stmts, "tail": tail,
                "span": (start, self._peek(-1).end)}

    def _parse_let(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="let")
        pat = self._parse_pattern()
        ty: Optional[Node] = None
        if self._eat_if(TokenKind.COLON):
            ty = self._parse_type()
        value: Optional[Node] = None
        if self._eat_if(TokenKind.EQ):
            value = self._parse_expr()
        return {"_kind": "let", "pat": pat, "ty": ty, "value": value,
                "span": (start, self._peek(-1).end)}

    # ----- patterns -----

    def _parse_pattern(self) -> Node:
        t = self._peek()
        start = t.start
        if self._eat_if(TokenKind.UNDERSCORE):
            return {"_kind": "pat_wild", "span": (start, self._peek(-1).end)}
        # Literal patterns (incl. range): handle simple int/str.
        if t.kind in (TokenKind.INT_LITERAL, TokenKind.FLOAT_LITERAL,
                      TokenKind.STRING_LITERAL):
            lit = self._eat()
            # Range pattern: 1..10
            if self._at(TokenKind.DOT_DOT) or self._at(TokenKind.DOT_DOT_EQ):
                op = self._eat().text
                hi = self._eat()
                return {"_kind": "pat_range", "lo": lit.text, "op": op,
                        "hi": hi.text, "span": (start, self._peek(-1).end)}
            return {"_kind": "pat_literal", "text": lit.text,
                    "span": (start, self._peek(-1).end)}
        if self._at_kw("true") or self._at_kw("false"):
            v = self._eat().text
            return {"_kind": "pat_literal", "text": v,
                    "span": (start, self._peek(-1).end)}
        # Tuple pattern.
        if self._eat_if(TokenKind.LPAREN):
            elems: list[Node] = []
            while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                elems.append(self._parse_pattern())
                if not self._eat_if(TokenKind.COMMA):
                    break
            self._expect(TokenKind.RPAREN)
            return {"_kind": "pat_tuple", "elems": elems,
                    "span": (start, self._peek(-1).end)}
        # Reference / ref pattern (``&p``).
        if self._eat_if(TokenKind.AMP):
            is_mut = bool(self._eat_if(TokenKind.KEYWORD, kw="mut"))
            inner = self._parse_pattern()
            return {"_kind": "pat_ref", "mut": is_mut, "inner": inner,
                    "span": (start, self._peek(-1).end)}
        # Path-based pattern: IDENT (.IDENT)* (tuple-payload | struct-fields)?
        if self._at(TokenKind.IDENT):
            segs = [self._eat().text]
            while self._eat_if(TokenKind.DOT):
                segs.append(self._eat().text)
            if self._eat_if(TokenKind.LPAREN):
                elems: list[Node] = []
                while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
                    elems.append(self._parse_pattern())
                    if not self._eat_if(TokenKind.COMMA):
                        break
                self._expect(TokenKind.RPAREN)
                return {"_kind": "pat_tuple_struct", "path": segs,
                        "elems": elems, "span": (start, self._peek(-1).end)}
            if self._eat_if(TokenKind.LBRACE):
                fields: list[Node] = []
                while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                    fname = self._expect(TokenKind.IDENT).text
                    fpat = None
                    if self._eat_if(TokenKind.COLON):
                        fpat = self._parse_pattern()
                    fields.append({"_kind": "pat_field", "name": fname,
                                   "pat": fpat})
                    self._eat_if(TokenKind.COMMA)
                self._expect(TokenKind.RBRACE)
                return {"_kind": "pat_struct", "path": segs, "fields": fields,
                        "span": (start, self._peek(-1).end)}
            if len(segs) == 1:
                return {"_kind": "pat_ident", "name": segs[0],
                        "span": (start, self._peek(-1).end)}
            return {"_kind": "pat_path", "segments": segs,
                    "span": (start, self._peek(-1).end)}
        # Fallback.
        self.diagnostics.append(
            Diagnostic(code=CODE_PARSE_EXPECTED,
                       message=f"expected pattern, got {t.kind.name}",
                       start=t.start, end=t.end))
        self._eat()
        return {"_kind": "pat_error", "span": (start, self._peek(-1).end)}

    # ----- expressions (Pratt) -----

    def _parse_expr(self) -> Node:
        return self._parse_expr_bp(0)

    def _parse_expr_bp(self, min_bp: int) -> Node:
        lhs = self._parse_unary()
        # Handle ``..`` / ``..=`` range as binary at low precedence.
        while True:
            # Postfix forms first (high binding).
            lhs = self._parse_postfix(lhs)
            tok = self._peek()
            op_text = tok.text
            # Keyword binary ops (``and``, ``or``) appear as KEYWORDs.
            if tok.kind == TokenKind.KEYWORD and tok.text in ("and", "or"):
                op_text = tok.text
            if op_text not in _BIN_OPS:
                break
            prec, rassoc = _BIN_OPS[op_text]
            if prec < min_bp:
                break
            op_tok = self._eat()
            next_bp = prec if rassoc else prec + 1
            rhs = self._parse_expr_bp(next_bp)
            lhs = {"_kind": "binop", "op": op_tok.text, "lhs": lhs, "rhs": rhs}
        return lhs

    def _parse_unary(self) -> Node:
        t = self._peek()
        start = t.start
        # Unary prefix operators.
        if t.kind in (TokenKind.MINUS, TokenKind.BANG, TokenKind.STAR,
                      TokenKind.AMP):
            op = self._eat()
            is_mut = False
            if op.kind == TokenKind.AMP and self._eat_if(TokenKind.KEYWORD, kw="mut"):
                is_mut = True
            inner = self._parse_unary()
            return {"_kind": "unary", "op": op.text, "mut": is_mut,
                    "operand": inner, "span": (start, self._peek(-1).end)}
        # Some keyword-led expression forms.
        if self._at_kw("if"):
            return self._parse_if()
        if self._at_kw("match"):
            return self._parse_match()
        if self._at_kw("loop"):
            return self._parse_loop()
        if self._at_kw("while"):
            return self._parse_while()
        if self._at_kw("for"):
            return self._parse_for()
        # Block-flavoured expression forms (§5.1 permissive scopes;
        # §10.1 arena / §16.1 sandbox / §16.2 budget / §21 unsafe).
        if self._at_kw("arena"):
            return self._parse_arena()
        if self._at_kw("unsafe"):
            return self._parse_unsafe()
        if self._at_kw("budget"):
            return self._parse_budget()
        if self._at_kw("sandbox"):
            return self._parse_sandbox()
        if self._at_kw("task"):
            return self._parse_task_scope()
        if self._at_kw("detach"):
            return self._parse_detach()
        if self._at_kw("return"):
            kw = self._eat()
            value = None
            if not (self._at(TokenKind.RBRACE) or self._at(TokenKind.SEMI)
                    or self._at(TokenKind.EOF)):
                value = self._parse_expr()
            return {"_kind": "return", "value": value,
                    "span": (kw.start, self._peek(-1).end)}
        if self._at_kw("break"):
            kw = self._eat()
            value = None
            if not (self._at(TokenKind.RBRACE) or self._at(TokenKind.SEMI)
                    or self._at(TokenKind.EOF)):
                value = self._parse_expr()
            return {"_kind": "break", "value": value,
                    "span": (kw.start, self._peek(-1).end)}
        if self._at_kw("continue"):
            kw = self._eat()
            return {"_kind": "continue",
                    "span": (kw.start, self._peek(-1).end)}
        if self._at_kw("panic"):
            kw = self._eat()
            # Treated like a normal call below; rewind by re-emitting
            # as a path-call primary.
            # The simplest path: fall back to atom which treats panic
            # as a callable IDENT-like name. But ``panic`` is KEYWORD;
            # build a synthetic path node + call if next is ``(``.
            path = {"_kind": "path", "segments": ["panic"],
                    "span": (kw.start, kw.end)}
            return self._parse_postfix(path)
        return self._parse_atom()

    def _parse_postfix(self, lhs: Node) -> Node:
        while True:
            # Call ``e(args)`` or ``e!(args)`` macro invocation.
            if self._at(TokenKind.LPAREN):
                args = self._parse_call_args()
                lhs = {"_kind": "call", "callee": lhs, "args": args}
                continue
            if self._at(TokenKind.BANG) and self._peek(1).kind == TokenKind.LPAREN:
                # Macro marker ``name!(args)`` (A90).
                self._eat()  # !
                args = self._parse_call_args()
                lhs = {"_kind": "macro_call", "callee": lhs, "args": args}
                continue
            # Index ``e[i]``.
            if self._at(TokenKind.LBRACKET):
                self._eat()
                idx = self._parse_expr()
                self._expect(TokenKind.RBRACKET)
                lhs = {"_kind": "index", "target": lhs, "index": idx}
                continue
            # Field ``e.f`` or method call ``e.f(args)``.
            if self._at(TokenKind.DOT):
                self._eat()
                # Per A3, keyword tokens after ``.`` are accepted as
                # method/field names.
                name = self._eat().text
                if self._at(TokenKind.LPAREN):
                    args = self._parse_call_args()
                    lhs = {"_kind": "method_call", "target": lhs,
                           "name": name, "args": args}
                else:
                    lhs = {"_kind": "field", "target": lhs, "name": name}
                continue
            # Turbofish ``e::[T,...]`` (A2).
            if self._at(TokenKind.COLON_COLON_LBRACKET):
                self._eat()
                args: list[Node] = []
                while not self._at(TokenKind.RBRACKET) and not self._at(TokenKind.EOF):
                    args.append(self._parse_type())
                    if not self._eat_if(TokenKind.COMMA):
                        break
                self._expect(TokenKind.RBRACKET)
                lhs = {"_kind": "turbofish", "target": lhs, "args": args}
                continue
            # Path segment ``::ident``.
            if self._at(TokenKind.COLON_COLON):
                self._eat()
                name = self._eat().text
                lhs = {"_kind": "path_seg", "target": lhs, "name": name}
                continue
            # Postfix ``?`` propagate (A7, A12).
            if self._at(TokenKind.QUESTION):
                # ``expr?Msg(args)`` is the ask sugar (A12) -- on the
                # same source line. We honour the same-line rule by
                # checking whether the next non-trivia token starts at
                # a smaller line than the ``?`` itself. We approximate
                # with a check: if the very next token (already
                # non-trivia in our stream) is an IDENT immediately
                # adjacent (no intervening newline), it's ask sugar.
                q = self._eat()
                nxt = self._peek()
                if (nxt.kind == TokenKind.IDENT
                        and self._tokens_same_line(q.end, nxt.start)
                        and self._peek(1).kind == TokenKind.LPAREN):
                    name = self._eat().text
                    args = self._parse_call_args()
                    lhs = {"_kind": "ask", "target": lhs, "msg": name,
                           "args": args}
                else:
                    lhs = {"_kind": "propagate", "operand": lhs}
                continue
            # Postfix ``!Msg(args)`` send sugar (A12).
            if self._at(TokenKind.BANG):
                b = self._peek()
                if (self._peek(1).kind == TokenKind.IDENT
                        and self._tokens_same_line(b.end, self._peek(1).start)
                        and self._peek(2).kind == TokenKind.LPAREN):
                    self._eat()  # !
                    name = self._eat().text
                    args = self._parse_call_args()
                    lhs = {"_kind": "send", "target": lhs, "msg": name,
                           "args": args}
                    continue
                # Not a send sugar (e.g. plain ``!=``) -- bail out.
                break
            # Deadline ``@duration`` (§12, §15.1).
            if self._at(TokenKind.AT):
                self._eat()
                # The deadline may be a duration literal or any expr.
                dl = self._parse_unary()
                lhs = {"_kind": "deadline", "target": lhs, "deadline": dl}
                continue
            break
        return lhs

    def _tokens_same_line(self, end_a: int, start_b: int) -> bool:
        # Source span between two token boundaries -- if it contains
        # a newline, they're on different lines.
        if not self.source:
            return True
        return "\n" not in self.source[end_a:start_b]

    def _parse_call_args(self) -> list[Node]:
        self._expect(TokenKind.LPAREN)
        out: list[Node] = []
        while not self._at(TokenKind.RPAREN) and not self._at(TokenKind.EOF):
            out.append(self._parse_expr())
            if not self._eat_if(TokenKind.COMMA):
                break
        self._expect(TokenKind.RPAREN)
        return out

    def _parse_atom(self) -> Node:
        t = self._peek()
        start = t.start
        if t.kind in (TokenKind.INT_LITERAL, TokenKind.FLOAT_LITERAL,
                      TokenKind.STRING_LITERAL, TokenKind.RAW_STRING_LITERAL,
                      TokenKind.BYTE_STRING_LITERAL, TokenKind.CHAR_LITERAL,
                      TokenKind.HTML_LITERAL,
                      TokenKind.SIZE_LITERAL, TokenKind.DURATION_LITERAL):
            self._eat()
            return {"_kind": "literal", "kind": t.kind.name, "text": t.text,
                    "span": (start, t.end)}
        if self._at_kw("true") or self._at_kw("false"):
            self._eat()
            return {"_kind": "literal", "kind": "BOOL", "text": t.text,
                    "span": (start, t.end)}
        if self._at_kw("self") or self._at_kw("Self"):
            kw = self._eat()
            return {"_kind": "path", "segments": [kw.text],
                    "span": (start, kw.end)}
        # Block expression.
        if self._at(TokenKind.LBRACE):
            return self._parse_block()
        # Paren / tuple / unit.
        if self._eat_if(TokenKind.LPAREN):
            if self._eat_if(TokenKind.RPAREN):
                return {"_kind": "tuple", "elems": [],
                        "span": (start, self._peek(-1).end)}
            elems = [self._parse_expr()]
            saw_comma = False
            while self._eat_if(TokenKind.COMMA):
                saw_comma = True
                if self._at(TokenKind.RPAREN):
                    break
                elems.append(self._parse_expr())
            self._expect(TokenKind.RPAREN)
            if not saw_comma and len(elems) == 1:
                return elems[0]
            return {"_kind": "tuple", "elems": elems,
                    "span": (start, self._peek(-1).end)}
        # Array literal ``[a, b, c]`` or ``[v; n]``.
        if self._eat_if(TokenKind.LBRACKET):
            if self._eat_if(TokenKind.RBRACKET):
                return {"_kind": "array", "elems": [],
                        "span": (start, self._peek(-1).end)}
            first = self._parse_expr()
            if self._eat_if(TokenKind.SEMI):
                count = self._parse_expr()
                self._expect(TokenKind.RBRACKET)
                return {"_kind": "array_repeat", "value": first, "count": count,
                        "span": (start, self._peek(-1).end)}
            elems = [first]
            while self._eat_if(TokenKind.COMMA):
                if self._at(TokenKind.RBRACKET):
                    break
                elems.append(self._parse_expr())
            self._expect(TokenKind.RBRACKET)
            return {"_kind": "array", "elems": elems,
                    "span": (start, self._peek(-1).end)}
        # Closure ``fn(...) { ... }`` (used in example 20).
        if self._at_kw("fn") and self._peek(1).kind == TokenKind.LPAREN:
            self._eat()  # fn
            params = self._parse_fn_params()
            ret = None
            if self._eat_if(TokenKind.ARROW):
                ret = self._parse_type()
            body = self._parse_block() if self._at(TokenKind.LBRACE) else None
            return {"_kind": "closure", "params": params, "ret": ret,
                    "body": body, "span": (start, self._peek(-1).end)}
        # Spawn expression: ``spawn Path(args)``.
        if self._at_kw("spawn"):
            self._eat()
            inner = self._parse_unary()
            return {"_kind": "spawn", "value": inner,
                    "span": (start, self._peek(-1).end)}
        # ``run`` keyword expression form (A5).
        if self._at_kw("run"):
            self._eat()
            inner = self._parse_expr()
            return {"_kind": "run", "value": inner,
                    "span": (start, self._peek(-1).end)}
        # Path / ident.
        if self._at(TokenKind.IDENT):
            segs = [self._eat().text]
            # Struct-literal: ``Path { f: v, ... }`` -- we have to be
            # careful not to swallow a control-flow `{` in expression
            # context; v1.0 spec doesn't restrict struct-literal in
            # condition position the way Rust does, but the conventional
            # interpretation is: in ``if cond { ... }`` the ``{`` is
            # the if-body. We disambiguate by NOT treating ``Path { ...``
            # as a struct literal at the top level of unary parses;
            # it's instead parsed via the explicit constructor call form
            # ``Path::[T]{}`` (turbofish + braces) or just elide. For
            # the examples we cover (01-05), struct literals appear in
            # value-position via ``Page {}`` and similar simple shapes;
            # we lookahead for a brace followed by an ident-colon
            # sequence or ``}``.
            if self._at(TokenKind.LBRACE) and self._looks_like_struct_literal():
                self._eat()  # {
                fields: list[Node] = []
                while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
                    fname = self._expect(TokenKind.IDENT).text
                    fval = None
                    if self._eat_if(TokenKind.COLON):
                        fval = self._parse_expr()
                    fields.append({"_kind": "lit_field", "name": fname,
                                   "value": fval})
                    self._eat_if(TokenKind.COMMA)
                self._expect(TokenKind.RBRACE)
                return {"_kind": "struct_literal", "path": segs,
                        "fields": fields, "span": (start, self._peek(-1).end)}
            return {"_kind": "path", "segments": segs,
                    "span": (start, self._peek(-1).end)}
        # Unknown -- diagnose and return error node.
        self.diagnostics.append(
            Diagnostic(code=CODE_PARSE_BAD_EXPR,
                       message=f"unexpected token {t.kind.name} {t.text!r} in expression",
                       start=t.start, end=t.end))
        self._eat()
        return {"_kind": "expr_error", "span": (start, self._peek(-1).end)}

    def _looks_like_struct_literal(self) -> bool:
        # Lookahead: ``{`` then either ``}`` (empty) or IDENT followed
        # by ``:`` or ``,`` or ``}``.
        if not self._at(TokenKind.LBRACE):
            return False
        a = self._peek(1)
        if a.kind == TokenKind.RBRACE:
            return True
        if a.kind != TokenKind.IDENT:
            return False
        b = self._peek(2)
        return b.kind in (TokenKind.COLON, TokenKind.COMMA, TokenKind.RBRACE)

    # ----- control-flow expressions -----

    def _parse_if(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="if")
        # ``if let Pat = expr { ... }`` (§11.1, A6).
        if self._at_kw("let"):
            self._eat()
            pat = self._parse_pattern()
            self._expect(TokenKind.EQ)
            scrut = self._parse_expr()
            body = self._parse_block()
            else_ = None
            if self._eat_if(TokenKind.KEYWORD, kw="else"):
                else_ = self._parse_block() if self._at(TokenKind.LBRACE) else self._parse_if()
            return {"_kind": "if_let", "pat": pat, "scrutinee": scrut,
                    "then": body, "else": else_,
                    "span": (start, self._peek(-1).end)}
        cond = self._parse_expr()
        body = self._parse_block()
        else_ = None
        if self._eat_if(TokenKind.KEYWORD, kw="else"):
            else_ = self._parse_block() if self._at(TokenKind.LBRACE) else self._parse_if()
        return {"_kind": "if", "cond": cond, "then": body, "else": else_,
                "span": (start, self._peek(-1).end)}

    def _parse_match(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="match")
        scrut = self._parse_expr()
        self._expect(TokenKind.LBRACE)
        arms: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            astart = self._peek().start
            pat = self._parse_pattern()
            guard = None
            if self._at_kw("if"):
                self._eat()
                guard = self._parse_expr()
            self._expect(TokenKind.FAT_ARROW)
            body = self._parse_expr()
            arms.append({"_kind": "arm", "pat": pat, "guard": guard,
                         "body": body, "span": (astart, self._peek(-1).end)})
            self._eat_if(TokenKind.COMMA)
        self._expect(TokenKind.RBRACE)
        return {"_kind": "match", "scrutinee": scrut, "arms": arms,
                "span": (start, self._peek(-1).end)}

    def _parse_loop(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="loop")
        body = self._parse_block()
        return {"_kind": "loop", "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_while(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="while")
        cond = self._parse_expr()
        body = self._parse_block()
        return {"_kind": "while", "cond": cond, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_for(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="for")
        pat = self._parse_pattern()
        self._expect(TokenKind.KEYWORD, kw="in")
        iter_ = self._parse_expr()
        body = self._parse_block()
        return {"_kind": "for", "pat": pat, "iter": iter_, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_arena(self) -> Node:
        # ``arena { ... }`` or ``arena LABEL { ... }`` or
        # ``arena LABEL: <expr>`` (the inline form in example 12).
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="arena")
        label: Optional[str] = None
        if self._at(TokenKind.IDENT):
            label = self._eat().text
        if self._eat_if(TokenKind.COLON):
            # Inline form: arena LABEL: expr
            inner = self._parse_expr()
            return {"_kind": "arena_inline", "label": label, "value": inner,
                    "span": (start, self._peek(-1).end)}
        body = self._parse_block()
        return {"_kind": "arena", "label": label, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_unsafe(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="unsafe")
        body = self._parse_block()
        return {"_kind": "unsafe", "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_budget(self) -> Node:
        # ``budget { wall = .., mem = .., ... } run <expr>`` or
        # ``budget { wall .., mem .. } run { ... }`` (example 11 uses
        # the bare-name form ``wall 2s``, no ``=``).
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="budget")
        entries = self._parse_kv_brace_block()
        run_kw = None
        if self._at_kw("run"):
            run_kw = self._eat()
        body = self._parse_expr() if run_kw else None
        return {"_kind": "budget", "entries": entries, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_sandbox(self) -> Node:
        # ``sandbox NAME with { entries } { body }`` per §16.1, A5.
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="sandbox")
        name: Optional[str] = None
        if self._at(TokenKind.IDENT):
            name = self._eat().text
        self._eat_if(TokenKind.KEYWORD, kw="with")
        entries = self._parse_kv_brace_block()
        body = self._parse_block() if self._at(TokenKind.LBRACE) else None
        return {"_kind": "sandbox", "name": name, "entries": entries,
                "body": body, "span": (start, self._peek(-1).end)}

    def _parse_task_scope(self) -> Node:
        # ``task scope @D { body }`` (§15.1).
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="task")
        # Optional ``scope`` ident before the body.
        if self._at(TokenKind.IDENT) and self._peek().text == "scope":
            self._eat()
        deadline = None
        if self._eat_if(TokenKind.AT):
            deadline = self._parse_unary()
        body = self._parse_block()
        return {"_kind": "task_scope", "deadline": deadline, "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_detach(self) -> Node:
        start = self._peek().start
        self._expect(TokenKind.KEYWORD, kw="detach")
        body = self._parse_block()
        return {"_kind": "detach", "body": body,
                "span": (start, self._peek(-1).end)}

    def _parse_kv_brace_block(self) -> list[Node]:
        # Used by sandbox / budget: ``{ key = value ; key value ; ... }``.
        # Tolerant of:
        #   * ``key = expr`` (sandbox-style, example 18)
        #   * ``key.subkey = expr`` (fs.read = ...)
        #   * ``key value`` (budget-style bare, example 11)
        #   * separated by newlines, ``,``, or ``;``.
        self._expect(TokenKind.LBRACE)
        out: list[Node] = []
        while not self._at(TokenKind.RBRACE) and not self._at(TokenKind.EOF):
            kstart = self._peek().start
            # Key: dotted identifier path.
            key_parts: list[str] = []
            if self._at(TokenKind.IDENT) or self._at(TokenKind.KEYWORD):
                key_parts.append(self._eat().text)
            else:
                # Shouldn't happen, but eat to avoid spinning.
                self._eat()
                continue
            while self._eat_if(TokenKind.DOT):
                if self._at(TokenKind.IDENT) or self._at(TokenKind.KEYWORD):
                    key_parts.append(self._eat().text)
                else:
                    break
            has_eq = bool(self._eat_if(TokenKind.EQ))
            value: Optional[Node] = None
            if has_eq or not (self._at(TokenKind.RBRACE)
                              or self._at(TokenKind.SEMI)
                              or self._at(TokenKind.COMMA)):
                value = self._parse_expr()
            out.append({
                "_kind": "kv_entry",
                "key": key_parts,
                "value": value,
                "span": (kstart, self._peek(-1).end),
            })
            # Optional separators.
            if self._eat_if(TokenKind.SEMI):
                continue
            if self._eat_if(TokenKind.COMMA):
                continue
            # Or just newline-separated; we already trimmed trivia so we
            # rely on the next iteration's token-class check.
        self._expect(TokenKind.RBRACE)
        return out


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def parse(source: str) -> tuple[Node, list[Diagnostic]]:
    """Parse a Mighty source string. Returns ``(file_node, diagnostics)``.

    Lexer diagnostics are concatenated in front of parser diagnostics.
    """
    tokens, lex_diags = lex(source)
    parser = Parser(tokens, source)
    tree = parser.parse_file()
    return tree, list(lex_diags) + list(parser.diagnostics)
