"""HIR (High-level Intermediate Representation) for the Python 2nd-impl.

The HIR is the parser AST after **name resolution** and **module-flat**
collection of all items. It is the input to the type checker.

Design notes (vs. the Rust reference at ``crates/mty-hir/``, which we
do NOT consult per the no-source-peek mandate):

* Dataclasses with ``__slots__``-free fields for clarity.
* Each node carries a ``span: tuple[int, int]`` (byte range, same as
  the parser nodes). Spans are propagated wholesale from the parser
  tree.
* Names are resolved to ``Resolution`` records. A resolution is one of:
  ``"local"`` (a let-binding or fn parameter in scope),
  ``"item"`` (a top-level item -- fn, struct, enum, etc.),
  ``"variant"`` (an enum variant by ``EnumName.VariantName``),
  ``"unknown"`` (unresolved -- diagnostic emitted at lower-time).
* The HIR is intentionally a smaller surface than the parser AST: we
  drop attribute bodies, ``unparsed`` token bags, deferred braced
  items (agents/protocols/sandbox), and macro definitions.

Why dataclasses and not ``dict``: the parser uses dicts because its
output is a structural diff target; HIR is consumed by typeck which
benefits from static field names + ``match`` exhaustiveness.

Spec sources consulted: v1.0-RC2 §5 (items), §6 (types), §11
(expressions). The HIR shapes follow spec terminology where possible.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional, Union


Span = tuple[int, int]


# ---------------------------------------------------------------------------
# Type expressions (the syntactic types referenced in signatures and
# annotations, *not* the inferred-typeck types -- those live in typeck.py).
# ---------------------------------------------------------------------------


@dataclass
class HirTy:
    """A syntactic type reference, post-resolution.

    ``name`` is a dotted path joined by ``.`` (e.g. ``"std.io.Reader"``);
    ``args`` are the type arguments (``Option[I32]`` → ``args=[I32]``).
    """

    name: str
    args: list["HirTy"] = field(default_factory=list)
    # Optional metadata for special type shapes:
    # ``kind`` ∈ {"path", "ref", "ptr", "tuple", "array", "slice",
    # "fn", "result", "dyn"}
    kind: str = "path"
    span: Span = (0, 0)
    # For ``kind="ref"`` / ``"ptr"``: mutability flag.
    mut: bool = False
    # For ``kind="ref"`` / ``"ptr"`` / ``"array"`` / ``"slice"`` / ``"result"``:
    # the inner type.
    inner: Optional["HirTy"] = None
    # For ``kind="tuple"`` / ``"fn"``: element types / param types.
    elems: list["HirTy"] = field(default_factory=list)
    # For ``kind="fn"``: the return type.
    ret: Optional["HirTy"] = None
    # For ``kind="result"`` with anonymous union: list of err types.
    errs: list["HirTy"] = field(default_factory=list)

    @staticmethod
    def unit() -> "HirTy":
        return HirTy(name="Unit", kind="path", span=(0, 0))


# ---------------------------------------------------------------------------
# Resolution records.
# ---------------------------------------------------------------------------


@dataclass
class Resolution:
    """Name-resolution result attached to ``HirIdent`` / ``HirPath`` nodes.

    Discriminator is ``kind``:
      * ``"local"``  — local binding; ``binding_id`` set.
      * ``"item"``   — top-level item; ``item_name`` set.
      * ``"variant"`` — enum variant; ``item_name`` is the enum,
                        ``variant_name`` is the variant.
      * ``"field"``  — struct field; ``item_name`` is the struct.
      * ``"builtin"`` — built-in (``log``, ``Some``, ``None``, etc.).
      * ``"unknown"`` — unresolved; typeck will report or skip.
    """

    kind: str
    binding_id: Optional[int] = None
    item_name: Optional[str] = None
    variant_name: Optional[str] = None


# ---------------------------------------------------------------------------
# Expressions.
# ---------------------------------------------------------------------------


@dataclass
class HirExprNode:
    """Base for all HIR expression nodes.

    Every node carries a span. The concrete shape is determined by the
    subclass (see ``HirLit``, ``HirIdent``, ``HirCall``, ...).
    """

    span: Span = (0, 0)


@dataclass
class HirLit(HirExprNode):
    """A literal: ``42``, ``3.14``, ``"hello"``, ``true``, ``()``.

    ``lit_kind`` mirrors the lexer's ``TokenKind.name`` for the literal
    (e.g. ``"INT_LITERAL"``, ``"STRING_LITERAL"``, ``"BOOL"``); ``text``
    is the raw source slice.
    """

    lit_kind: str = ""
    text: str = ""


@dataclass
class HirIdent(HirExprNode):
    """A bare-identifier reference, post-resolution."""

    name: str = ""
    res: Resolution = field(default_factory=lambda: Resolution(kind="unknown"))


@dataclass
class HirPath(HirExprNode):
    """A dotted-path reference like ``Shape.Circle`` or ``std.io.read``."""

    segments: list[str] = field(default_factory=list)
    res: Resolution = field(default_factory=lambda: Resolution(kind="unknown"))


@dataclass
class HirCall(HirExprNode):
    """A function call: ``callee(args...)``."""

    callee: HirExprNode = field(default_factory=HirExprNode)
    args: list[HirExprNode] = field(default_factory=list)


@dataclass
class HirMethodCall(HirExprNode):
    """A method call: ``target.name(args...)``."""

    target: HirExprNode = field(default_factory=HirExprNode)
    name: str = ""
    args: list[HirExprNode] = field(default_factory=list)


@dataclass
class HirField(HirExprNode):
    """A field access: ``target.name``."""

    target: HirExprNode = field(default_factory=HirExprNode)
    name: str = ""


@dataclass
class HirIndex(HirExprNode):
    """An index access: ``target[index]``."""

    target: HirExprNode = field(default_factory=HirExprNode)
    index: HirExprNode = field(default_factory=HirExprNode)


@dataclass
class HirBinOp(HirExprNode):
    """A binary operator: ``lhs op rhs``."""

    op: str = ""
    lhs: HirExprNode = field(default_factory=HirExprNode)
    rhs: HirExprNode = field(default_factory=HirExprNode)


@dataclass
class HirUnaryOp(HirExprNode):
    """A unary prefix operator: ``op operand``."""

    op: str = ""
    operand: HirExprNode = field(default_factory=HirExprNode)
    mut: bool = False  # for ``&mut x``


@dataclass
class HirIf(HirExprNode):
    """``if cond { then } else { else_ }``. ``else_`` may be None."""

    cond: HirExprNode = field(default_factory=HirExprNode)
    then: "HirBlock" = field(default_factory=lambda: HirBlock())
    else_: Optional[Union["HirBlock", "HirIf"]] = None


@dataclass
class HirArm:
    """A single ``match`` arm."""

    pat: "HirPat" = field(default_factory=lambda: HirPat())
    guard: Optional[HirExprNode] = None
    body: HirExprNode = field(default_factory=HirExprNode)
    span: Span = (0, 0)


@dataclass
class HirMatch(HirExprNode):
    """A ``match`` expression."""

    scrutinee: HirExprNode = field(default_factory=HirExprNode)
    arms: list[HirArm] = field(default_factory=list)


@dataclass
class HirBlock(HirExprNode):
    """A ``{ stmts; tail }`` block expression."""

    stmts: list["HirStmt"] = field(default_factory=list)
    tail: Optional[HirExprNode] = None


@dataclass
class HirReturn(HirExprNode):
    value: Optional[HirExprNode] = None


@dataclass
class HirBreak(HirExprNode):
    value: Optional[HirExprNode] = None


@dataclass
class HirContinue(HirExprNode):
    pass


@dataclass
class HirLoop(HirExprNode):
    body: HirBlock = field(default_factory=HirBlock)


@dataclass
class HirWhile(HirExprNode):
    cond: HirExprNode = field(default_factory=HirExprNode)
    body: HirBlock = field(default_factory=HirBlock)


@dataclass
class HirFor(HirExprNode):
    pat: "HirPat" = field(default_factory=lambda: HirPat())
    iter: HirExprNode = field(default_factory=HirExprNode)
    body: HirBlock = field(default_factory=HirBlock)


@dataclass
class HirStructLit(HirExprNode):
    """A record/struct literal: ``Name { f: v, ... }``."""

    path: list[str] = field(default_factory=list)
    fields: list[tuple[str, HirExprNode]] = field(default_factory=list)
    res: Resolution = field(default_factory=lambda: Resolution(kind="unknown"))


@dataclass
class HirTuple(HirExprNode):
    elems: list[HirExprNode] = field(default_factory=list)


@dataclass
class HirArray(HirExprNode):
    elems: list[HirExprNode] = field(default_factory=list)


@dataclass
class HirPropagate(HirExprNode):
    """``expr?`` — Result/Option propagate."""

    operand: HirExprNode = field(default_factory=HirExprNode)


@dataclass
class HirClosure(HirExprNode):
    params: list["HirParam"] = field(default_factory=list)
    ret_ty: Optional[HirTy] = None
    body: Optional[HirBlock] = None


@dataclass
class HirOpaque(HirExprNode):
    """Catch-all for parser shapes we don't yet lower (agents, spawn,
    sandbox, ask/send sugars, macro calls, ...). Typeck treats opaque
    nodes as ``TyAny`` (skipping rather than failing). This lets us
    sweep the full example corpus without forcing every shape through
    the type checker in v0.17.
    """

    parser_kind: str = ""
    # We retain the original parser dict for downstream tooling. The
    # type checker ignores it.
    raw: dict = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Patterns.
# ---------------------------------------------------------------------------


@dataclass
class HirPat:
    """A pattern node. Discriminator is ``kind``.

    Kinds:
      * ``"wild"`` — ``_``.
      * ``"literal"`` — ``42`` / ``"foo"`` / ``true``.
      * ``"range"`` — ``1..10`` / ``1..=10``.
      * ``"ident"`` — a fresh binding (``x``); ``name`` set.
      * ``"tuple"`` — ``(p1, p2, ...)``; ``elems`` set.
      * ``"tuple_struct"`` — ``Shape.Circle(r)``; ``path``/``elems`` set.
      * ``"struct"`` — ``Point { x, y }``; ``path``/``fields`` set.
      * ``"ref"`` — ``&p``; ``inner`` set.
    """

    kind: str = "wild"
    name: str = ""
    text: str = ""
    path: list[str] = field(default_factory=list)
    elems: list["HirPat"] = field(default_factory=list)
    fields: list[tuple[str, Optional["HirPat"]]] = field(default_factory=list)
    inner: Optional["HirPat"] = None
    mut: bool = False
    span: Span = (0, 0)


# ---------------------------------------------------------------------------
# Statements.
# ---------------------------------------------------------------------------


@dataclass
class HirStmt:
    """A statement. ``kind`` ∈ {"let", "expr", "assign"}."""

    kind: str = "expr"
    pat: Optional[HirPat] = None
    ty: Optional[HirTy] = None
    value: Optional[HirExprNode] = None
    target: Optional[HirExprNode] = None  # for assign
    op: str = ""  # for assign (=, +=, ...)
    span: Span = (0, 0)


# ---------------------------------------------------------------------------
# Items: functions, structs, enums, type aliases, constants.
# ---------------------------------------------------------------------------


@dataclass
class HirParam:
    """A function parameter."""

    name: str
    ty: HirTy
    span: Span = (0, 0)
    mut: bool = False
    binding_id: int = -1  # assigned during lowering


@dataclass
class HirGenericParam:
    """A generic parameter with optional bounds.

    Introduced in v0.19 to support generics-with-constraints typeck. The
    older code path (which kept generics as ``list[str]`` of plain names)
    is still accepted everywhere — ``HirFn.generics`` is a parallel field
    holding the same names for backward compatibility. The bound vocabulary
    is intentionally string-typed because we don't model trait items in
    the Python 2nd-impl; the type checker treats bounds as opaque tags it
    can verify presence-of-trait-name against.
    """

    name: str
    bounds: tuple[str, ...] = ()
    span: Span = (0, 0)


@dataclass
class HirFn:
    """A function declaration."""

    name: str
    params: list[HirParam] = field(default_factory=list)
    return_ty: HirTy = field(default_factory=HirTy.unit)
    effects: list[str] = field(default_factory=list)
    generics: list[str] = field(default_factory=list)
    body: Optional[HirBlock] = None
    span: Span = (0, 0)
    visibility: str = "private"
    has_body: bool = True
    # v0.19 — generic-parameter records with bounds. The plain ``generics``
    # list above is preserved for backward compatibility.
    generic_params: list[HirGenericParam] = field(default_factory=list)


@dataclass
class HirField_:  # underscore to avoid clash with expr-HirField
    name: str
    ty: HirTy
    span: Span = (0, 0)


@dataclass
class HirStruct:
    name: str
    fields: list[HirField_] = field(default_factory=list)
    generics: list[str] = field(default_factory=list)
    # ``shape`` ∈ {"record", "tuple", "unit"}.
    shape: str = "record"
    # For tuple shape, ``tuple_fields`` holds the positional types.
    tuple_fields: list[HirTy] = field(default_factory=list)
    span: Span = (0, 0)
    visibility: str = "private"


@dataclass
class HirVariant:
    name: str
    # ``shape`` ∈ {"unit", "tuple", "record"}.
    shape: str = "unit"
    tuple_fields: list[HirTy] = field(default_factory=list)
    record_fields: list[HirField_] = field(default_factory=list)
    span: Span = (0, 0)


@dataclass
class HirEnum:
    name: str
    variants: list[HirVariant] = field(default_factory=list)
    generics: list[str] = field(default_factory=list)
    span: Span = (0, 0)
    visibility: str = "private"


@dataclass
class HirTypeAlias:
    name: str
    ty: HirTy
    generics: list[str] = field(default_factory=list)
    span: Span = (0, 0)


@dataclass
class HirConst:
    name: str
    ty: HirTy
    value: HirExprNode = field(default_factory=HirExprNode)
    span: Span = (0, 0)


HirItem = Union[HirFn, HirStruct, HirEnum, HirTypeAlias, HirConst]


# ---------------------------------------------------------------------------
# Module: the root of a lowered file.
# ---------------------------------------------------------------------------


@dataclass
class HirModule:
    """The lowered form of a single source file."""

    items: list[HirItem] = field(default_factory=list)
    # Diagnostics produced during lowering (name-resolution failures,
    # unsupported shapes, etc.). Codes are in the MT15xx band.
    span: Span = (0, 0)

    # ----- convenience accessors -----

    def find_fn(self, name: str) -> Optional[HirFn]:
        for it in self.items:
            if isinstance(it, HirFn) and it.name == name:
                return it
        return None

    def find_struct(self, name: str) -> Optional[HirStruct]:
        for it in self.items:
            if isinstance(it, HirStruct) and it.name == name:
                return it
        return None

    def find_enum(self, name: str) -> Optional[HirEnum]:
        for it in self.items:
            if isinstance(it, HirEnum) and it.name == name:
                return it
        return None
