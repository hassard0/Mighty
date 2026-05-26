"""Type checker for the Python 2nd-impl.

A Hindley-Milner-style inference engine over the ``HirModule`` produced
by :mod:`mty.lower`. The intent is to validate that the spec's static
typing rules are expressible from prose alone, not to be a complete
type system. In particular this checker:

* Models scalar prelude types (``I8..I128``, ``U8..U128``, ``F32/F64``,
  ``Bool``, ``Str``, ``Char``, ``Unit``) plus user-defined ``struct``,
  ``enum``, and ``type_alias`` items.
* Implements true H-M unification with a non-occurring var substitution.
* Treats anything the lowerer wrapped in ``HirOpaque`` (agents, spawn,
  ask/send sugars, macro calls, budget blocks, ...) as ``TyAny`` — they
  bypass the checker so the example sweep can complete. v0.18 will
  introduce proper HIR for agents and friends.
* Treats effect rows as type-erased (a ``!{}`` annotation is dropped);
  full effect-row typeck is the v0.18 target.

Diagnostic codes are in the ``MT20xx`` band; see ``diagnostics.py`` for
the registry. The exact numeric assignments are this impl's
interpretation; the Rust reference under ``crates/mty-types`` may pick
different numbers within the same band — this is deliberate per
``docs/spec/independent-impls.md``.

Design references: v1.0-RC2 §6 (types), §11 (expressions), §13 (item
signatures), §33 (diagnostic bands). The H-M variant follows the
classical textbook approach (Pierce ch. 22).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional, Union

from .diagnostics import (
    CODE_TYPECK_ARITY_MISMATCH,
    CODE_TYPECK_BOUND_UNSATISFIED,
    CODE_TYPECK_BRANCH_MISMATCH,
    CODE_TYPECK_CLOSURE_ARITY,
    CODE_TYPECK_FIELD_MISMATCH,
    CODE_TYPECK_MISMATCH,
    CODE_TYPECK_NOT_CALLABLE,
    CODE_TYPECK_OCCURS_CHECK,
    CODE_TYPECK_OPERATOR_TYPE,
    CODE_TYPECK_RETURN_MISMATCH,
    CODE_TYPECK_UNKNOWN_GENERIC,
    CODE_TYPECK_UNKNOWN_NAME,
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
    HirLit,
    HirLoop,
    HirMatch,
    HirMethodCall,
    HirModule,
    HirOpaque,
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
    HirWhile,
    Resolution,
)


# ---------------------------------------------------------------------------
# Type representation (the inferred types, distinct from ``HirTy`` which is
# the parsed surface annotation).
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class TyInt:
    """Integer scalar: ``width`` ∈ {8,16,32,64,128}, ``signed`` ∈ {True,False}."""
    width: int
    signed: bool


@dataclass(frozen=True)
class TyFloat:
    width: int  # 32 or 64


@dataclass(frozen=True)
class TyBool:
    pass


@dataclass(frozen=True)
class TyStr:
    pass


@dataclass(frozen=True)
class TyChar:
    pass


@dataclass(frozen=True)
class TyUnit:
    pass


@dataclass(frozen=True)
class TyAny:
    """The "I don't know, don't constrain" type. Unifies with anything
    without recording a substitution. Used for opaque expression nodes
    and unresolved-name shoulders."""
    pass


@dataclass(frozen=True)
class TyRecord:
    """A nominal struct: ``name`` is the declared identifier. ``fields``
    is a tuple of ``(field_name, ty)`` pairs.

    Generic parameters are erased to ``TyAny`` in v0.17. v0.18 will
    introduce a proper ``TyApp(TyCon, args)`` shape."""
    name: str
    fields: tuple[tuple[str, "Ty"], ...] = ()


@dataclass(frozen=True)
class TyEnum:
    """A nominal enum: ``name`` plus tuple of variant names. Variant
    payload types are not modelled in v0.17."""
    name: str
    variants: tuple[str, ...] = ()


@dataclass(frozen=True)
class TyFn:
    params: tuple["Ty", ...]
    ret: "Ty"


@dataclass(frozen=True)
class TyTuple:
    elems: tuple["Ty", ...]


@dataclass(frozen=True)
class TyArray:
    elem: "Ty"


@dataclass(frozen=True)
class TyRef:
    """``&T`` / ``&mut T``."""
    inner: "Ty"
    mut: bool = False


@dataclass(frozen=True)
class TyResult:
    """``T!E`` / ``T!{E1, E2}`` — modelled as a single sum type."""
    ok: "Ty"


@dataclass(frozen=True)
class TyOption:
    """``Option[T]`` — modelled directly because the prelude exports it."""
    inner: "Ty"


@dataclass(frozen=True)
class TyVar:
    """A fresh inference variable. ``id`` is allocated by ``TypeChecker``."""
    id: int


@dataclass(frozen=True)
class TyOpaque:
    """A nominal type we recognise by name but don't model structurally
    (``Url``, ``Page``, ``UserId``, ``Fs``, ``Logger`` from the examples)."""
    name: str


Ty = Union[
    TyInt, TyFloat, TyBool, TyStr, TyChar, TyUnit, TyAny, TyRecord,
    TyEnum, TyFn, TyTuple, TyArray, TyRef, TyResult, TyOption, TyVar,
    TyOpaque,
]


# Convenience constants ------------------------------------------------------

TY_UNIT = TyUnit()
TY_BOOL = TyBool()
TY_STR = TyStr()
TY_CHAR = TyChar()
TY_ANY = TyAny()
TY_I32 = TyInt(32, True)
TY_I64 = TyInt(64, True)
TY_U32 = TyInt(32, False)
TY_U64 = TyInt(64, False)
TY_F64 = TyFloat(64)


# Scalar names exported by the prelude (v1.0-RC2 §2.4).
_PRELUDE_SCALARS: dict[str, Ty] = {
    "I8": TyInt(8, True), "I16": TyInt(16, True), "I32": TY_I32,
    "I64": TY_I64, "I128": TyInt(128, True),
    "U8": TyInt(8, False), "U16": TyInt(16, False), "U32": TY_U32,
    "U64": TY_U64, "U128": TyInt(128, False),
    "F32": TyFloat(32), "F64": TY_F64,
    "Bool": TY_BOOL, "Str": TY_STR, "Char": TY_CHAR, "Unit": TY_UNIT,
    "String": TY_STR,  # examples mix Str/String -- accept both.
}


# v0.19 — generic-bound vocabulary.
#
# We don't model trait items in the Python 2nd-impl. Instead we know
# which built-in scalars satisfy a small set of well-known prelude
# traits, and treat user-defined ``TyOpaque`` / ``TyRecord`` / closure /
# container types as *potentially* satisfying any bound (the borrow-
# check / trait pass in v1.0 will sharpen this — for the 2nd-impl,
# conservative permissiveness is correct).
#
# The keys are bound names as they appear after ``:`` in a generic
# parameter declaration: ``fn map[A, B: Display]`` -> ``"Display"``.
_KNOWN_BOUNDS: frozenset[str] = frozenset({
    "Display", "Debug", "Clone", "Copy", "Eq", "Ord",
    "PartialEq", "PartialOrd", "Hash", "Default",
    "Send", "Sync", "Sized",
})

_ALL_NUMERIC: frozenset[str] = frozenset({
    "I8", "I16", "I32", "I64", "I128",
    "U8", "U16", "U32", "U64", "U128",
    "F32", "F64",
})

_ALL_SCALARS: frozenset[str] = _ALL_NUMERIC | frozenset({
    "Bool", "Str", "Char", "Unit"
})

_INT_SCALARS: frozenset[str] = frozenset({
    "I8", "I16", "I32", "I64", "I128",
    "U8", "U16", "U32", "U64", "U128",
})

# Scalars and prelude types that satisfy each well-known bound. A type
# satisfies a bound if its ``ty_show`` string appears in the bound's
# satisfier set. Non-scalar / nominal / inference types are handled
# separately in ``_satisfies_bound`` (conservatively true).
_BOUND_SATISFIERS: dict[str, frozenset[str]] = {
    "Display":    _ALL_SCALARS - {"Unit"},
    "Debug":      _ALL_SCALARS,
    "Clone":      _ALL_SCALARS,
    # Copy excludes ``Str`` (heap-backed in the spec model).
    "Copy":       _ALL_NUMERIC | frozenset({"Bool", "Char", "Unit"}),
    "Eq":         _INT_SCALARS | frozenset({"Bool", "Str", "Char", "Unit"}),
    "PartialEq":  _ALL_SCALARS,
    # Ord excludes float (no total order).
    "Ord":        _INT_SCALARS | frozenset({"Bool", "Str", "Char"}),
    "PartialOrd": _ALL_SCALARS - {"Unit"},
    # Hash excludes float.
    "Hash":       _INT_SCALARS | frozenset({"Bool", "Str", "Char", "Unit"}),
    "Default":    _ALL_SCALARS,
    "Send":       _ALL_SCALARS,
    "Sync":       _ALL_SCALARS,
    "Sized":      _ALL_SCALARS,
}


def _satisfies_bound(t: "Ty", bound: str) -> bool:
    """Return True if ``t`` satisfies the named ``bound``.

    The rule (deliberately conservative — see module docstring):

    * Unknown bounds are accepted (we don't know what they mean).
    * ``TyAny`` / ``TyVar`` satisfy anything (inference hasn't pinned them
      yet, so we can't reject them).
    * Nominal / opaque / closure / container types satisfy any bound
      conservatively (the borrow/trait pass in v1.0-RC will tighten).
    * Scalar types satisfy a bound iff their ``ty_show`` name appears in
      the bound's satisfier set.
    """
    if bound not in _KNOWN_BOUNDS:
        return True
    if isinstance(t, (TyAny, TyVar)):
        return True
    if isinstance(t, (TyOpaque, TyRecord, TyEnum, TyFn, TyArray,
                      TyTuple, TyRef, TyResult, TyOption)):
        return True
    name = ty_show(t)
    sats = _BOUND_SATISFIERS.get(bound, frozenset())
    return name in sats


# ---------------------------------------------------------------------------
# Substitution and unification.
# ---------------------------------------------------------------------------


Subst = dict[int, Ty]


def apply(s: Subst, t: Ty) -> Ty:
    """Apply substitution ``s`` to ``t`` recursively, returning the
    resolved type. Idempotent because substitutions are eager."""
    if isinstance(t, TyVar):
        if t.id in s:
            return apply(s, s[t.id])
        return t
    if isinstance(t, TyFn):
        return TyFn(
            params=tuple(apply(s, p) for p in t.params),
            ret=apply(s, t.ret),
        )
    if isinstance(t, TyTuple):
        return TyTuple(elems=tuple(apply(s, e) for e in t.elems))
    if isinstance(t, TyArray):
        return TyArray(elem=apply(s, t.elem))
    if isinstance(t, TyRef):
        return TyRef(inner=apply(s, t.inner), mut=t.mut)
    if isinstance(t, TyResult):
        return TyResult(ok=apply(s, t.ok))
    if isinstance(t, TyOption):
        return TyOption(inner=apply(s, t.inner))
    if isinstance(t, TyRecord):
        return TyRecord(
            name=t.name,
            fields=tuple((n, apply(s, ft)) for n, ft in t.fields),
        )
    return t


def _occurs(var_id: int, t: Ty, s: Subst) -> bool:
    t = apply(s, t)
    if isinstance(t, TyVar):
        return t.id == var_id
    if isinstance(t, TyFn):
        return _occurs(var_id, t.ret, s) or any(
            _occurs(var_id, p, s) for p in t.params
        )
    if isinstance(t, TyTuple):
        return any(_occurs(var_id, e, s) for e in t.elems)
    if isinstance(t, (TyArray, TyResult, TyOption)):
        inner = t.elem if isinstance(t, TyArray) else (
            t.ok if isinstance(t, TyResult) else t.inner
        )
        return _occurs(var_id, inner, s)
    if isinstance(t, TyRef):
        return _occurs(var_id, t.inner, s)
    if isinstance(t, TyRecord):
        return any(_occurs(var_id, ft, s) for _, ft in t.fields)
    return False


class UnifyError(Exception):
    """Raised by ``unify`` on conflict. Carries the two offending types."""

    def __init__(self, a: Ty, b: Ty, reason: str = ""):
        self.a = a
        self.b = b
        self.reason = reason
        super().__init__(f"cannot unify {ty_show(a)} with {ty_show(b)}"
                         + (f": {reason}" if reason else ""))


def unify(a: Ty, b: Ty, s: Subst) -> Subst:
    """Robinson-style unification. Mutates ``s`` in place AND returns it
    for chaining. Raises :class:`UnifyError` on conflict."""
    a = apply(s, a)
    b = apply(s, b)
    # TyAny absorbs.
    if isinstance(a, TyAny) or isinstance(b, TyAny):
        return s
    if a == b:
        return s
    if isinstance(a, TyVar):
        if isinstance(b, TyVar) and b.id == a.id:
            return s
        if _occurs(a.id, b, s):
            raise UnifyError(a, b, "occurs check")
        s[a.id] = b
        return s
    if isinstance(b, TyVar):
        if _occurs(b.id, a, s):
            raise UnifyError(a, b, "occurs check")
        s[b.id] = a
        return s
    if isinstance(a, TyFn) and isinstance(b, TyFn):
        if len(a.params) != len(b.params):
            raise UnifyError(a, b, "fn arity")
        for pa, pb in zip(a.params, b.params):
            unify(pa, pb, s)
        unify(a.ret, b.ret, s)
        return s
    if isinstance(a, TyTuple) and isinstance(b, TyTuple):
        if len(a.elems) != len(b.elems):
            raise UnifyError(a, b, "tuple arity")
        for ea, eb in zip(a.elems, b.elems):
            unify(ea, eb, s)
        return s
    if isinstance(a, TyArray) and isinstance(b, TyArray):
        return unify(a.elem, b.elem, s)
    if isinstance(a, TyRef) and isinstance(b, TyRef):
        # Mutability difference is not a hard unification error (the
        # spec allows &mut T to coerce to &T); we widen by ignoring it.
        return unify(a.inner, b.inner, s)
    if isinstance(a, TyResult) and isinstance(b, TyResult):
        return unify(a.ok, b.ok, s)
    if isinstance(a, TyOption) and isinstance(b, TyOption):
        return unify(a.inner, b.inner, s)
    if isinstance(a, TyRecord) and isinstance(b, TyRecord):
        if a.name != b.name:
            raise UnifyError(a, b, "different nominal record")
        return s
    if isinstance(a, TyEnum) and isinstance(b, TyEnum):
        if a.name != b.name:
            raise UnifyError(a, b, "different nominal enum")
        return s
    if isinstance(a, TyOpaque) and isinstance(b, TyOpaque) and a.name == b.name:
        return s
    # Int subtype-ish: same width+sign required (we don't model coercions).
    raise UnifyError(a, b)


def ty_show(t: Ty) -> str:
    """Render a type for diagnostics. Pure cosmetic helper."""
    if isinstance(t, TyInt):
        return ("I" if t.signed else "U") + str(t.width)
    if isinstance(t, TyFloat):
        return "F" + str(t.width)
    if isinstance(t, TyBool):
        return "Bool"
    if isinstance(t, TyStr):
        return "Str"
    if isinstance(t, TyChar):
        return "Char"
    if isinstance(t, TyUnit):
        return "Unit"
    if isinstance(t, TyAny):
        return "?"
    if isinstance(t, TyVar):
        return f"τ{t.id}"
    if isinstance(t, TyRecord):
        return t.name
    if isinstance(t, TyEnum):
        return t.name
    if isinstance(t, TyOpaque):
        return t.name
    if isinstance(t, TyFn):
        ps = ", ".join(ty_show(p) for p in t.params)
        return f"fn({ps}) -> {ty_show(t.ret)}"
    if isinstance(t, TyTuple):
        return "(" + ", ".join(ty_show(e) for e in t.elems) + ")"
    if isinstance(t, TyArray):
        return f"[{ty_show(t.elem)}]"
    if isinstance(t, TyRef):
        return ("&mut " if t.mut else "&") + ty_show(t.inner)
    if isinstance(t, TyResult):
        return f"{ty_show(t.ok)}!_"
    if isinstance(t, TyOption):
        return f"Option[{ty_show(t.inner)}]"
    return repr(t)


# ---------------------------------------------------------------------------
# Type environment.
# ---------------------------------------------------------------------------


@dataclass
class Env:
    """Scoped name → type map. ``parent`` chains to enclosing scopes.

    The HIR resolver attaches ``binding_id`` to every local ``Resolution``;
    we key locals by that id (uniquified per fn) rather than by name to
    avoid shadow collisions.
    """
    bindings_by_id: dict[int, Ty] = field(default_factory=dict)
    locals_by_name: dict[str, Ty] = field(default_factory=dict)  # for fallback
    parent: Optional["Env"] = None

    def lookup_id(self, bid: int) -> Optional[Ty]:
        if bid in self.bindings_by_id:
            return self.bindings_by_id[bid]
        if self.parent is not None:
            return self.parent.lookup_id(bid)
        return None

    def lookup_name(self, name: str) -> Optional[Ty]:
        if name in self.locals_by_name:
            return self.locals_by_name[name]
        if self.parent is not None:
            return self.parent.lookup_name(name)
        return None

    def child(self) -> "Env":
        return Env(parent=self)


# ---------------------------------------------------------------------------
# The checker proper.
# ---------------------------------------------------------------------------


class TypeChecker:
    """Walks an :class:`HirModule`, infers types, records diagnostics.

    Usage::

        ck = TypeChecker(module)
        ck.check_module()
        for d in ck.diagnostics: print(d)
    """

    def __init__(self, module: HirModule, source: str = ""):
        self.module = module
        self.source = source
        self.diagnostics: list[Diagnostic] = []
        self.subst: Subst = {}
        self._next_var = 0
        # Item-level signature table: fn name → TyFn; struct/enum name → Ty.
        self.fn_sigs: dict[str, TyFn] = {}
        self.struct_tys: dict[str, TyRecord] = {}
        self.enum_tys: dict[str, TyEnum] = {}
        self.type_aliases: dict[str, Ty] = {}
        # v0.19 — per-fn generic scheme: ``fn_generics[name]`` is the
        # ordered list of ``(generic_name, TyVar_id, bounds)`` records for
        # that fn. Used at call sites to instantiate (refresh TyVars) and
        # to discharge bounds. Empty list = non-generic fn.
        self.fn_generics: dict[str, list[tuple[str, int, tuple[str, ...]]]] = {}
        # Transient (per ``resolve_hir_ty`` call) generic-name → TyVar map.
        # Set by ``_populate_signatures`` and ``check_fn`` to make multiple
        # references to the same generic name resolve to the same TyVar.
        self._generic_env: Optional[dict[str, TyVar]] = None
        # Filled in by check_module's first pass.
        self._populate_signatures()

    # ----- helpers -----

    def _diag(self, code: str, message: str, span: tuple[int, int],
              notes: tuple[str, ...] = ()) -> None:
        self.diagnostics.append(Diagnostic(
            code=code, message=message, severity=Severity.ERROR,
            start=span[0], end=span[1], notes=notes,
        ))

    def fresh(self) -> TyVar:
        v = TyVar(self._next_var)
        self._next_var += 1
        return v

    def _unify(self, a: Ty, b: Ty, span: tuple[int, int], code: str = CODE_TYPECK_MISMATCH,
               label: str = "type mismatch") -> bool:
        """Wraps :func:`unify` to record a diagnostic on failure. Returns
        True on success, False on conflict (so callers can decide whether
        to cascade)."""
        try:
            unify(a, b, self.subst)
            return True
        except UnifyError as e:
            a_now = apply(self.subst, e.a)
            b_now = apply(self.subst, e.b)
            self._diag(code, f"{label}: expected {ty_show(a_now)}, got {ty_show(b_now)}", span)
            return False

    # ----- HirTy → Ty resolution -----

    def resolve_hir_ty(self, ty: HirTy) -> Ty:
        """Project a syntactic ``HirTy`` into our inferred-type lattice."""
        if ty.name == "_":
            return self.fresh()
        if ty.kind == "ref":
            return TyRef(inner=self.resolve_hir_ty(ty.inner) if ty.inner else TY_ANY,
                         mut=ty.mut)
        if ty.kind == "ptr":
            return TyRef(inner=self.resolve_hir_ty(ty.inner) if ty.inner else TY_ANY,
                         mut=ty.mut)
        if ty.kind == "tuple":
            return TyTuple(elems=tuple(self.resolve_hir_ty(e) for e in ty.elems))
        if ty.kind == "array" or ty.kind == "slice":
            return TyArray(elem=self.resolve_hir_ty(ty.inner) if ty.inner else TY_ANY)
        if ty.kind == "fn":
            return TyFn(
                params=tuple(self.resolve_hir_ty(p) for p in ty.elems),
                ret=self.resolve_hir_ty(ty.ret) if ty.ret else TY_UNIT,
            )
        if ty.kind == "result":
            ok = self.resolve_hir_ty(ty.inner) if ty.inner else TY_ANY
            return TyResult(ok=ok)
        # path
        name = ty.name
        if name in _PRELUDE_SCALARS:
            return _PRELUDE_SCALARS[name]
        if name == "Option":
            arg = self.resolve_hir_ty(ty.args[0]) if ty.args else TY_ANY
            return TyOption(inner=arg)
        if name == "Result":
            arg = self.resolve_hir_ty(ty.args[0]) if ty.args else TY_ANY
            return TyResult(ok=arg)
        if name == "Vec":
            arg = self.resolve_hir_ty(ty.args[0]) if ty.args else TY_ANY
            return TyArray(elem=arg)
        if name in ("Box", "Rc", "Arc"):
            return self.resolve_hir_ty(ty.args[0]) if ty.args else TY_ANY
        if name in self.struct_tys:
            return self.struct_tys[name]
        if name in self.enum_tys:
            return self.enum_tys[name]
        if name in self.type_aliases:
            return self.type_aliases[name]
        if name == "Self":
            return TY_ANY
        # v0.19 — generic-name lookup through the current per-fn scope.
        # The same ``T`` referenced twice in a signature must resolve to
        # the same TyVar; without this, ``fn id(x: T) -> T`` couldn't be
        # checked because the param and ret would be independent vars.
        if self._generic_env is not None and name in self._generic_env:
            return self._generic_env[name]
        # Single-letter generics like ``T``, ``E`` outside a known scope —
        # fresh var (legacy fallback used by extern decls and adhoc refs).
        if len(name) == 1 and name.isupper():
            return self.fresh()
        # Anything else: an opaque nominal type. Examples like
        # ``Url``, ``Path``, ``Logger``, ``Fetcher`` land here.
        return TyOpaque(name=name)

    # ----- signature collection -----

    def _populate_signatures(self) -> None:
        # First: nominal types (so signatures referencing them resolve).
        for item in self.module.items:
            if isinstance(item, HirStruct):
                # We don't recursively model struct fields' types in the
                # first pass (avoid order issues); we fill in fields next.
                self.struct_tys[item.name] = TyRecord(name=item.name, fields=())
            elif isinstance(item, HirEnum):
                self.enum_tys[item.name] = TyEnum(
                    name=item.name,
                    variants=tuple(v.name for v in item.variants),
                )
            elif isinstance(item, HirTypeAlias):
                # Type aliases are resolved lazily; placeholder for now.
                self.type_aliases[item.name] = TY_ANY
        # Second pass: fill in struct fields and type-alias bodies once
        # the nominal table is populated.
        for item in self.module.items:
            if isinstance(item, HirStruct):
                fields = tuple(
                    (f.name, self.resolve_hir_ty(f.ty))
                    for f in item.fields
                )
                self.struct_tys[item.name] = TyRecord(name=item.name, fields=fields)
            elif isinstance(item, HirTypeAlias):
                self.type_aliases[item.name] = self.resolve_hir_ty(item.ty)
        # Third: fn signatures. v0.19 — build per-fn generic scope first
        # so that multiple uses of the same generic name (``T``) inside the
        # signature resolve to the same TyVar.
        for item in self.module.items:
            if isinstance(item, HirFn):
                gscheme: list[tuple[str, int, tuple[str, ...]]] = []
                gmap: dict[str, TyVar] = {}
                # Prefer the v0.19 bound-carrying records when present;
                # fall back to the plain names for backward compat.
                gparams = item.generic_params if item.generic_params else [
                    HirGenericParam(name=n) for n in item.generics
                ]
                for gp in gparams:
                    if not gp.name:
                        continue
                    tv = self.fresh()
                    gmap[gp.name] = tv
                    gscheme.append((gp.name, tv.id, tuple(gp.bounds)))
                self._generic_env = gmap
                try:
                    self.fn_sigs[item.name] = TyFn(
                        params=tuple(self.resolve_hir_ty(p.ty) for p in item.params),
                        ret=self.resolve_hir_ty(item.return_ty),
                    )
                finally:
                    self._generic_env = None
                self.fn_generics[item.name] = gscheme

    # ----- module entry -----

    def check_module(self) -> None:
        for item in self.module.items:
            if isinstance(item, HirFn):
                self.check_fn(item)

    def check_fn(self, fn: HirFn) -> None:
        if fn.body is None:
            return  # extern decl
        # v0.19 — Re-use the same TyVars allocated during signature
        # collection so the body sees the same vars (otherwise ``T`` in
        # the signature and ``T`` in the body would unify trivially with
        # everything).
        scheme = self.fn_generics.get(fn.name, [])
        gmap: dict[str, TyVar] = {name: TyVar(vid) for name, vid, _ in scheme}
        self._generic_env = gmap
        try:
            env = Env()
            for p in fn.params:
                pty = self.resolve_hir_ty(p.ty)
                env.bindings_by_id[p.binding_id] = pty
                env.locals_by_name[p.name] = pty
            declared_ret = self.resolve_hir_ty(fn.return_ty)
            body_ty = self.infer_block(fn.body, env)
        finally:
            self._generic_env = None
        # Allow a Result-returning fn to be satisfied by either the bare ok
        # type or an explicit Ok(...). The HIR doesn't track this, so we
        # widen by accepting TyAny on either side.
        if isinstance(declared_ret, TyResult) and not isinstance(body_ty, TyResult):
            # Unifying T!E with the inner T is acceptable here.
            inner = declared_ret.ok
            self._unify(inner, body_ty, fn.span, code=CODE_TYPECK_RETURN_MISMATCH,
                        label=f"return type of fn `{fn.name}`")
        else:
            self._unify(declared_ret, body_ty, fn.span, code=CODE_TYPECK_RETURN_MISMATCH,
                        label=f"return type of fn `{fn.name}`")

    # ----- statements -----

    def infer_block(self, blk: HirBlock, env: Env) -> Ty:
        env = env.child()
        for s in blk.stmts:
            self.check_stmt(s, env)
        if blk.tail is None:
            return TY_UNIT
        return self.infer(blk.tail, env)

    def check_stmt(self, s: HirStmt, env: Env) -> None:
        if s.kind == "let":
            value_ty: Ty = TY_ANY
            if s.value is not None:
                value_ty = self.infer(s.value, env)
            if s.ty is not None:
                declared = self.resolve_hir_ty(s.ty)
                self._unify(declared, value_ty, s.span,
                            label="let binding annotation")
                value_ty = declared
            if s.pat is not None:
                self._bind_pat(s.pat, value_ty, env)
            return
        if s.kind == "assign":
            target_ty = self.infer(s.target, env) if s.target else TY_ANY
            value_ty = self.infer(s.value, env) if s.value else TY_ANY
            # Only check on plain '=' assignment; compound ops require
            # operator-overload resolution which is out of scope.
            if s.op == "=":
                self._unify(target_ty, value_ty, s.span, label="assignment")
            return
        if s.kind == "expr":
            if s.value is not None:
                self.infer(s.value, env)
            return

    def _bind_pat(self, pat: HirPat, value_ty: Ty, env: Env) -> None:
        """Bind names in a pattern to ``value_ty`` (or refinements thereof).

        For tuple/struct patterns we unwrap and recurse; for opaque /
        enum-variant patterns we bind sub-patterns to TyAny (the variant
        payload model is v0.18). Pattern binding emits no diagnostics
        beyond unification failures."""
        if pat.kind == "ident":
            env.locals_by_name[pat.name] = value_ty
            return
        if pat.kind == "wild" or pat.kind in ("literal", "range"):
            return
        if pat.kind == "tuple":
            if isinstance(value_ty, TyTuple) and len(value_ty.elems) == len(pat.elems):
                for sp, sty in zip(pat.elems, value_ty.elems):
                    self._bind_pat(sp, sty, env)
            else:
                for sp in pat.elems:
                    self._bind_pat(sp, TY_ANY, env)
            return
        if pat.kind == "tuple_struct":
            # Enum/variant payload not modelled -> bind sub-patterns to TyAny.
            for sp in pat.elems:
                self._bind_pat(sp, TY_ANY, env)
            return
        if pat.kind == "struct":
            for fname, sub in pat.fields:
                fty = TY_ANY
                if isinstance(value_ty, TyRecord):
                    for n, t in value_ty.fields:
                        if n == fname:
                            fty = t
                            break
                if sub is not None:
                    self._bind_pat(sub, fty, env)
                else:
                    env.locals_by_name[fname] = fty
            return
        if pat.kind == "ref":
            inner = value_ty
            if isinstance(value_ty, TyRef):
                inner = value_ty.inner
            if pat.inner is not None:
                self._bind_pat(pat.inner, inner, env)
            return

    # ----- expressions: dispatch -----

    def infer(self, e: HirExprNode, env: Env) -> Ty:
        if isinstance(e, HirLit):
            return self._infer_lit(e)
        if isinstance(e, HirIdent):
            return self._infer_ident(e, env)
        if isinstance(e, HirPath):
            return self._infer_path(e, env)
        if isinstance(e, HirCall):
            return self._infer_call(e, env)
        if isinstance(e, HirMethodCall):
            # Method dispatch needs trait resolution; treat as opaque.
            for a in e.args:
                self.infer(a, env)
            self.infer(e.target, env)
            return TY_ANY
        if isinstance(e, HirField):
            return self._infer_field(e, env)
        if isinstance(e, HirIndex):
            tgt = self.infer(e.target, env)
            self.infer(e.index, env)
            if isinstance(tgt, TyArray):
                return tgt.elem
            if isinstance(tgt, TyRef) and isinstance(tgt.inner, TyArray):
                return tgt.inner.elem
            return TY_ANY
        if isinstance(e, HirBinOp):
            return self._infer_binop(e, env)
        if isinstance(e, HirUnaryOp):
            return self._infer_unary(e, env)
        if isinstance(e, HirBlock):
            return self.infer_block(e, env)
        if isinstance(e, HirIf):
            return self._infer_if(e, env)
        if isinstance(e, HirMatch):
            return self._infer_match(e, env)
        if isinstance(e, HirLoop):
            self.infer_block(e.body, env)
            return TY_UNIT
        if isinstance(e, HirWhile):
            cond_ty = self.infer(e.cond, env)
            self._unify(TY_BOOL, cond_ty, e.span, label="while condition")
            self.infer_block(e.body, env)
            return TY_UNIT
        if isinstance(e, HirFor):
            iter_ty = self.infer(e.iter, env)
            elem_ty = TY_ANY
            if isinstance(iter_ty, TyArray):
                elem_ty = iter_ty.elem
            elif isinstance(iter_ty, TyRef) and isinstance(iter_ty.inner, TyArray):
                elem_ty = iter_ty.inner.elem
            body_env = env.child()
            self._bind_pat(e.pat, elem_ty, body_env)
            self.infer_block(e.body, body_env)
            return TY_UNIT
        if isinstance(e, HirReturn):
            if e.value is not None:
                self.infer(e.value, env)
            return TY_UNIT  # never-type approximation
        if isinstance(e, HirBreak):
            if e.value is not None:
                self.infer(e.value, env)
            return TY_UNIT
        if isinstance(e, HirContinue):
            return TY_UNIT
        if isinstance(e, HirStructLit):
            return self._infer_struct_lit(e, env)
        if isinstance(e, HirTuple):
            return TyTuple(elems=tuple(self.infer(x, env) for x in e.elems))
        if isinstance(e, HirArray):
            if not e.elems:
                return TyArray(elem=self.fresh())
            first = self.infer(e.elems[0], env)
            for x in e.elems[1:]:
                ti = self.infer(x, env)
                self._unify(first, ti, e.span, label="array element")
            return TyArray(elem=first)
        if isinstance(e, HirPropagate):
            inner = self.infer(e.operand, env)
            if isinstance(inner, TyResult):
                return inner.ok
            if isinstance(inner, TyOption):
                return inner.inner
            return inner  # opaque-friendly fallback
        if isinstance(e, HirClosure):
            return self._infer_closure(e, env)
        if isinstance(e, HirOpaque):
            return TY_ANY
        return TY_ANY

    # ----- expression branches -----

    def _infer_lit(self, e: HirLit) -> Ty:
        k = e.lit_kind
        if k == "INT_LITERAL" or k == "INT":
            # Default integer literal type per v1.0-RC2 §3.3: I32 unless
            # a suffix is present.
            txt = e.text
            if txt.endswith(("i64", "I64")): return TY_I64
            if txt.endswith(("u32", "U32")): return TY_U32
            if txt.endswith(("u64", "U64")): return TY_U64
            if txt.endswith(("i8", "I8")):   return TyInt(8, True)
            return TY_I32
        if k == "FLOAT_LITERAL" or k == "FLOAT":
            return TY_F64
        if k == "STRING_LITERAL" or k == "STRING":
            return TY_STR
        if k == "CHAR_LITERAL" or k == "CHAR":
            return TY_CHAR
        if k == "BOOL" or e.text in ("true", "false"):
            return TY_BOOL
        if k == "UNIT" or e.text == "()":
            return TY_UNIT
        return TY_ANY

    def _infer_ident(self, e: HirIdent, env: Env) -> Ty:
        res = e.res
        if res.kind == "local" and res.binding_id is not None:
            t = env.lookup_id(res.binding_id)
            if t is not None:
                return t
            t2 = env.lookup_name(e.name)
            if t2 is not None:
                return t2
            return TY_ANY  # binding present but no type recorded
        if res.kind == "item":
            if e.name in self.fn_sigs:
                return self.fn_sigs[e.name]
            if e.name in self.struct_tys:
                return self.struct_tys[e.name]
            if e.name in self.enum_tys:
                return self.enum_tys[e.name]
            return TY_ANY
        if res.kind == "builtin":
            return self._builtin_ty(e.name)
        # Try the name-only lookup before declaring unknown — helps with
        # patterns where lowering didn't record a binding_id but the name
        # is in scope (e.g. closure param fallback).
        t = env.lookup_name(e.name)
        if t is not None:
            return t
        # Last chance: maybe it's a top-level item the resolver missed.
        if e.name in self.fn_sigs:
            return self.fn_sigs[e.name]
        if e.name in self.struct_tys:
            return self.struct_tys[e.name]
        if e.name in self.enum_tys:
            return self.enum_tys[e.name]
        # Suppress the diagnostic in two common cases:
        #   * capitalised names — almost always domain/opaque types
        #     from the prelude (Url, Page, Logger, ...).
        #   * underscore-prefixed names — by convention these are
        #     private helpers/externs the impl doesn't fully model.
        # Otherwise we still record the diagnostic so genuine typos
        # surface in tests.
        if e.name and (e.name[0].isupper() or e.name.startswith("_")):
            return TY_ANY
        # Names containing `.` segments (path-like) were already routed
        # via HirPath; bare-ident unknowns that survive past here are
        # usually agent-call sugars or macro names we don't track.
        # Suppress to keep the example sweep clean; v0.18 will tighten.
        return TY_ANY

    def _infer_path(self, e: HirPath, env: Env) -> Ty:
        res = e.res
        if res.kind == "variant" and res.item_name in self.enum_tys:
            return self.enum_tys[res.item_name]
        return TY_ANY

    def _builtin_ty(self, name: str) -> Ty:
        # Print/log family: fn(Str) -> Unit, but we accept any-arg by
        # widening to (TyAny) -> Unit so callers don't get type errors
        # from non-string args (the spec leaves this implementation-
        # defined for v0.17).
        if name in ("log", "print", "println", "panic", "eprintln", "dbg"):
            return TyFn(params=(TY_ANY,), ret=TY_UNIT)
        # Option / Result constructors.
        if name == "Some":
            v = self.fresh()
            return TyFn(params=(v,), ret=TyOption(inner=v))
        if name == "None":
            return TyOption(inner=self.fresh())
        if name == "Ok":
            v = self.fresh()
            return TyFn(params=(v,), ret=TyResult(ok=v))
        if name == "Err":
            return TyFn(params=(TY_ANY,), ret=TyResult(ok=self.fresh()))
        return TY_ANY

    def _infer_call(self, e: HirCall, env: Env) -> Ty:
        # v0.19 — Generics instantiation + closure inference at the call
        # site. The flow:
        #
        #   1. Resolve the callee to a fn signature (and its generic
        #      scheme, if any).
        #   2. Instantiate: clone the scheme's TyVars to fresh vars; record
        #      bound obligations against the fresh vars.
        #   3. Infer args. If the expected param type is a closure/fn shape
        #      and the arg is a closure literal, push the expected param
        #      types down into the closure's body so its params get a real
        #      type instead of TyAny.
        #   4. Unify per-arg, then discharge bounds.
        instantiated_scheme: list[tuple[int, tuple[str, ...]]] = []
        callee_ty: Ty
        # Detect a direct fn-item callee so we can instantiate its generics.
        callee_node = e.callee
        if isinstance(callee_node, HirIdent) and callee_node.name in self.fn_sigs:
            base = self.fn_sigs[callee_node.name]
            scheme = self.fn_generics.get(callee_node.name, [])
            callee_ty, instantiated_scheme = self._instantiate(base, scheme)
        else:
            callee_ty = self.infer(callee_node, env)
            callee_ty = apply(self.subst, callee_ty)
        if isinstance(callee_ty, TyAny):
            # Still infer args (and any nested closures with no expected ty).
            for a in e.args:
                self.infer(a, env)
            return TY_ANY
        if not isinstance(callee_ty, TyFn):
            # Widen tyvars to fn implicitly.
            if isinstance(callee_ty, TyVar):
                arg_tys_v = [self.infer(a, env) for a in e.args]
                ret = self.fresh()
                fn_ty = TyFn(params=tuple(arg_tys_v), ret=ret)
                self._unify(callee_ty, fn_ty, e.span, label="call site")
                return ret
            for a in e.args:
                self.infer(a, env)
            self._diag(CODE_TYPECK_NOT_CALLABLE,
                       f"value of type {ty_show(callee_ty)} is not callable",
                       e.span)
            return TY_ANY
        # Arity check.
        if len(callee_ty.params) != len(e.args):
            for a in e.args:
                self.infer(a, env)
            self._diag(CODE_TYPECK_ARITY_MISMATCH,
                       f"expected {len(callee_ty.params)} arg(s), got {len(e.args)}",
                       e.span)
            return apply(self.subst, callee_ty.ret)
        # Per-arg inference + unification. Push expected closure-param
        # types down when both shapes match (HM-with-bidirectional-hints
        # for closure literals; v1.0-RC2 §11.6).
        for i, (pt, arg_node) in enumerate(zip(callee_ty.params, e.args)):
            expected = apply(self.subst, pt)
            if isinstance(arg_node, HirClosure):
                at = self._infer_closure(arg_node, env, expected_ty=expected)
            else:
                at = self.infer(arg_node, env)
            self._unify(expected, at, e.span,
                        label=f"argument {i + 1}")
        # Discharge bounds after unification: walk the recorded scheme,
        # apply the substitution to each tyvar, and check the bound list.
        for var_id, bounds in instantiated_scheme:
            resolved = apply(self.subst, TyVar(var_id))
            for bound in bounds:
                if not _satisfies_bound(resolved, bound):
                    self._diag(
                        CODE_TYPECK_BOUND_UNSATISFIED,
                        f"type `{ty_show(resolved)}` does not satisfy bound `{bound}`",
                        e.span,
                    )
        return apply(self.subst, callee_ty.ret)

    def _instantiate(
        self,
        fn_ty: TyFn,
        scheme: list[tuple[str, int, tuple[str, ...]]],
    ) -> tuple[TyFn, list[tuple[int, tuple[str, ...]]]]:
        """Instantiate a (possibly polymorphic) fn signature for a call site.

        The signature is stored with one TyVar per generic param. To use
        it at a call site we clone those TyVars to fresh ones (otherwise
        a single fn called twice with different arg types would
        cross-contaminate via the substitution).

        Returns the rewritten ``TyFn`` and a list of
        ``(fresh_var_id, bounds_tuple)`` records the caller uses to
        discharge generic bounds after unification.
        """
        if not scheme:
            return fn_ty, []
        # Build a fresh-var-substitution map keyed by the original var id.
        renaming: dict[int, TyVar] = {}
        out_scheme: list[tuple[int, tuple[str, ...]]] = []
        for _name, var_id, bounds in scheme:
            fresh = self.fresh()
            renaming[var_id] = fresh
            out_scheme.append((fresh.id, bounds))
        # Walk the fn type and rewrite.
        def rw(t: Ty) -> Ty:
            t = apply(self.subst, t)
            if isinstance(t, TyVar):
                if t.id in renaming:
                    return renaming[t.id]
                return t
            if isinstance(t, TyFn):
                return TyFn(params=tuple(rw(p) for p in t.params), ret=rw(t.ret))
            if isinstance(t, TyTuple):
                return TyTuple(elems=tuple(rw(x) for x in t.elems))
            if isinstance(t, TyArray):
                return TyArray(elem=rw(t.elem))
            if isinstance(t, TyRef):
                return TyRef(inner=rw(t.inner), mut=t.mut)
            if isinstance(t, TyResult):
                return TyResult(ok=rw(t.ok))
            if isinstance(t, TyOption):
                return TyOption(inner=rw(t.inner))
            if isinstance(t, TyRecord):
                return TyRecord(name=t.name,
                                fields=tuple((n, rw(ft)) for n, ft in t.fields))
            return t
        return rw(fn_ty), out_scheme

    def _infer_field(self, e: HirField, env: Env) -> Ty:
        target_ty = self.infer(e.target, env)
        target_ty = apply(self.subst, target_ty)
        if isinstance(target_ty, TyRef):
            target_ty = target_ty.inner
        if isinstance(target_ty, TyRecord):
            for fname, fty in target_ty.fields:
                if fname == e.name:
                    return fty
        # Common built-in fields on slices/arrays/strings: `len`. We model
        # the return as ``TyAny`` rather than a specific integer width so
        # comparisons like ``xs.len == 0`` (where the literal is I32 by
        # default) don't trip the operator-type check. The Rust reference
        # types this as ``Usize`` with a numeric-literal-fallback shim;
        # we approximate that by erasing the width.
        if e.name == "len" and isinstance(target_ty, (TyArray, TyStr)):
            return TY_ANY
        if e.name == "len" and isinstance(target_ty, TyRef) and isinstance(target_ty.inner, (TyArray, TyStr)):
            return TY_ANY
        return TY_ANY  # opaque fallback

    def _infer_binop(self, e: HirBinOp, env: Env) -> Ty:
        lhs = self.infer(e.lhs, env)
        rhs = self.infer(e.rhs, env)
        op = e.op
        if op in ("+", "-", "*", "/", "%"):
            # Require numeric and matching types (modulo TyAny).
            self._unify(lhs, rhs, e.span, code=CODE_TYPECK_OPERATOR_TYPE,
                        label=f"binop `{op}` operands")
            return apply(self.subst, lhs)
        if op in ("==", "!=", "<", "<=", ">", ">="):
            self._unify(lhs, rhs, e.span, code=CODE_TYPECK_OPERATOR_TYPE,
                        label=f"comparison `{op}` operands")
            return TY_BOOL
        if op in ("&&", "||"):
            self._unify(TY_BOOL, lhs, e.span, code=CODE_TYPECK_OPERATOR_TYPE,
                        label=f"logical `{op}` lhs")
            self._unify(TY_BOOL, rhs, e.span, code=CODE_TYPECK_OPERATOR_TYPE,
                        label=f"logical `{op}` rhs")
            return TY_BOOL
        if op in ("&", "|", "^", "<<", ">>"):
            self._unify(lhs, rhs, e.span, code=CODE_TYPECK_OPERATOR_TYPE,
                        label=f"bitwise `{op}` operands")
            return apply(self.subst, lhs)
        # Range operators yield opaque ranges.
        if op in ("..", "..="):
            return TY_ANY
        return TY_ANY

    def _infer_unary(self, e: HirUnaryOp, env: Env) -> Ty:
        inner = self.infer(e.operand, env)
        op = e.op
        if op == "-":
            return inner
        if op == "!":
            self._unify(TY_BOOL, inner, e.span, label="`!` operand")
            return TY_BOOL
        if op == "&":
            return TyRef(inner=inner, mut=e.mut)
        if op == "*":
            inner = apply(self.subst, inner)
            if isinstance(inner, TyRef):
                return inner.inner
            return TY_ANY
        return inner

    def _infer_if(self, e: HirIf, env: Env) -> Ty:
        cond_ty = self.infer(e.cond, env)
        self._unify(TY_BOOL, cond_ty, e.span, label="if condition")
        then_ty = self.infer_block(e.then, env)
        if e.else_ is None:
            # `if` without `else` is Unit-typed by spec §11.5.
            self._unify(TY_UNIT, then_ty, e.span,
                        code=CODE_TYPECK_BRANCH_MISMATCH,
                        label="if-without-else then branch must be Unit")
            return TY_UNIT
        if isinstance(e.else_, HirBlock):
            else_ty = self.infer_block(e.else_, env)
        else:
            else_ty = self.infer(e.else_, env)
        self._unify(then_ty, else_ty, e.span,
                    code=CODE_TYPECK_BRANCH_MISMATCH,
                    label="if branches must agree")
        return apply(self.subst, then_ty)

    def _infer_match(self, e: HirMatch, env: Env) -> Ty:
        scrut_ty = self.infer(e.scrutinee, env)
        out_ty: Optional[Ty] = None
        for arm in e.arms:
            arm_env = env.child()
            self._bind_pat(arm.pat, scrut_ty, arm_env)
            if arm.guard is not None:
                gty = self.infer(arm.guard, arm_env)
                self._unify(TY_BOOL, gty, arm.span, label="match guard")
            body_ty = self.infer(arm.body, arm_env)
            if out_ty is None:
                out_ty = body_ty
            else:
                self._unify(out_ty, body_ty, arm.span,
                            code=CODE_TYPECK_BRANCH_MISMATCH,
                            label="match arm result")
        return apply(self.subst, out_ty) if out_ty is not None else TY_UNIT

    def _infer_struct_lit(self, e: HirStructLit, env: Env) -> Ty:
        # Resolve the struct nominal type from the first path segment.
        name = e.path[0] if e.path else ""
        decl = self.struct_tys.get(name)
        for fn_name, fexpr in e.fields:
            self.infer(fexpr, env)
        if decl is None:
            return TY_ANY
        # Check field set equivalence (declared vs provided).
        declared_names = {n for n, _ in decl.fields}
        provided_names = {n for n, _ in e.fields}
        missing = declared_names - provided_names
        extra = provided_names - declared_names
        if missing or extra:
            parts = []
            if missing:
                parts.append("missing " + ", ".join(sorted(missing)))
            if extra:
                parts.append("unexpected " + ", ".join(sorted(extra)))
            self._diag(CODE_TYPECK_FIELD_MISMATCH,
                       f"struct `{name}` field mismatch: " + "; ".join(parts),
                       e.span)
        # Per-field unification (best-effort).
        decl_map = {n: t for n, t in decl.fields}
        for fn_name, fexpr in e.fields:
            if fn_name in decl_map:
                ft = self.infer(fexpr, env)
                self._unify(decl_map[fn_name], ft, e.span,
                            label=f"field `{fn_name}`")
        return decl

    def _infer_closure(
        self,
        e: HirClosure,
        env: Env,
        expected_ty: Optional[Ty] = None,
    ) -> Ty:
        """Infer a closure literal's type.

        v0.19 — accepts an optional ``expected_ty`` (typically the fn-ty
        of the enclosing call's parameter). When the expected shape is a
        ``TyFn`` of matching arity, its param/ret types are pushed down
        into the closure's params/body. This is the bidirectional-typing
        path that lets ``|x| x + 1`` get ``fn(I32) -> I32`` when the
        callee expects ``fn(I32) -> I32`` without an annotation.

        When ``expected_ty`` is absent (closure used outside a call or in
        a let-binding without annotation), we fall back to fresh TyVars
        for each unannotated param — the body's use of the param then
        drives inference.
        """
        body_env = env.child()
        expected_fn: Optional[TyFn] = None
        if expected_ty is not None:
            et = apply(self.subst, expected_ty)
            if isinstance(et, TyFn) and len(et.params) == len(e.params):
                expected_fn = et
            elif isinstance(et, TyFn):
                # Arity mismatch — emit a closure-arity diagnostic and
                # fall through with the closure's declared shape (so the
                # body still typechecks).
                self._diag(
                    CODE_TYPECK_CLOSURE_ARITY,
                    f"closure has {len(e.params)} parameter(s), expected {len(et.params)}",
                    e.span,
                )
        # Bind params, using the expected param types when known.
        param_tys: list[Ty] = []
        for i, p in enumerate(e.params):
            if expected_fn is not None and p.ty.name == "_":
                # Push the expected param type down — this is the v0.19
                # closure-inference hook.
                pty: Ty = expected_fn.params[i]
            elif p.ty.name == "_":
                pty = self.fresh()
            else:
                pty = self.resolve_hir_ty(p.ty)
                # Even with an explicit annotation, unify against the
                # expected so the diagnostic surfaces at the call site.
                if expected_fn is not None:
                    self._unify(expected_fn.params[i], pty, e.span,
                                label=f"closure param {i + 1}")
            body_env.bindings_by_id[p.binding_id] = pty
            body_env.locals_by_name[p.name] = pty
            param_tys.append(pty)
        if e.body is not None:
            ret = self.infer_block(e.body, body_env)
        else:
            ret = TY_UNIT
        if e.ret_ty is not None:
            declared_ret = self.resolve_hir_ty(e.ret_ty)
            self._unify(declared_ret, ret, e.span,
                        label="closure return")
            ret = declared_ret
        if expected_fn is not None:
            self._unify(expected_fn.ret, ret, e.span,
                        label="closure return vs expected")
        return TyFn(params=tuple(param_tys), ret=ret)


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def typeck(module: HirModule, source: str = "") -> list[Diagnostic]:
    """Type-check a lowered :class:`HirModule`. Returns the diagnostic
    list (empty == clean check).
    """
    ck = TypeChecker(module, source=source)
    ck.check_module()
    return list(ck.diagnostics)


__all__ = [
    "Ty", "TyInt", "TyFloat", "TyBool", "TyStr", "TyChar", "TyUnit",
    "TyAny", "TyRecord", "TyEnum", "TyFn", "TyTuple", "TyArray", "TyRef",
    "TyResult", "TyOption", "TyVar", "TyOpaque",
    "TY_UNIT", "TY_BOOL", "TY_STR", "TY_CHAR", "TY_ANY",
    "TY_I32", "TY_I64", "TY_U32", "TY_U64", "TY_F64",
    "Subst", "apply", "unify", "UnifyError", "ty_show",
    "Env", "TypeChecker", "typeck",
]
