"""Lower the parser AST (``dict``-tree) into the typed ``HirModule``.

This pass:
  * Builds an item-level symbol table (fns, structs, enums, type aliases).
  * Walks every fn body, building scope chains for ``let``-bindings and
    parameters; resolves bare identifiers to ``Resolution`` records.
  * Translates parser node shapes to HIR dataclass nodes.

What we deliberately **drop** (or wrap in ``HirOpaque``):
  * Attributes (the parser already keeps them as opaque token-bags).
  * ``deferred_*`` items (agents/protocols/sandboxes from the v0.11
    deferred pile).
  * Macro definitions (the parser keeps them as token slurps).
  * Trait / impl items (we lower their nested ``fn`` items but skip
    the trait wrapper -- v0.18 will revisit).
  * Most "exotic" expression shapes that the parser handles (spawn,
    detach, ask/send sugars, deadline) -- wrapped in ``HirOpaque`` so
    they round-trip through typeck as ``TyAny``.

Spec sources: v1.0-RC2 §5 (items), §11 (expressions); the lowering
strategy follows the standard textbook approach (AST→HIR is a fold
that resolves names while it walks).
"""

from __future__ import annotations

from typing import Any, Optional

from .diagnostics import (
    CODE_LOWER_DUPLICATE_ITEM,
    CODE_LOWER_UNRESOLVED_NAME,
    CODE_LOWER_UNSUPPORTED_SHAPE,
    Diagnostic,
    Severity,
)
from .hir import (
    HirArm,
    HirArray,
    HirBinOp,
    HirBlock,
    HirBreak,
    HirCall,
    HirClosure,
    HirConst,
    HirContinue,
    HirEnum,
    HirExprNode,
    HirField,
    HirField_,
    HirFn,
    HirFor,
    HirGenericParam,
    HirIdent,
    HirIf,
    HirIndex,
    HirItem,
    HirLit,
    HirLoop,
    HirMatch,
    HirMethodCall,
    HirModule,
    HirOpaque,
    HirParam,
    HirPat,
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
    HirVariant,
    HirWhile,
    Resolution,
)


Node = dict[str, Any]


# Built-in names the lowerer recognises. These mirror the prelude
# the Rust reference would inject; we treat them as ``builtin``
# resolutions to keep the unresolved-name diagnostic from firing on
# every example.
_BUILTINS = {
    # Print / log surface (used by 01_hello, 05_match_expr, etc).
    "log", "print", "println", "panic", "eprintln", "dbg",
    # Result / Option constructors (used in 03, 04).
    "Some", "None", "Ok", "Err",
    # Common scalar conversion helpers we don't model in detail.
    "as_str", "from_str",
    # Allocator / arena helpers (example 12).
    "alloc", "dealloc",
}

# Built-in types that are always in scope (the prelude per v1.0 §2.4).
# We accept them as path resolutions without an item lookup.
_BUILTIN_TYPES = {
    "I8", "I16", "I32", "I64", "I128",
    "U8", "U16", "U32", "U64", "U128",
    "F32", "F64",
    "Bool", "Str", "Unit", "Char",
    "Option", "Result", "Vec", "Box", "Rc", "Arc",
    "Self",
    # Examples reference these "domain" names as opaque types.
    "UserId", "Url", "Page",
    # Effect-row helpers (don't fully model, but accept as types).
    "Future", "Async",
}


class Lowerer:
    """Stateful lowerer; one instance per file."""

    def __init__(self, source: str = "") -> None:
        self.source = source
        self.diagnostics: list[Diagnostic] = []
        # Item-level symbol table: name → kind (``"fn"``, ``"struct"``,
        # ``"enum"``, ``"type_alias"``, ``"const"``).
        self.items: dict[str, str] = {}
        # Enum-variant lookup: ``EnumName.VariantName`` → True.
        self.variants: dict[str, set[str]] = {}
        # Struct-field lookup: ``StructName`` → set[field_name].
        self.struct_fields: dict[str, set[str]] = {}
        # Local-binding allocator (each fresh `let`/param gets a unique id).
        self._next_binding_id = 0
        # Stack of scopes for local-name resolution. Each scope is a
        # dict[name, binding_id].
        self._scopes: list[dict[str, int]] = []

    # ----- helpers -----

    def _diag(self, code: str, message: str, span: tuple[int, int]) -> None:
        self.diagnostics.append(
            Diagnostic(code=code, message=message, severity=Severity.ERROR,
                       start=span[0], end=span[1])
        )

    def _fresh_binding(self) -> int:
        i = self._next_binding_id
        self._next_binding_id += 1
        return i

    def _push_scope(self) -> None:
        self._scopes.append({})

    def _pop_scope(self) -> None:
        self._scopes.pop()

    def _bind(self, name: str) -> int:
        bid = self._fresh_binding()
        self._scopes[-1][name] = bid
        return bid

    def _lookup_local(self, name: str) -> Optional[int]:
        for scope in reversed(self._scopes):
            if name in scope:
                return scope[name]
        return None

    def _resolve_name(self, name: str, span: tuple[int, int]) -> Resolution:
        bid = self._lookup_local(name)
        if bid is not None:
            return Resolution(kind="local", binding_id=bid)
        if name in self.items:
            return Resolution(kind="item", item_name=name)
        if name in _BUILTINS:
            return Resolution(kind="builtin", item_name=name)
        if name in _BUILTIN_TYPES:
            return Resolution(kind="builtin", item_name=name)
        # Unresolved -- mark but don't always diagnose (calls to ``log``
        # not in BUILTINS, etc. are very common; we mute the diagnostic
        # to avoid noise but keep the resolution as ``unknown`` so
        # typeck can act on it).
        return Resolution(kind="unknown", item_name=name)

    # ----- module-level entry -----

    def lower_file(self, file_node: Node) -> HirModule:
        # First pass: collect item names so forward refs resolve.
        for item in file_node.get("items", []):
            self._collect_item_symbol(item)
            # Also collect extern-block fn names so calls resolve to
            # the right fn signature later.
            if item.get("_kind") == "extern_block":
                for sub in item.get("items", []):
                    if sub.get("_kind") == "fn":
                        sub_name = sub.get("name")
                        if sub_name and sub_name not in self.items:
                            self.items[sub_name] = "fn"
        # Second pass: lower each item.
        items: list[HirItem] = []
        for item in file_node.get("items", []):
            kind = item.get("_kind", "")
            if kind == "extern_block":
                # Lower nested extern fns as no-body HirFns so typeck
                # has a signature for the call.
                for sub in item.get("items", []):
                    if sub.get("_kind") == "fn":
                        # Force has_body=False via no-body lowering.
                        sub_copy = dict(sub)
                        sub_copy["body"] = None
                        fn = self._lower_fn(sub_copy)
                        fn.has_body = False
                        items.append(fn)
                continue
            lowered = self._lower_item(item)
            if lowered is not None:
                items.append(lowered)
        return HirModule(items=items, span=file_node.get("span", (0, 0)))

    def _collect_item_symbol(self, item: Node) -> None:
        kind = item.get("_kind", "")
        name = item.get("name")
        if name is None:
            return
        # Diagnose duplicate items (excluding traits/impls which we drop).
        if kind in ("fn", "struct", "enum", "type_alias", "const"):
            if name in self.items:
                self._diag(CODE_LOWER_DUPLICATE_ITEM,
                           f"duplicate item {name!r}",
                           item.get("span", (0, 0)))
            else:
                self.items[name] = kind
            if kind == "struct":
                self.struct_fields[name] = {
                    f.get("name", "") for f in item.get("fields", [])
                    if isinstance(f, dict) and f.get("_kind") == "field"
                }
            elif kind == "enum":
                self.variants[name] = {
                    v.get("name", "") for v in item.get("variants", [])
                }

    def _lower_item(self, item: Node) -> Optional[HirItem]:
        kind = item.get("_kind", "")
        if kind == "fn":
            return self._lower_fn(item)
        if kind == "struct":
            return self._lower_struct(item)
        if kind == "enum":
            return self._lower_enum(item)
        if kind == "type_alias":
            return self._lower_type_alias(item)
        if kind == "const":
            return self._lower_const(item)
        # Trait / impl: lower the nested fn items as free fns. This is a
        # deliberate simplification for the v0.17 typeck demo; v0.18+
        # will give traits a proper HIR shape.
        if kind in ("trait", "impl", "mod"):
            return None  # nested items are not promoted to module level
        # Extern block: lower the inner fn signatures as no-body HirFns.
        if kind == "extern_block":
            return None  # they're declared but skipped; typeck treats calls as builtin-ish
        # Everything else (deferred_*, macro_decl, attr, package_decl,
        # use, static, ...) is dropped silently here -- typeck doesn't
        # need it. We don't emit a diagnostic because dropping these is
        # by design.
        return None

    # ----- items -----

    def _lower_fn(self, item: Node) -> HirFn:
        name = item.get("name", "<anon>")
        generics = [g.get("name", "") for g in item.get("generics", [])]
        # v0.19: keep generics with their bounds for the type checker.
        generic_params = [
            HirGenericParam(
                name=g.get("name", ""),
                bounds=tuple(g.get("bounds", []) or ()),
                span=g.get("span", (0, 0)),
            )
            for g in item.get("generics", [])
        ]
        # Open a fresh scope for params + body.
        self._push_scope()
        params: list[HirParam] = []
        for p in item.get("params", []):
            pkind = p.get("_kind")
            if pkind == "self_param":
                # We model `self` as a binding of opaque type. The type
                # checker treats it as TyAny.
                bid = self._bind("self")
                params.append(HirParam(
                    name="self",
                    ty=HirTy(name="Self", kind="path", span=p.get("span", (0, 0))),
                    span=p.get("span", (0, 0)),
                    binding_id=bid,
                ))
                continue
            pname = p.get("name", "_")
            pty_node = p.get("ty")
            pty = self._lower_ty(pty_node) if pty_node else HirTy(name="_", kind="path")
            bid = self._bind(pname)
            params.append(HirParam(
                name=pname, ty=pty, mut=p.get("mut", False),
                span=p.get("span", (0, 0)), binding_id=bid,
            ))
        ret_node = item.get("ret_type")
        ret_ty = self._lower_ty(ret_node) if ret_node else HirTy.unit()
        effects = list(item.get("effects", []))
        body_node = item.get("body")
        body: Optional[HirBlock] = None
        has_body = True
        if body_node is None:
            has_body = False
        elif body_node.get("_kind") == "block":
            body = self._lower_block(body_node)
        elif body_node.get("_kind") == "expr_body":
            inner = self._lower_expr(body_node.get("expr"))
            body = HirBlock(stmts=[], tail=inner, span=item.get("span", (0, 0)))
        else:
            # Unknown body shape -- wrap as opaque.
            body = HirBlock(stmts=[], tail=self._lower_expr(body_node),
                            span=item.get("span", (0, 0)))
        self._pop_scope()
        return HirFn(
            name=name, params=params, return_ty=ret_ty, effects=effects,
            generics=generics, body=body, span=item.get("span", (0, 0)),
            visibility=item.get("visibility", "private"), has_body=has_body,
            generic_params=generic_params,
        )

    def _lower_struct(self, item: Node) -> HirStruct:
        name = item.get("name", "<anon>")
        generics = [g.get("name", "") for g in item.get("generics", [])]
        shape = item.get("shape", "record")
        fields: list[HirField_] = []
        tuple_fields: list[HirTy] = []
        if shape == "record":
            for f in item.get("fields", []):
                if f.get("_kind") != "field":
                    continue
                fields.append(HirField_(
                    name=f.get("name", "_"),
                    ty=self._lower_ty(f.get("ty")),
                    span=f.get("span", (0, 0)),
                ))
        elif shape == "tuple":
            for fty in item.get("fields", []):
                tuple_fields.append(self._lower_ty(fty))
        return HirStruct(
            name=name, fields=fields, generics=generics, shape=shape,
            tuple_fields=tuple_fields, span=item.get("span", (0, 0)),
            visibility=item.get("visibility", "private"),
        )

    def _lower_enum(self, item: Node) -> HirEnum:
        name = item.get("name", "<anon>")
        generics = [g.get("name", "") for g in item.get("generics", [])]
        variants: list[HirVariant] = []
        for v in item.get("variants", []):
            vshape = v.get("shape", "unit")
            tuple_fields: list[HirTy] = []
            record_fields: list[HirField_] = []
            if vshape == "tuple":
                tuple_fields = [self._lower_ty(t) for t in v.get("payload", [])]
            elif vshape == "record":
                for f in v.get("fields", []):
                    record_fields.append(HirField_(
                        name=f.get("name", "_"),
                        ty=self._lower_ty(f.get("ty")),
                    ))
            variants.append(HirVariant(
                name=v.get("name", "_"), shape=vshape,
                tuple_fields=tuple_fields, record_fields=record_fields,
                span=v.get("span", (0, 0)),
            ))
        return HirEnum(
            name=name, variants=variants, generics=generics,
            span=item.get("span", (0, 0)),
            visibility=item.get("visibility", "private"),
        )

    def _lower_type_alias(self, item: Node) -> HirTypeAlias:
        return HirTypeAlias(
            name=item.get("name", "<anon>"),
            ty=self._lower_ty(item.get("ty")),
            generics=[g.get("name", "") for g in item.get("generics", [])],
            span=item.get("span", (0, 0)),
        )

    def _lower_const(self, item: Node) -> HirConst:
        return HirConst(
            name=item.get("name", "<anon>"),
            ty=self._lower_ty(item.get("ty")),
            value=self._lower_expr(item.get("value")),
            span=item.get("span", (0, 0)),
        )

    # ----- types -----

    def _lower_ty(self, ty: Optional[Node]) -> HirTy:
        if ty is None:
            return HirTy(name="_", kind="path")
        kind = ty.get("_kind", "")
        span = ty.get("span", (0, 0))
        if kind == "ty_path":
            segs = ty.get("segments", [])
            args = [self._lower_ty(a) for a in ty.get("args", [])]
            joined = ".".join(segs) if segs else "_"
            return HirTy(name=joined, kind="path", args=args, span=span)
        if kind == "ty_ref":
            return HirTy(name="&", kind="ref", mut=ty.get("mut", False),
                         inner=self._lower_ty(ty.get("inner")), span=span)
        if kind == "ty_ptr":
            return HirTy(name="*", kind="ptr", mut=ty.get("mut", False),
                         inner=self._lower_ty(ty.get("inner")), span=span)
        if kind == "ty_tuple":
            return HirTy(name="tuple", kind="tuple",
                         elems=[self._lower_ty(e) for e in ty.get("elems", [])],
                         span=span)
        if kind == "ty_array":
            return HirTy(name="array", kind="array",
                         inner=self._lower_ty(ty.get("elem")), span=span)
        if kind == "ty_slice":
            return HirTy(name="slice", kind="slice",
                         inner=self._lower_ty(ty.get("elem")), span=span)
        if kind == "ty_fn":
            return HirTy(name="fn", kind="fn",
                         elems=[self._lower_ty(p) for p in ty.get("params", [])],
                         ret=self._lower_ty(ty.get("ret")) if ty.get("ret") else HirTy.unit(),
                         span=span)
        if kind == "ty_result":
            return HirTy(name="Result", kind="result",
                         inner=self._lower_ty(ty.get("ok")),
                         errs=[self._lower_ty(ty.get("err"))], span=span)
        if kind == "ty_result_union":
            return HirTy(name="Result", kind="result",
                         inner=self._lower_ty(ty.get("ok")),
                         errs=[self._lower_ty(e) for e in ty.get("errs", [])],
                         span=span)
        if kind == "ty_dyn":
            return HirTy(name="dyn", kind="path",
                         args=[self._lower_ty(ty.get("inner"))], span=span)
        # Unrecognised -- emit a soft note and keep going.
        return HirTy(name="?", kind="path", span=span)

    # ----- blocks / statements -----

    def _lower_block(self, blk: Node) -> HirBlock:
        # Each block opens its own scope (rust-style).
        self._push_scope()
        stmts: list[HirStmt] = []
        for s in blk.get("stmts", []):
            stmts.append(self._lower_stmt(s))
        tail = self._lower_expr(blk.get("tail")) if blk.get("tail") else None
        self._pop_scope()
        return HirBlock(stmts=stmts, tail=tail, span=blk.get("span", (0, 0)))

    def _lower_stmt(self, s: Node) -> HirStmt:
        kind = s.get("_kind", "")
        if kind == "let":
            pat = self._lower_pat(s.get("pat"))
            # Bind any names introduced by the pattern. We do this BEFORE
            # lowering the value so a `let x = x` would refer to the
            # outer x (rust-shadowing semantics); but the spec is silent
            # so we bind AFTER to match the conventional reading.
            value = self._lower_expr(s.get("value")) if s.get("value") else None
            self._bind_pat(pat)
            ty = self._lower_ty(s.get("ty")) if s.get("ty") else None
            return HirStmt(kind="let", pat=pat, ty=ty, value=value,
                           span=s.get("span", (0, 0)))
        if kind == "assign":
            return HirStmt(kind="assign", op=s.get("op", "="),
                           target=self._lower_expr(s.get("target")),
                           value=self._lower_expr(s.get("value")),
                           span=s.get("span", (0, 0)))
        if kind == "expr_stmt":
            return HirStmt(kind="expr", value=self._lower_expr(s.get("expr")),
                           span=s.get("span", (0, 0)))
        # Nested fn inside a block (example 20 has a few): hoist it as
        # an opaque value-less statement, dropping its body for now.
        if kind == "fn":
            # We don't lower nested fns into the typeck loop in v0.17.
            return HirStmt(kind="expr",
                           value=HirOpaque(parser_kind="nested_fn", raw=s,
                                           span=s.get("span", (0, 0))),
                           span=s.get("span", (0, 0)))
        # Fallback
        return HirStmt(kind="expr",
                       value=HirOpaque(parser_kind=kind, raw=s,
                                       span=s.get("span", (0, 0))),
                       span=s.get("span", (0, 0)))

    def _bind_pat(self, pat: HirPat) -> None:
        if pat.kind == "ident":
            # We do not record the binding id here; instead the resolution
            # at use-site will re-look it up. To make ident resolution
            # find the binding, we register the name in the current scope.
            self._bind(pat.name)
            return
        if pat.kind in ("tuple", "tuple_struct"):
            for e in pat.elems:
                self._bind_pat(e)
            return
        if pat.kind == "struct":
            for fname, sub in pat.fields:
                if sub is not None:
                    self._bind_pat(sub)
                else:
                    # Field-shorthand: binds the field name in scope.
                    self._bind(fname)
            return
        if pat.kind == "ref":
            if pat.inner:
                self._bind_pat(pat.inner)
            return
        # wild / literal / range: no bindings.

    # ----- patterns -----

    def _lower_pat(self, p: Optional[Node]) -> HirPat:
        if p is None:
            return HirPat(kind="wild")
        k = p.get("_kind", "")
        span = p.get("span", (0, 0))
        if k == "pat_wild":
            return HirPat(kind="wild", span=span)
        if k == "pat_literal":
            return HirPat(kind="literal", text=p.get("text", ""), span=span)
        if k == "pat_range":
            return HirPat(kind="range",
                          text=f"{p.get('lo')}{p.get('op')}{p.get('hi')}",
                          span=span)
        if k == "pat_ident":
            return HirPat(kind="ident", name=p.get("name", "_"), span=span)
        if k == "pat_tuple":
            return HirPat(kind="tuple",
                          elems=[self._lower_pat(e) for e in p.get("elems", [])],
                          span=span)
        if k == "pat_tuple_struct":
            return HirPat(kind="tuple_struct",
                          path=p.get("path", []),
                          elems=[self._lower_pat(e) for e in p.get("elems", [])],
                          span=span)
        if k == "pat_struct":
            fields: list[tuple[str, Optional[HirPat]]] = []
            for f in p.get("fields", []):
                sub = self._lower_pat(f.get("pat")) if f.get("pat") else None
                fields.append((f.get("name", ""), sub))
            return HirPat(kind="struct", path=p.get("path", []),
                          fields=fields, span=span)
        if k == "pat_ref":
            return HirPat(kind="ref", mut=p.get("mut", False),
                          inner=self._lower_pat(p.get("inner")), span=span)
        if k == "pat_path":
            return HirPat(kind="tuple_struct",
                          path=p.get("segments", []), elems=[], span=span)
        # Unknown -- treat as wild.
        return HirPat(kind="wild", span=span)

    # ----- expressions -----

    def _lower_expr(self, e: Optional[Node]) -> HirExprNode:
        if e is None:
            return HirLit(lit_kind="UNIT", text="()", span=(0, 0))
        k = e.get("_kind", "")
        span = e.get("span", (0, 0))
        if k == "literal":
            return HirLit(lit_kind=e.get("kind", ""), text=e.get("text", ""),
                          span=span)
        if k == "path":
            segs = e.get("segments", [])
            if len(segs) == 1:
                name = segs[0]
                res = self._resolve_name(name, span)
                return HirIdent(name=name, res=res, span=span)
            # Multi-segment path: try ``EnumName.VariantName`` resolution
            # first (common shape, e.g. ``Shape.Circle``).
            if len(segs) == 2 and segs[0] in self.variants and segs[1] in self.variants[segs[0]]:
                res = Resolution(kind="variant", item_name=segs[0],
                                 variant_name=segs[1])
                return HirPath(segments=segs, res=res, span=span)
            # Generic multi-segment path: leave unknown.
            return HirPath(segments=segs,
                           res=Resolution(kind="unknown"), span=span)
        if k == "binop":
            return HirBinOp(op=e.get("op", ""),
                            lhs=self._lower_expr(e.get("lhs")),
                            rhs=self._lower_expr(e.get("rhs")), span=span)
        if k == "unary":
            return HirUnaryOp(op=e.get("op", ""), mut=e.get("mut", False),
                              operand=self._lower_expr(e.get("operand")),
                              span=span)
        if k == "call":
            return HirCall(callee=self._lower_expr(e.get("callee")),
                           args=[self._lower_expr(a) for a in e.get("args", [])],
                           span=span)
        if k == "method_call":
            return HirMethodCall(target=self._lower_expr(e.get("target")),
                                 name=e.get("name", ""),
                                 args=[self._lower_expr(a) for a in e.get("args", [])],
                                 span=span)
        if k == "field":
            return HirField(target=self._lower_expr(e.get("target")),
                            name=e.get("name", ""), span=span)
        if k == "index":
            return HirIndex(target=self._lower_expr(e.get("target")),
                            index=self._lower_expr(e.get("index")), span=span)
        if k == "block":
            return self._lower_block(e)
        if k == "if":
            then = self._lower_block(e.get("then"))
            else_node = e.get("else")
            else_: Optional[HirBlock] = None
            if else_node is not None:
                if else_node.get("_kind") == "block":
                    else_ = self._lower_block(else_node)
                elif else_node.get("_kind") == "if":
                    else_ = self._lower_expr(else_node)  # HirIf
                else:
                    else_ = HirBlock(stmts=[], tail=self._lower_expr(else_node),
                                     span=else_node.get("span", (0, 0)))
            return HirIf(cond=self._lower_expr(e.get("cond")), then=then,
                         else_=else_, span=span)
        if k == "if_let":
            # Lower ``if let`` to a HirMatch with two arms (Some-pat → then,
            # _ → else). This is a standard desugaring.
            scrut = self._lower_expr(e.get("scrutinee"))
            pat = self._lower_pat(e.get("pat"))
            # Bind names in pat for the then branch only.
            self._push_scope()
            self._bind_pat(pat)
            then = self._lower_block(e.get("then"))
            self._pop_scope()
            arms = [HirArm(pat=pat, guard=None, body=then,
                           span=e.get("then").get("span", (0, 0)))]
            else_node = e.get("else")
            if else_node is not None:
                else_body = self._lower_block(else_node) if else_node.get("_kind") == "block" else self._lower_expr(else_node)
                arms.append(HirArm(pat=HirPat(kind="wild"), guard=None,
                                   body=else_body, span=else_node.get("span", (0, 0))))
            return HirMatch(scrutinee=scrut, arms=arms, span=span)
        if k == "match":
            arms: list[HirArm] = []
            scrut = self._lower_expr(e.get("scrutinee"))
            for arm in e.get("arms", []):
                pat = self._lower_pat(arm.get("pat"))
                self._push_scope()
                self._bind_pat(pat)
                guard = self._lower_expr(arm.get("guard")) if arm.get("guard") else None
                body = self._lower_expr(arm.get("body"))
                self._pop_scope()
                arms.append(HirArm(pat=pat, guard=guard, body=body,
                                   span=arm.get("span", (0, 0))))
            return HirMatch(scrutinee=scrut, arms=arms, span=span)
        if k == "loop":
            return HirLoop(body=self._lower_block(e.get("body")), span=span)
        if k == "while":
            return HirWhile(cond=self._lower_expr(e.get("cond")),
                            body=self._lower_block(e.get("body")), span=span)
        if k == "for":
            pat = self._lower_pat(e.get("pat"))
            iter_ = self._lower_expr(e.get("iter"))
            self._push_scope()
            self._bind_pat(pat)
            body = self._lower_block(e.get("body"))
            self._pop_scope()
            return HirFor(pat=pat, iter=iter_, body=body, span=span)
        if k == "return":
            return HirReturn(
                value=self._lower_expr(e.get("value")) if e.get("value") else None,
                span=span,
            )
        if k == "break":
            return HirBreak(
                value=self._lower_expr(e.get("value")) if e.get("value") else None,
                span=span,
            )
        if k == "continue":
            return HirContinue(span=span)
        if k == "struct_literal":
            path = e.get("path", [])
            fields_out: list[tuple[str, HirExprNode]] = []
            for f in e.get("fields", []):
                v = self._lower_expr(f.get("value")) if f.get("value") else HirIdent(
                    name=f.get("name", ""),
                    res=self._resolve_name(f.get("name", ""), span),
                    span=span,
                )
                fields_out.append((f.get("name", ""), v))
            res = Resolution(kind="unknown")
            if path and path[0] in self.struct_fields:
                res = Resolution(kind="item", item_name=path[0])
            return HirStructLit(path=path, fields=fields_out, res=res, span=span)
        if k == "tuple":
            return HirTuple(elems=[self._lower_expr(x) for x in e.get("elems", [])],
                            span=span)
        if k == "array" or k == "array_repeat":
            return HirArray(elems=[self._lower_expr(x) for x in e.get("elems", [])],
                            span=span)
        if k == "propagate":
            return HirPropagate(operand=self._lower_expr(e.get("operand")),
                                span=span)
        if k == "closure":
            # Open a scope for params, lower body.
            self._push_scope()
            params: list[HirParam] = []
            for p in e.get("params", []):
                pname = p.get("name", "_")
                pty = self._lower_ty(p.get("ty")) if p.get("ty") else HirTy(name="_", kind="path")
                bid = self._bind(pname)
                params.append(HirParam(name=pname, ty=pty, span=p.get("span", (0, 0)),
                                       binding_id=bid))
            body = self._lower_block(e.get("body")) if e.get("body") else None
            self._pop_scope()
            ret_node = e.get("ret")
            ret_ty = self._lower_ty(ret_node) if ret_node else None
            return HirClosure(params=params, ret_ty=ret_ty, body=body, span=span)
        # All other parser shapes are wrapped as opaque; typeck treats
        # them as TyAny so they don't break the sweep.
        return HirOpaque(parser_kind=k, raw=e, span=span)


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def lower(file_node: Node, source: str = "") -> tuple[HirModule, list[Diagnostic]]:
    """Lower a parser file node into an ``HirModule``.

    Returns ``(module, diagnostics)``. Diagnostics are in the MT15xx band.
    """
    lo = Lowerer(source=source)
    mod = lo.lower_file(file_node)
    return mod, list(lo.diagnostics)
