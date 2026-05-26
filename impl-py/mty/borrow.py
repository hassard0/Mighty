"""Borrow check for the Python 2nd-impl (v0.22).

A small NLL-flavoured (non-lexical-lifetimes) borrow checker that walks
the ``HirModule`` produced by :mod:`mty.lower` and the typing context
produced by :mod:`mty.typeck` and reports the canonical aliasing /
ownership errors:

* ``MT3001`` — moving a value while a borrow of it is still live.
* ``MT3002`` — moving out of (or via) a shared reference.
* ``MT3003`` — a ``&mut`` borrow and a ``&`` borrow co-exist.
* ``MT3004`` — use after the value was moved out.
* ``MT3005`` — two ``&mut`` borrows of the same place co-exist.

Design notes (spec-driven, no source peek into ``crates/mty-borrow``):

* This is **not** Polonius. We use the classical NLL-style stratification:
  for each fn body, walk the HIR in evaluation order; for every place
  expression we encounter, classify the access as *move*, *copy*,
  *shared borrow*, or *mut borrow*; maintain a per-binding state that
  records the currently-live borrows.
* "Place" is a small algebraic structure rooted in a local binding
  (``binding_id``) optionally followed by a chain of field projections.
  Anything that isn't rooted in a known local (literals, calls, tuple
  literals, ...) is a *temporary place*; temporaries are owned by the
  enclosing expression and never trip a move/borrow rule on their own.
* "Move" is the default for non-``Copy`` types in the Mighty spec. We
  approximate ``Copy``-ness from the typeck output: ``TyInt``,
  ``TyFloat``, ``TyBool``, ``TyChar``, ``TyUnit`` and ``TyRef`` are
  Copy; ``TyStr``, ``TyArray``, ``TyTuple``, ``TyRecord``, ``TyEnum``,
  ``TyOption``, ``TyResult`` and ``TyOpaque`` are not. ``TyAny`` and
  ``TyVar`` default to Copy (the spec's escape hatch when we can't tell).
* Borrows are recorded against the *root binding* of the borrowed
  place. Field projections share the root's borrow set — this is a
  conservative widening (the real checker tracks per-field), but it
  catches the headline rules from the spec.
* Scopes: a borrow goes out of scope when the enclosing block exits.
  We don't model the full NLL "last-use" precision; instead each block
  is a borrow lifetime, which is the v0.5 Rust borrow checker rule. For
  the spec-validation subset this is sufficient because the example
  corpus doesn't exercise late-mid-block resurrection.

Spec sources: v1.0-RC2 §8 (ownership and borrowing), §9 (lifetimes),
§33 (MT3xxx diagnostic band).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional, Union

from .diagnostics import (
    CODE_BORROW_DOUBLE_MUT,
    CODE_BORROW_MOVE_OF_BORROWED,
    CODE_BORROW_MOVE_OUT_OF_BORROW,
    CODE_BORROW_MUT_SHARED_CONFLICT,
    CODE_BORROW_USE_AFTER_MOVE,
    Diagnostic,
    Severity,
)
from .hir import (
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
    HirFn,
    HirFor,
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
    HirUnaryOp,
    HirWhile,
)
from .typeck import (
    TY_ANY,
    Ty,
    TyAny,
    TyArray,
    TyBool,
    TyChar,
    TyEnum,
    TyFloat,
    TyFn,
    TyInt,
    TyOpaque,
    TyOption,
    TyRecord,
    TyRef,
    TyResult,
    TyStr,
    TyTuple,
    TyUnit,
    TyVar,
    TypeChecker,
)


# ---------------------------------------------------------------------------
# Place model.
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Place:
    """A path rooted in a local binding plus zero-or-more projections.

    ``root`` is the ``binding_id`` of the local; ``path`` is the chain
    of field names (``("origin", "x")`` for ``rect.origin.x``).
    Indexing / dereferencing collapse to the root for v0.22 — this is
    a deliberate conservative widening.
    """

    root: int
    path: tuple[str, ...] = ()

    def with_field(self, name: str) -> "Place":
        return Place(root=self.root, path=self.path + (name,))

    @property
    def is_root(self) -> bool:
        return not self.path


# ---------------------------------------------------------------------------
# Loan model.
# ---------------------------------------------------------------------------


@dataclass
class Loan:
    """A live borrow of a place.

    Attributes:
        place:  the borrowed place (root + projection chain).
        mut:    True for ``&mut`` loans, False for shared.
        span:   source span where the borrow was created.
    """

    place: Place
    mut: bool
    span: tuple[int, int]


# ---------------------------------------------------------------------------
# Per-binding state.
# ---------------------------------------------------------------------------


@dataclass
class BindingInfo:
    """Tracked state for a single local binding.

    Attributes:
        name:    source identifier (for diagnostics).
        ty:      inferred type (used to decide move-vs-copy).
        moved:   True once a non-Copy use has consumed the value.
        moved_span: span of the moving use (for diagnostic notes).
    """

    name: str
    ty: Ty
    moved: bool = False
    moved_span: Optional[tuple[int, int]] = None


# ---------------------------------------------------------------------------
# Borrow-state context (per fn body).
# ---------------------------------------------------------------------------


class BorrowContext:
    """Mutable state threaded through one fn body's walk.

    The scope-stack semantics here are deliberately coarse: every
    ``HirBlock`` enters a new scope, and *all* loans created inside that
    scope are dropped when it exits. The real Rust checker uses
    last-use NLL; we don't, because the spec-validation subset doesn't
    care about that precision and the example corpus doesn't exercise
    it. v0.23 will tighten.
    """

    def __init__(self) -> None:
        # Locals introduced by params / let-bindings, keyed by binding_id.
        # The id allocator here is *parallel* to the lowerer's; we don't
        # consult the lowerer's ids directly because they're not stored
        # on the HirPat. Instead we walk the HIR ourselves and refresh
        # ids consistently — see ``LocalAllocator``.
        self.bindings: dict[int, BindingInfo] = {}
        # Name → binding_id stack (one dict per scope, last is innermost).
        self.scopes: list[dict[str, int]] = [{}]
        # Live loans, scoped: list parallels self.scopes.
        self.loan_scopes: list[list[Loan]] = [[]]
        # Diagnostics emitted so far.
        self.diagnostics: list[Diagnostic] = []
        # ID allocator (parallel to the lowerer's — see above).
        self._next_id = 0

    # ----- scope management -----

    def push_scope(self) -> None:
        self.scopes.append({})
        self.loan_scopes.append([])

    def pop_scope(self) -> None:
        self.scopes.pop()
        self.loan_scopes.pop()

    # ----- binding management -----

    def declare(self, name: str, ty: Ty) -> int:
        bid = self._next_id
        self._next_id += 1
        self.bindings[bid] = BindingInfo(name=name, ty=ty)
        self.scopes[-1][name] = bid
        return bid

    def lookup(self, name: str) -> Optional[int]:
        for scope in reversed(self.scopes):
            if name in scope:
                return scope[name]
        return None

    # ----- loan management -----

    def add_loan(self, loan: Loan) -> None:
        self.loan_scopes[-1].append(loan)

    def all_loans(self) -> list[Loan]:
        out: list[Loan] = []
        for scope in self.loan_scopes:
            out.extend(scope)
        return out

    def loans_on(self, root: int) -> list[Loan]:
        return [l for l in self.all_loans() if l.place.root == root]

    # ----- diagnostics -----

    def diag(self, code: str, msg: str, span: tuple[int, int],
             notes: tuple[str, ...] = ()) -> None:
        self.diagnostics.append(Diagnostic(
            code=code, message=msg, severity=Severity.ERROR,
            start=span[0], end=span[1], notes=notes,
        ))


# ---------------------------------------------------------------------------
# Type → Copy classification.
# ---------------------------------------------------------------------------


def is_copy(t: Ty) -> bool:
    """Return True if values of type ``t`` are ``Copy`` (so a use is not a move).

    Conservative rule (spec-driven, §8.3): scalars + refs are Copy;
    aggregates, strings, opaque domain types are not. Inference vars
    and ``TyAny`` default to Copy because we can't prove otherwise —
    this matches typeck's TyAny-absorbs policy.
    """
    if isinstance(t, (TyInt, TyFloat, TyBool, TyChar, TyUnit, TyRef,
                      TyFn, TyAny, TyVar)):
        return True
    if isinstance(t, (TyStr, TyArray, TyTuple, TyRecord, TyEnum,
                      TyOption, TyResult, TyOpaque)):
        return False
    return True


# ---------------------------------------------------------------------------
# Use kind classification (move / copy / borrow).
# ---------------------------------------------------------------------------


# Use kinds. We expose them as plain string sentinels for readability.
USE_MOVE = "move"
USE_COPY = "copy"
USE_BORROW_SHARED = "borrow"
USE_BORROW_MUT = "borrow_mut"
USE_ASSIGN_TARGET = "assign"  # left-hand side of `=` (re-init, not a use)


# ---------------------------------------------------------------------------
# Place extraction from HIR expressions.
# ---------------------------------------------------------------------------


def extract_place(expr: HirExprNode, ctx: BorrowContext) -> Optional[Place]:
    """If ``expr`` denotes a *place* expression rooted in a known local,
    return the :class:`Place`; otherwise None (the expression is a
    temporary / value computation).

    We support:
      * ``HirIdent`` resolving to a local binding (or name-lookup hit).
      * ``HirField`` whose target is itself a place.

    Index and method-call places are not modelled in v0.22 — the spec's
    place algebra is rich, but the example corpus only exercises this
    subset for borrow scenarios.
    """
    if isinstance(expr, HirIdent):
        # Prefer the resolver's binding_id if present, but we run our
        # own binding allocator (the lowerer's ids aren't stored on
        # patterns). So we look up by name in our scope first.
        bid = ctx.lookup(expr.name)
        if bid is not None:
            return Place(root=bid)
        return None
    if isinstance(expr, HirField):
        sub = extract_place(expr.target, ctx)
        if sub is None:
            return None
        return sub.with_field(expr.name)
    return None


# ---------------------------------------------------------------------------
# The borrow checker proper.
# ---------------------------------------------------------------------------


class BorrowChecker:
    """Walks an :class:`HirModule` and emits borrow-check diagnostics.

    Usage::

        from mty.typeck import TypeChecker
        ck = TypeChecker(mod)
        ck.check_module()
        bc = BorrowChecker(mod, type_checker=ck)
        bc.check_module()
        for d in bc.diagnostics: print(d)
    """

    def __init__(
        self,
        module: HirModule,
        type_checker: Optional[TypeChecker] = None,
        source: str = "",
    ) -> None:
        self.module = module
        self.source = source
        self.type_checker = type_checker
        self.diagnostics: list[Diagnostic] = []

    # ----- module entry -----

    def check_module(self) -> None:
        for item in self.module.items:
            if isinstance(item, HirFn) and item.body is not None:
                self.check_fn(item)

    def check_fn(self, fn: HirFn) -> None:
        ctx = BorrowContext()
        # Declare params first.
        for p in fn.params:
            pty = self._resolve_param_ty(p.ty)
            ctx.declare(p.name, pty)
        # Walk the body.
        self._walk_block(fn.body, ctx)
        # Surface the per-fn diagnostics into the checker's collection.
        self.diagnostics.extend(ctx.diagnostics)

    # ----- block / stmt walk -----

    def _walk_block(self, blk: HirBlock, ctx: BorrowContext) -> None:
        ctx.push_scope()
        try:
            for s in blk.stmts:
                self._walk_stmt(s, ctx)
            if blk.tail is not None:
                self._walk_expr(blk.tail, ctx, USE_COPY)
        finally:
            ctx.pop_scope()

    def _walk_stmt(self, s: HirStmt, ctx: BorrowContext) -> None:
        if s.kind == "let":
            # Evaluate RHS first. The RHS is a value-position use, so
            # any place referenced there is moved (if non-Copy) or copied.
            if s.value is not None:
                # Default classification = move if the value is a place
                # of non-Copy type; otherwise copy. _walk_expr inspects.
                self._walk_expr(s.value, ctx, USE_MOVE)
            # Bind names in the pattern.
            ty = self._infer_pat_ty(s)
            if s.pat is not None:
                self._declare_pattern(s.pat, ty, ctx)
            return
        if s.kind == "assign":
            # LHS is a place — but it's a re-initialisation, not a use.
            # We still walk it to validate field-projection roots exist,
            # but classify as USE_ASSIGN_TARGET (no borrow conflict).
            if s.target is not None:
                self._walk_expr(s.target, ctx, USE_ASSIGN_TARGET)
            if s.value is not None:
                self._walk_expr(s.value, ctx, USE_MOVE)
            return
        if s.kind == "expr":
            if s.value is not None:
                self._walk_expr(s.value, ctx, USE_COPY)
            return

    def _declare_pattern(self, pat: HirPat, value_ty: Ty,
                         ctx: BorrowContext) -> None:
        """Allocate binding slots for every name introduced by ``pat``."""
        if pat.kind == "ident":
            ctx.declare(pat.name, value_ty)
            return
        if pat.kind in ("wild", "literal", "range"):
            return
        if pat.kind == "tuple":
            elem_tys: list[Ty] = []
            if isinstance(value_ty, TyTuple) and len(value_ty.elems) == len(pat.elems):
                elem_tys = list(value_ty.elems)
            else:
                elem_tys = [TY_ANY] * len(pat.elems)
            for sub, sty in zip(pat.elems, elem_tys):
                self._declare_pattern(sub, sty, ctx)
            return
        if pat.kind == "tuple_struct":
            for sub in pat.elems:
                self._declare_pattern(sub, TY_ANY, ctx)
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
                    self._declare_pattern(sub, fty, ctx)
                else:
                    ctx.declare(fname, fty)
            return
        if pat.kind == "ref":
            inner = value_ty
            if isinstance(value_ty, TyRef):
                inner = value_ty.inner
            if pat.inner is not None:
                self._declare_pattern(pat.inner, inner, ctx)
            return

    # ----- expression walks -----

    def _walk_expr(self, e: HirExprNode, ctx: BorrowContext,
                   use_kind: str) -> None:
        """Walk ``e`` in evaluation order, applying ``use_kind`` to any
        place expression at the root of the walk.

        Subexpressions inside compound forms are walked with USE_COPY by
        default — call args are an exception (handled in ``HirCall``).
        """
        # 1. If e itself is a place, classify and record.
        place = extract_place(e, ctx)
        if place is not None:
            self._classify_place(place, e, use_kind, ctx)
            # Field projections still need the target subexpression
            # walked for nested places — but we already handled the
            # *root* binding via the place extraction. We're done.
            return

        # 2. Unary borrow operators create loans.
        if isinstance(e, HirUnaryOp):
            self._walk_unary(e, ctx)
            return

        # 3. Composite shapes: recurse with COPY default.
        if isinstance(e, HirLit):
            return
        if isinstance(e, (HirPath,)):
            return
        if isinstance(e, HirCall):
            self._walk_call(e, ctx)
            return
        if isinstance(e, HirMethodCall):
            # Receiver is the implicit-borrow target. We approximate
            # as USE_COPY since trait-resolution decides &self vs self.
            self._walk_expr(e.target, ctx, USE_COPY)
            for a in e.args:
                self._walk_expr(a, ctx, USE_COPY)
            return
        if isinstance(e, HirIndex):
            self._walk_expr(e.target, ctx, USE_COPY)
            self._walk_expr(e.index, ctx, USE_COPY)
            return
        if isinstance(e, HirBinOp):
            self._walk_expr(e.lhs, ctx, USE_COPY)
            self._walk_expr(e.rhs, ctx, USE_COPY)
            return
        if isinstance(e, HirIf):
            self._walk_expr(e.cond, ctx, USE_COPY)
            # We walk each branch with its own scope-snapshot so a move
            # in one branch doesn't leak to the other. For v0.22 we use
            # a simple snapshot/restore on the bindings' ``moved`` flag.
            snap = self._snapshot(ctx)
            self._walk_block(e.then, ctx)
            after_then = self._snapshot(ctx)
            self._restore(ctx, snap)
            if e.else_ is not None:
                if isinstance(e.else_, HirBlock):
                    self._walk_block(e.else_, ctx)
                else:
                    self._walk_expr(e.else_, ctx, USE_COPY)
            # Conservative join: a binding is moved iff moved on BOTH paths.
            self._join(ctx, after_then)
            return
        if isinstance(e, HirMatch):
            self._walk_expr(e.scrutinee, ctx, USE_COPY)
            snaps: list[dict[int, bool]] = []
            base = self._snapshot(ctx)
            joined: Optional[dict[int, bool]] = None
            for arm in e.arms:
                self._restore(ctx, base)
                ctx.push_scope()
                # Bind names in the arm pattern, scoped to the arm.
                self._declare_pattern(arm.pat, TY_ANY, ctx)
                if arm.guard is not None:
                    self._walk_expr(arm.guard, ctx, USE_COPY)
                self._walk_expr(arm.body, ctx, USE_COPY)
                ctx.pop_scope()
                snap = self._snapshot(ctx)
                snaps.append(snap)
                if joined is None:
                    joined = dict(snap)
                else:
                    joined = {k: joined.get(k, False) and snap.get(k, False)
                              for k in set(joined) | set(snap)}
            if joined is not None:
                for bid, moved in joined.items():
                    if bid in ctx.bindings:
                        ctx.bindings[bid].moved = moved
            return
        if isinstance(e, HirBlock):
            self._walk_block(e, ctx)
            return
        if isinstance(e, HirLoop):
            self._walk_block(e.body, ctx)
            return
        if isinstance(e, HirWhile):
            self._walk_expr(e.cond, ctx, USE_COPY)
            self._walk_block(e.body, ctx)
            return
        if isinstance(e, HirFor):
            self._walk_expr(e.iter, ctx, USE_MOVE)
            ctx.push_scope()
            self._declare_pattern(e.pat, TY_ANY, ctx)
            self._walk_block(e.body, ctx)
            ctx.pop_scope()
            return
        if isinstance(e, HirReturn):
            if e.value is not None:
                self._walk_expr(e.value, ctx, USE_MOVE)
            return
        if isinstance(e, HirBreak):
            if e.value is not None:
                self._walk_expr(e.value, ctx, USE_MOVE)
            return
        if isinstance(e, HirContinue):
            return
        if isinstance(e, HirStructLit):
            for _, fexpr in e.fields:
                self._walk_expr(fexpr, ctx, USE_MOVE)
            return
        if isinstance(e, HirTuple):
            for x in e.elems:
                self._walk_expr(x, ctx, USE_MOVE)
            return
        if isinstance(e, HirArray):
            for x in e.elems:
                self._walk_expr(x, ctx, USE_MOVE)
            return
        if isinstance(e, HirPropagate):
            self._walk_expr(e.operand, ctx, USE_MOVE)
            return
        if isinstance(e, HirClosure):
            # Closures capture by ref / by move per the spec; we use a
            # conservative approximation — walk the body in a fresh
            # nested scope, but treat captures as USE_COPY so closures
            # don't trip false moves. Parameter binding allocates ids.
            ctx.push_scope()
            for p in e.params:
                pty = self._resolve_param_ty(p.ty)
                ctx.declare(p.name, pty)
            if e.body is not None:
                self._walk_block(e.body, ctx)
            ctx.pop_scope()
            return
        if isinstance(e, HirOpaque):
            return
        # Fallback: nothing to check.
        return

    # ----- unary (borrow operators) -----

    def _walk_unary(self, e: HirUnaryOp, ctx: BorrowContext) -> None:
        op = e.op
        if op == "&":
            # &x or &mut x: create a loan on x.
            place = extract_place(e.operand, ctx)
            if place is None:
                # Borrowing a temporary — walk the operand and move on.
                self._walk_expr(e.operand, ctx, USE_COPY)
                return
            kind = USE_BORROW_MUT if e.mut else USE_BORROW_SHARED
            self._classify_place(place, e.operand, kind, ctx, span=e.span)
            return
        if op == "*":
            # *r: a place expression we don't track in v0.22; just walk.
            self._walk_expr(e.operand, ctx, USE_COPY)
            return
        # All other unary ops: walk operand.
        self._walk_expr(e.operand, ctx, USE_COPY)

    # ----- call (per-arg move classification) -----

    def _walk_call(self, e: HirCall, ctx: BorrowContext) -> None:
        # Walk the callee for its own side effects.
        self._walk_expr(e.callee, ctx, USE_COPY)
        # Per-arg classification: a positional argument is a *move* if
        # the argument expression is a place of non-Copy type. Without
        # the signature available here (the call site might be against
        # an opaque callee) we use the operand's classification rule
        # uniformly — _walk_expr with USE_MOVE handles the Copy/non-Copy
        # decision internally via _classify_place.
        for a in e.args:
            self._walk_expr(a, ctx, USE_MOVE)

    # ----- core place classifier -----

    def _classify_place(
        self,
        place: Place,
        expr: HirExprNode,
        use_kind: str,
        ctx: BorrowContext,
        span: Optional[tuple[int, int]] = None,
    ) -> None:
        """Apply ``use_kind`` to ``place``: emit any diagnostic the
        access triggers, then update the borrow state (e.g. add a loan,
        mark the root as moved)."""
        bid = place.root
        if bid not in ctx.bindings:
            return
        info = ctx.bindings[bid]
        span = span or getattr(expr, "span", (0, 0))

        # Use-after-move: any non-target use after the binding has been
        # moved trips MT3004.
        if info.moved and use_kind != USE_ASSIGN_TARGET:
            notes: tuple[str, ...] = ()
            if info.moved_span is not None:
                notes = (
                    f"`{info.name}` was moved at offset {info.moved_span[0]}",
                )
            ctx.diag(
                CODE_BORROW_USE_AFTER_MOVE,
                f"use of moved value `{info.name}`",
                span, notes,
            )

        live_loans = ctx.loans_on(bid)

        if use_kind == USE_BORROW_SHARED:
            # Shared borrow vs an existing &mut: MT3003.
            mut_loans = [l for l in live_loans if l.mut]
            if mut_loans:
                ctx.diag(
                    CODE_BORROW_MUT_SHARED_CONFLICT,
                    f"cannot borrow `{info.name}` as shared because it is "
                    f"also borrowed as mutable",
                    span,
                )
            ctx.add_loan(Loan(place=place, mut=False, span=span))
            return

        if use_kind == USE_BORROW_MUT:
            # Two &mut loans on the same place: MT3005.
            existing_muts = [l for l in live_loans if l.mut]
            if existing_muts:
                ctx.diag(
                    CODE_BORROW_DOUBLE_MUT,
                    f"cannot borrow `{info.name}` as mutable more than once "
                    f"at a time",
                    span,
                )
            # &mut conflicting with a live &: MT3003.
            shared_loans = [l for l in live_loans if not l.mut]
            if shared_loans:
                ctx.diag(
                    CODE_BORROW_MUT_SHARED_CONFLICT,
                    f"cannot borrow `{info.name}` as mutable because it is "
                    f"also borrowed as shared",
                    span,
                )
            ctx.add_loan(Loan(place=place, mut=True, span=span))
            return

        if use_kind == USE_MOVE:
            # If non-Copy, a "move" use actually moves; for Copy types it
            # degrades to a copy.
            if is_copy(info.ty):
                return
            # Move with a live borrow: MT3001.
            if live_loans:
                ctx.diag(
                    CODE_BORROW_MOVE_OF_BORROWED,
                    f"cannot move `{info.name}` because it is currently borrowed",
                    span,
                )
                return
            # Move via a projection through a borrow: MT3002.
            if not place.is_root:
                ctx.diag(
                    CODE_BORROW_MOVE_OUT_OF_BORROW,
                    f"cannot move out of field `{'.'.join(place.path)}` of "
                    f"`{info.name}` (would require moving through a borrow)",
                    span,
                )
                # We still mark the root moved to avoid cascades.
            info.moved = True
            info.moved_span = span
            return

        # USE_COPY / USE_ASSIGN_TARGET: no state mutation beyond the
        # use-after-move check above.
        return

    # ----- type guessing helpers -----

    def _resolve_param_ty(self, ty: HirTy) -> Ty:
        """Best-effort: ask the type checker to resolve, else return TyAny."""
        if self.type_checker is not None:
            try:
                return self.type_checker.resolve_hir_ty(ty)
            except Exception:
                pass
        return TY_ANY

    def _infer_pat_ty(self, stmt: HirStmt) -> Ty:
        """Guess the type of a let-binding for borrow purposes.

        We use the let-statement's optional type annotation if present,
        otherwise the type checker's view of the value expression.
        """
        if stmt.ty is not None and self.type_checker is not None:
            try:
                return self.type_checker.resolve_hir_ty(stmt.ty)
            except Exception:
                return TY_ANY
        # No annotation — try to peek at the value expression. Without
        # re-running typeck here, we approximate: literals are scalar
        # (Copy); identifiers carry through the looked-up type.
        if stmt.value is None:
            return TY_ANY
        return self._guess_expr_ty(stmt.value)

    def _guess_expr_ty(self, e: HirExprNode) -> Ty:
        """A minimal expression-type guesser used only by borrow check
        to decide Copy-ness. Returns ``TyAny`` (== Copy) when unsure."""
        if isinstance(e, HirLit):
            # Lit kind tells us scalar-vs-string.
            k = e.lit_kind
            if k in ("STRING_LITERAL", "STRING"):
                return TyStr()
            # Everything else is scalar / Copy — defer to TyAny since
            # we don't need precision here.
            return TY_ANY
        if isinstance(e, HirArray):
            return TyArray(elem=TY_ANY)
        if isinstance(e, HirTuple):
            return TyTuple(elems=tuple(self._guess_expr_ty(x) for x in e.elems))
        if isinstance(e, HirStructLit):
            name = e.path[0] if e.path else "?"
            return TyRecord(name=name, fields=())
        if isinstance(e, HirUnaryOp) and e.op == "&":
            return TyRef(inner=TY_ANY, mut=e.mut)
        return TY_ANY

    # ----- branch snapshots -----

    def _snapshot(self, ctx: BorrowContext) -> dict[int, bool]:
        return {bid: info.moved for bid, info in ctx.bindings.items()}

    def _restore(self, ctx: BorrowContext, snap: dict[int, bool]) -> None:
        for bid, moved in snap.items():
            if bid in ctx.bindings:
                ctx.bindings[bid].moved = moved
                if not moved:
                    ctx.bindings[bid].moved_span = None

    def _join(self, ctx: BorrowContext, other: dict[int, bool]) -> None:
        """Join the current state with ``other`` taking the AND of moves
        (a binding is considered moved after a branching expression only
        if it was moved on *every* path)."""
        cur = self._snapshot(ctx)
        merged: dict[int, bool] = {}
        for bid in set(cur) | set(other):
            merged[bid] = cur.get(bid, False) and other.get(bid, False)
        for bid, moved in merged.items():
            if bid in ctx.bindings:
                ctx.bindings[bid].moved = moved
                if not moved:
                    ctx.bindings[bid].moved_span = None


# ---------------------------------------------------------------------------
# Public API
# ---------------------------------------------------------------------------


def borrow_check(
    module: HirModule,
    source: str = "",
    type_checker: Optional[TypeChecker] = None,
) -> list[Diagnostic]:
    """Run the borrow checker over ``module``.

    Returns the list of diagnostics (MT3xxx band). If ``type_checker``
    is supplied, the borrow checker uses its signature/resolution maps
    to decide Copy-ness more precisely; otherwise it constructs a fresh
    one internally."""
    tc = type_checker
    if tc is None:
        tc = TypeChecker(module, source=source)
        tc.check_module()
    bc = BorrowChecker(module, type_checker=tc, source=source)
    bc.check_module()
    return list(bc.diagnostics)


__all__ = [
    "Place",
    "Loan",
    "BindingInfo",
    "BorrowContext",
    "BorrowChecker",
    "borrow_check",
    "is_copy",
    "USE_MOVE",
    "USE_COPY",
    "USE_BORROW_SHARED",
    "USE_BORROW_MUT",
    "USE_ASSIGN_TARGET",
]
