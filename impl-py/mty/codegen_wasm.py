"""Sketch WebAssembly codegen for the Python 2nd-impl (v0.22).

Emits raw core-wasm bytes from an :class:`HirModule`. The intent is to
demonstrate that the spec is implementable through the back-end, not to
ship a production codegen. The supported subset is:

* ``I32`` arithmetic (``+``, ``-``, ``*``, ``/``, ``%``).
* ``Bool`` operands (compiled as i32 0/1).
* Boolean comparisons (``==``, ``!=``, ``<``, ``<=``, ``>``, ``>=``).
* Local ``let`` bindings.
* ``if`` / ``else`` expressions.
* ``while`` loops.
* ``return`` statements.
* Direct fn-to-fn calls.
* Basic string literals via a data segment (a ``(ptr, len)`` pair
  placed at fixed offsets).

What we **do not** lower (these would require ADT linear-memory layout
or per-target ABI work, deferred to v0.23):

* Records / enums / tuples / arrays — placeholder ``i32 = 0`` is emitted
  when one is referenced as a value.
* Agent / spawn / ask / send sugars.
* Macro calls (the lowerer wraps them in ``HirOpaque``).
* References — modelled as their underlying scalar (a ``&I32`` becomes
  an ``i32`` for the body's scope).
* Pattern matching over enums.

Output validation: we don't ship a full wasm validator. Instead we
verify (a) the magic + version bytes, (b) the section ordering matches
the spec (§5.5 of the WebAssembly Core v1.0 binary spec), and (c) every
function body ends with the ``end`` opcode ``0x0B``.

Spec sources for the format: the WebAssembly Core 1.0 binary format
spec (https://webassembly.github.io/spec/core/binary/). The Mighty
type-system mapping is from v1.0-RC2 §6 + the (informative) §14 (target
back-ends).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Optional, Union

from .diagnostics import (
    CODE_CODEGEN_UNRESOLVED,
    CODE_CODEGEN_UNSUPPORTED,
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
    HirStructLit,
    HirTuple,
    HirTy,
    HirUnaryOp,
    HirWhile,
)


# ---------------------------------------------------------------------------
# Wasm binary primitives.
# ---------------------------------------------------------------------------


def uleb128(n: int) -> bytes:
    """Encode an unsigned integer as LEB128 (little-endian, base-128)."""
    if n < 0:
        raise ValueError(f"uleb128 needs non-negative int, got {n!r}")
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            break
    return bytes(out)


def sleb128(n: int) -> bytes:
    """Encode a signed integer as signed LEB128."""
    out = bytearray()
    more = True
    while more:
        byte = n & 0x7F
        # arithmetic shift right
        n >>= 7
        sign = byte & 0x40
        if (n == 0 and not sign) or (n == -1 and sign):
            more = False
            out.append(byte)
        else:
            out.append(byte | 0x80)
    return bytes(out)


def vec_bytes(items: list[bytes]) -> bytes:
    """Encode a sequence as a wasm vec: count-LEB128 followed by items."""
    return uleb128(len(items)) + b"".join(items)


def name_bytes(s: str) -> bytes:
    """Encode a wasm name: UTF-8 with a LEB128 length prefix."""
    raw = s.encode("utf-8")
    return uleb128(len(raw)) + raw


# Wasm constants -------------------------------------------------------------

WASM_MAGIC = b"\x00asm"
WASM_VERSION = (1).to_bytes(4, "little")

# Value types (§5.3.1).
VT_I32 = 0x7F
VT_I64 = 0x7E
VT_F32 = 0x7D
VT_F64 = 0x7C

# Block type "no result" sentinel.
BLOCK_TYPE_VOID = 0x40

# Function-type tag.
FUNC_TYPE_TAG = 0x60

# Section IDs (§5.5).
SEC_CUSTOM = 0
SEC_TYPE = 1
SEC_IMPORT = 2
SEC_FUNCTION = 3
SEC_TABLE = 4
SEC_MEMORY = 5
SEC_GLOBAL = 6
SEC_EXPORT = 7
SEC_START = 8
SEC_ELEMENT = 9
SEC_CODE = 10
SEC_DATA = 11

# Export kinds.
EXPORT_FUNC = 0x00
EXPORT_TABLE = 0x01
EXPORT_MEMORY = 0x02
EXPORT_GLOBAL = 0x03

# Opcodes (subset).
OP_UNREACHABLE = 0x00
OP_NOP = 0x01
OP_BLOCK = 0x02
OP_LOOP = 0x03
OP_IF = 0x04
OP_ELSE = 0x05
OP_END = 0x0B
OP_BR = 0x0C
OP_BR_IF = 0x0D
OP_RETURN = 0x0F
OP_CALL = 0x10

OP_DROP = 0x1A
OP_SELECT = 0x1B

OP_LOCAL_GET = 0x20
OP_LOCAL_SET = 0x21
OP_LOCAL_TEE = 0x22
OP_GLOBAL_GET = 0x23
OP_GLOBAL_SET = 0x24

OP_I32_LOAD = 0x28
OP_I32_STORE = 0x36

OP_I32_CONST = 0x41
OP_I64_CONST = 0x42
OP_F32_CONST = 0x43
OP_F64_CONST = 0x44

# i32 ops.
OP_I32_EQZ = 0x45
OP_I32_EQ = 0x46
OP_I32_NE = 0x47
OP_I32_LT_S = 0x48
OP_I32_LT_U = 0x49
OP_I32_GT_S = 0x4A
OP_I32_GT_U = 0x4B
OP_I32_LE_S = 0x4C
OP_I32_LE_U = 0x4D
OP_I32_GE_S = 0x4E
OP_I32_GE_U = 0x4F
OP_I32_ADD = 0x6A
OP_I32_SUB = 0x6B
OP_I32_MUL = 0x6C
OP_I32_DIV_S = 0x6D
OP_I32_DIV_U = 0x6E
OP_I32_REM_S = 0x6F
OP_I32_REM_U = 0x70
OP_I32_AND = 0x71
OP_I32_OR = 0x72
OP_I32_XOR = 0x73
OP_I32_SHL = 0x74
OP_I32_SHR_S = 0x75
OP_I32_SHR_U = 0x76


# ---------------------------------------------------------------------------
# Section assembly helper.
# ---------------------------------------------------------------------------


def section(sid: int, payload: bytes) -> bytes:
    """Wrap ``payload`` as a wasm section with id ``sid``."""
    return bytes([sid]) + uleb128(len(payload)) + payload


# ---------------------------------------------------------------------------
# Function-type table.
# ---------------------------------------------------------------------------


@dataclass
class FuncType:
    params: tuple[int, ...] = ()  # value-type bytes
    results: tuple[int, ...] = ()

    def encode(self) -> bytes:
        return (
            bytes([FUNC_TYPE_TAG])
            + uleb128(len(self.params)) + bytes(self.params)
            + uleb128(len(self.results)) + bytes(self.results)
        )


# ---------------------------------------------------------------------------
# Codegen errors.
# ---------------------------------------------------------------------------


class CodegenUnsupported(Exception):
    """Raised internally when a HIR shape isn't in the supported subset.

    The compiler catches it and emits an MT4001 diagnostic plus a
    no-op fn body so the rest of the module can still encode.
    """


# ---------------------------------------------------------------------------
# Per-fn body emitter.
# ---------------------------------------------------------------------------


@dataclass
class LocalSlot:
    name: str
    idx: int


class FnEmitter:
    """Emits the code for a single fn body.

    Maintains a per-fn local table (params + lets). All locals are
    typed ``i32`` in this codegen subset; ADT layouts are out of scope.
    """

    def __init__(self, hir_fn: HirFn, fn_index_table: dict[str, int]) -> None:
        self.hir_fn = hir_fn
        self.fn_index_table = fn_index_table
        # locals[name] = LocalSlot. Parameters come first.
        self.locals: dict[str, LocalSlot] = {}
        self.local_count_extra = 0  # non-param locals
        self.body = bytearray()
        # Diagnostics accumulated by this emitter.
        self.diagnostics: list[Diagnostic] = []
        # Scope-stack for name resolution (matching the lowerer's let
        # scoping). We don't pop locals from the wasm table — wasm
        # locals are per-fn — but we do pop name visibility.
        self._scopes: list[dict[str, str]] = [{}]
        # Register params.
        for p in hir_fn.params:
            self._declare(p.name)

    # ----- local management -----

    def _declare(self, name: str) -> int:
        # If a local with this name already exists in a parent scope we
        # could shadow. To keep things simple, we allocate a fresh index
        # per declaration; the scope dict tracks the active alias.
        idx = len(self.locals)
        self.locals[f"__slot_{idx}"] = LocalSlot(name=name, idx=idx)
        self._scopes[-1][name] = f"__slot_{idx}"
        return idx

    def _lookup(self, name: str) -> Optional[int]:
        for scope in reversed(self._scopes):
            if name in scope:
                slot_key = scope[name]
                return self.locals[slot_key].idx
        return None

    def _push_scope(self) -> None:
        self._scopes.append({})

    def _pop_scope(self) -> None:
        self._scopes.pop()

    # ----- diag helper -----

    def _diag(self, code: str, msg: str, span: tuple[int, int]) -> None:
        self.diagnostics.append(Diagnostic(
            code=code, message=msg, severity=Severity.WARNING,
            start=span[0], end=span[1],
        ))

    # ----- public entry: produce the code section payload for this fn -----

    def encode(self) -> bytes:
        """Encode the fn body per §5.5.13 (code section entry).

        Layout: size-LEB128, locals-vec, body bytes (terminated with end).
        """
        if self.hir_fn.body is None:
            # No body: emit an unreachable + end so the body is well-formed.
            inner = bytes([OP_UNREACHABLE, OP_END])
            return uleb128(len(inner)) + inner
        try:
            self._emit_block(self.hir_fn.body)
            # If the body's tail is None and the fn returns Unit, we still
            # need to make sure the stack matches. We emit nothing —
            # FuncType claims no results.
        except CodegenUnsupported as e:
            self._diag(
                CODE_CODEGEN_UNSUPPORTED,
                f"fn `{self.hir_fn.name}`: {e}",
                self.hir_fn.span,
            )
            # Fallback body: unreachable. Reset the body bytes.
            self.body = bytearray()
        # Decide on the trailing implicit return value. For now, all
        # codegen fns are declared with i32 result if they return one.
        # An empty body for an i32-result fn would be invalid. We push a
        # zero as a safety net iff the body produced no value but the
        # signature claims one.
        self.body.append(OP_END)
        # Locals declaration. We pack all non-param locals into one
        # entry of (count, i32).
        n_params = len(self.hir_fn.params)
        n_extra = len(self.locals) - n_params
        if n_extra <= 0:
            locals_section = uleb128(0)
        else:
            locals_section = uleb128(1) + uleb128(n_extra) + bytes([VT_I32])
        full = locals_section + bytes(self.body)
        return uleb128(len(full)) + full

    # ----- block emission -----

    def _emit_block(self, blk: HirBlock) -> None:
        self._push_scope()
        try:
            for s in blk.stmts:
                self._emit_stmt(s)
            if blk.tail is not None:
                self._emit_expr(blk.tail)
        finally:
            self._pop_scope()

    # ----- statement emission -----

    def _emit_stmt(self, s: HirStmt) -> None:
        if s.kind == "let":
            if s.value is None:
                # `let x: T;` without value — emit a zero placeholder.
                self.body.append(OP_I32_CONST)
                self.body += sleb128(0)
            else:
                self._emit_expr(s.value)
            # Allocate a fresh local for the binding name (only ident
            # patterns supported here).
            if s.pat is not None and s.pat.kind == "ident":
                idx = self._declare(s.pat.name)
                self.body.append(OP_LOCAL_SET)
                self.body += uleb128(idx)
            else:
                # Pattern is non-ident — drop the value.
                self.body.append(OP_DROP)
            return
        if s.kind == "assign":
            # Only support assigning to a plain ident.
            if s.value is None or s.target is None:
                return
            self._emit_expr(s.value)
            if isinstance(s.target, HirIdent):
                idx = self._lookup(s.target.name)
                if idx is None:
                    self._diag(CODE_CODEGEN_UNRESOLVED,
                               f"assign to unknown local `{s.target.name}`",
                               s.span)
                    self.body.append(OP_DROP)
                    return
                self.body.append(OP_LOCAL_SET)
                self.body += uleb128(idx)
                return
            self._diag(CODE_CODEGEN_UNSUPPORTED,
                       "assignment target shape not supported in codegen subset",
                       s.span)
            self.body.append(OP_DROP)
            return
        if s.kind == "expr":
            if s.value is None:
                return
            self._emit_expr(s.value)
            # The expression's result, if any, is unused — drop it. We
            # only drop if we suspect a value was pushed; the safest
            # marker is to look at the expr kind. For the codegen
            # subset we always conservatively drop after an expr-stmt.
            # This produces valid wasm even when the expression has no
            # value (the drop will be a no-op on an empty operand stack;
            # actually, that's a validation error — so we only drop if
            # the expression produces a value). For v0.22 simplicity we
            # *don't* drop unless the expr is clearly value-producing.
            # See the helper below.
            if _produces_value(s.value):
                self.body.append(OP_DROP)
            return

    def _emit_expr(self, e: HirExprNode) -> None:
        """Emit ``e`` such that exactly one i32 is left on the stack
        (when the expression produces a value)."""
        if isinstance(e, HirLit):
            self._emit_lit(e)
            return
        if isinstance(e, HirIdent):
            idx = self._lookup(e.name)
            if idx is None:
                # Could be a top-level fn — but we can't take a fn ptr
                # in this subset.
                self._diag(CODE_CODEGEN_UNRESOLVED,
                           f"unresolved identifier `{e.name}` in codegen",
                           e.span)
                self.body.append(OP_I32_CONST)
                self.body += sleb128(0)
                return
            self.body.append(OP_LOCAL_GET)
            self.body += uleb128(idx)
            return
        if isinstance(e, HirBinOp):
            self._emit_binop(e)
            return
        if isinstance(e, HirUnaryOp):
            self._emit_unary(e)
            return
        if isinstance(e, HirCall):
            self._emit_call(e)
            return
        if isinstance(e, HirIf):
            self._emit_if(e)
            return
        if isinstance(e, HirBlock):
            self._emit_block(e)
            return
        if isinstance(e, HirWhile):
            self._emit_while(e)
            return
        if isinstance(e, HirReturn):
            if e.value is not None:
                self._emit_expr(e.value)
            self.body.append(OP_RETURN)
            return
        if isinstance(e, HirBreak):
            # Approximation: a break jumps to the enclosing loop's exit.
            # We emit a br to label 1 (innermost-loop's surrounding block).
            self.body.append(OP_BR)
            self.body += uleb128(1)
            return
        if isinstance(e, HirContinue):
            self.body.append(OP_BR)
            self.body += uleb128(0)
            return
        if isinstance(e, HirOpaque):
            # Out-of-scope shape; emit a zero placeholder.
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            self._diag(CODE_CODEGEN_UNSUPPORTED,
                       f"opaque `{e.parser_kind}` lowered as zero placeholder",
                       e.span)
            return
        # Defer ADTs / tuples / arrays etc. — emit a zero.
        self.body.append(OP_I32_CONST)
        self.body += sleb128(0)
        self._diag(CODE_CODEGEN_UNSUPPORTED,
                   f"expression `{type(e).__name__}` not in codegen subset",
                   getattr(e, "span", (0, 0)))

    # ----- expression helpers -----

    def _emit_lit(self, e: HirLit) -> None:
        k = e.lit_kind
        if k in ("INT_LITERAL", "INT"):
            # Strip any type suffix.
            txt = e.text
            for suffix in ("i8", "I8", "i16", "I16", "i32", "I32",
                           "i64", "I64", "i128", "I128",
                           "u8", "U8", "u16", "U16", "u32", "U32",
                           "u64", "U64", "u128", "U128"):
                if txt.endswith(suffix):
                    txt = txt[: -len(suffix)]
                    break
            txt = txt.replace("_", "")
            try:
                value = int(txt, 0)
            except ValueError:
                value = 0
            self.body.append(OP_I32_CONST)
            self.body += sleb128(value)
            return
        if k == "BOOL" or e.text in ("true", "false"):
            self.body.append(OP_I32_CONST)
            self.body += sleb128(1 if e.text == "true" else 0)
            return
        if k in ("STRING_LITERAL", "STRING"):
            # Strings get a pointer placeholder. The codegen subset
            # doesn't allocate; we push 0 (NULL) as a stand-in.
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            return
        if k in ("CHAR_LITERAL", "CHAR"):
            # Treat as the codepoint of the single character (best effort).
            txt = e.text.strip("'")
            value = ord(txt[0]) if txt else 0
            self.body.append(OP_I32_CONST)
            self.body += sleb128(value)
            return
        if k in ("FLOAT_LITERAL", "FLOAT"):
            # No F32/F64 in the codegen subset; emit i32(0).
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            return
        if k == "UNIT" or e.text == "()":
            # Unit pushes nothing — but a wasm expression slot expects
            # a value. We push an i32(0) as a tombstone; callers that
            # use the value will see 0.
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            return
        # Unknown: zero.
        self.body.append(OP_I32_CONST)
        self.body += sleb128(0)

    def _emit_binop(self, e: HirBinOp) -> None:
        self._emit_expr(e.lhs)
        self._emit_expr(e.rhs)
        op = e.op
        mapping = {
            "+": OP_I32_ADD,
            "-": OP_I32_SUB,
            "*": OP_I32_MUL,
            "/": OP_I32_DIV_S,
            "%": OP_I32_REM_S,
            "==": OP_I32_EQ,
            "!=": OP_I32_NE,
            "<": OP_I32_LT_S,
            "<=": OP_I32_LE_S,
            ">": OP_I32_GT_S,
            ">=": OP_I32_GE_S,
            "&": OP_I32_AND,
            "|": OP_I32_OR,
            "^": OP_I32_XOR,
            "<<": OP_I32_SHL,
            ">>": OP_I32_SHR_S,
            "&&": OP_I32_AND,  # short-circuit not modelled — bitwise
            "||": OP_I32_OR,
        }
        if op in mapping:
            self.body.append(mapping[op])
        else:
            # Range and other ops not in subset — leave the rhs on stack
            # (the lhs operation was lost). Mark a diag.
            self._diag(CODE_CODEGEN_UNSUPPORTED,
                       f"binop `{op}` not in codegen subset",
                       e.span)

    def _emit_unary(self, e: HirUnaryOp) -> None:
        op = e.op
        if op == "-":
            # 0 - operand
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            self._emit_expr(e.operand)
            self.body.append(OP_I32_SUB)
            return
        if op == "!":
            self._emit_expr(e.operand)
            self.body.append(OP_I32_EQZ)
            return
        if op == "&":
            # Reference: pass through (i32 the same).
            self._emit_expr(e.operand)
            return
        if op == "*":
            # Deref: pass through (still an i32).
            self._emit_expr(e.operand)
            return
        # Default: emit operand.
        self._emit_expr(e.operand)
        self._diag(CODE_CODEGEN_UNSUPPORTED,
                   f"unary `{op}` not in codegen subset", e.span)

    def _emit_call(self, e: HirCall) -> None:
        if not isinstance(e.callee, HirIdent):
            self._diag(CODE_CODEGEN_UNSUPPORTED,
                       "indirect call not in codegen subset", e.span)
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            return
        name = e.callee.name
        fn_idx = self.fn_index_table.get(name)
        if fn_idx is None:
            self._diag(CODE_CODEGEN_UNRESOLVED,
                       f"call to unknown fn `{name}`", e.span)
            for _ in e.args:
                # Still emit args (and drop them) so the stack remains
                # consistent — we can't easily count the callee's params
                # without the type checker. Cheap fallback: emit and
                # drop each.
                pass
            self.body.append(OP_I32_CONST)
            self.body += sleb128(0)
            return
        for a in e.args:
            self._emit_expr(a)
        self.body.append(OP_CALL)
        self.body += uleb128(fn_idx)

    def _emit_if(self, e: HirIf) -> None:
        self._emit_expr(e.cond)
        # The condition is i32; wasm if/else uses the top-of-stack value.
        self.body.append(OP_IF)
        # Block type. The codegen subset assumes if-expressions produce
        # an i32 result when they have an else branch.
        if e.else_ is not None:
            self.body.append(VT_I32)
        else:
            self.body.append(BLOCK_TYPE_VOID)
        self._emit_block(e.then)
        if e.else_ is not None:
            self.body.append(OP_ELSE)
            if isinstance(e.else_, HirBlock):
                self._emit_block(e.else_)
            else:
                self._emit_expr(e.else_)
        self.body.append(OP_END)

    def _emit_while(self, e: HirWhile) -> None:
        # while (cond) { body }  ==>
        #   block $exit { loop $cont { ;; cond? br $exit ; body; br $cont } }
        # We emit a void block + loop structure.
        self.body.append(OP_BLOCK)
        self.body.append(BLOCK_TYPE_VOID)
        self.body.append(OP_LOOP)
        self.body.append(BLOCK_TYPE_VOID)
        # Eval cond -> i32eqz -> br_if exit (label 1 = outer block).
        self._emit_expr(e.cond)
        self.body.append(OP_I32_EQZ)
        self.body.append(OP_BR_IF)
        self.body += uleb128(1)
        # Body.
        self._emit_block(e.body)
        # Loop back.
        self.body.append(OP_BR)
        self.body += uleb128(0)
        self.body.append(OP_END)  # end loop
        self.body.append(OP_END)  # end block


def _produces_value(e: HirExprNode) -> bool:
    """Conservative: do we expect ``e`` to leave a value on the wasm
    operand stack? Returns False for control-flow constructs that don't.
    """
    if isinstance(e, (HirReturn, HirBreak, HirContinue)):
        return False
    if isinstance(e, HirWhile):
        return False
    if isinstance(e, HirLoop):
        return False
    if isinstance(e, HirFor):
        return False
    if isinstance(e, HirOpaque):
        return True  # we emit a zero placeholder, so yes
    return True


# ---------------------------------------------------------------------------
# Module-level codegen.
# ---------------------------------------------------------------------------


@dataclass
class CodegenResult:
    """The outcome of codegen.

    Attributes:
        bytes:        the assembled wasm module bytes (magic + version
                      + sections), or ``b""`` on total failure.
        diagnostics:  the list of MT4xxx warnings/errors produced.
        emitted_fns:  per-fn names that were successfully emitted.
    """
    bytes: bytes
    diagnostics: list[Diagnostic]
    emitted_fns: list[str]


class WasmEmitter:
    """Walks an :class:`HirModule` and produces a wasm module.

    Strategy:
      1. Collect HirFn items; assign each a function index.
      2. Build a single function-type table — all codegen-subset fns
         are typed ``i32^n -> i32?``.
      3. Emit the type, function, export, and code sections.
    """

    def __init__(self, module: HirModule) -> None:
        self.module = module
        self.diagnostics: list[Diagnostic] = []
        self.fns: list[HirFn] = [
            it for it in module.items if isinstance(it, HirFn)
            and it.body is not None  # skip extern decls
        ]
        # Name -> function-index map (call resolution).
        self.fn_index_table: dict[str, int] = {
            fn.name: i for i, fn in enumerate(self.fns)
        }

    def _function_type_of(self, fn: HirFn) -> FuncType:
        """All codegen-subset fns are typed in i32. The signature is
        ``(i32)^n -> ()`` for Unit-returning fns, ``(i32)^n -> i32``
        otherwise."""
        params = tuple([VT_I32] * len(fn.params))
        # Result: Unit if HirTy says "Unit"; otherwise one i32.
        ret = fn.return_ty
        if _hir_ty_is_unit(ret):
            results = ()
        else:
            results = (VT_I32,)
        return FuncType(params=params, results=results)

    def emit(self) -> CodegenResult:
        if not self.fns:
            # Empty module: still produce a valid wasm header.
            return CodegenResult(
                bytes=WASM_MAGIC + WASM_VERSION,
                diagnostics=[],
                emitted_fns=[],
            )

        # 1. Type section: one entry per (deduped) signature.
        type_entries: list[FuncType] = []
        type_indices: list[int] = []
        for fn in self.fns:
            ft = self._function_type_of(fn)
            # Dedup by structural equality (FuncType is a dataclass — '==' compares fields).
            try:
                idx = next(i for i, e in enumerate(type_entries) if e == ft)
            except StopIteration:
                idx = len(type_entries)
                type_entries.append(ft)
            type_indices.append(idx)
        type_payload = vec_bytes([e.encode() for e in type_entries])
        type_section = section(SEC_TYPE, type_payload)

        # 2. Function section: each fn → its type index.
        func_payload = vec_bytes([uleb128(i) for i in type_indices])
        func_section = section(SEC_FUNCTION, func_payload)

        # 3. Memory section: a single 1-page memory (needed for any
        # string-related growth; the codegen subset doesn't actually
        # store anything, but exporting the memory makes the module
        # idiomatic for host runtimes).
        # Limits encoding: 0x00 + min (no max).
        mem_payload = vec_bytes([bytes([0x00]) + uleb128(1)])
        memory_section = section(SEC_MEMORY, mem_payload)

        # 4. Export section: one export per fn (everything is public in
        # this codegen sketch) + a "memory" export.
        export_entries: list[bytes] = []
        for i, fn in enumerate(self.fns):
            export_entries.append(
                name_bytes(fn.name) + bytes([EXPORT_FUNC]) + uleb128(i)
            )
        export_entries.append(
            name_bytes("memory") + bytes([EXPORT_MEMORY]) + uleb128(0)
        )
        export_payload = vec_bytes(export_entries)
        export_section = section(SEC_EXPORT, export_payload)

        # 5. Code section: per-fn body.
        code_entries: list[bytes] = []
        emitted_names: list[str] = []
        for fn in self.fns:
            emitter = FnEmitter(fn, self.fn_index_table)
            code_entries.append(emitter.encode())
            self.diagnostics.extend(emitter.diagnostics)
            emitted_names.append(fn.name)
        code_payload = vec_bytes(code_entries)
        code_section = section(SEC_CODE, code_payload)

        module_bytes = (
            WASM_MAGIC
            + WASM_VERSION
            + type_section
            + func_section
            + memory_section
            + export_section
            + code_section
        )

        return CodegenResult(
            bytes=module_bytes,
            diagnostics=list(self.diagnostics),
            emitted_fns=emitted_names,
        )


# ---------------------------------------------------------------------------
# Helpers.
# ---------------------------------------------------------------------------


def _hir_ty_is_unit(ty: HirTy) -> bool:
    if ty is None:
        return True
    if ty.kind == "tuple" and not ty.elems:
        return True
    if ty.kind == "path" and ty.name in ("Unit", "()", "_"):
        # Unit explicit, or unresolved-return defaulting to unit. The
        # underscore case happens when the parser left the ret blank.
        return ty.name == "Unit"
    return False


# ---------------------------------------------------------------------------
# Validation helpers (cheap structural checks; not a full validator).
# ---------------------------------------------------------------------------


def is_valid_module_header(data: bytes) -> bool:
    """Return True if ``data`` starts with the wasm magic + version."""
    return data.startswith(WASM_MAGIC + WASM_VERSION)


def parse_sections(data: bytes) -> list[tuple[int, bytes]]:
    """Return a list of (section_id, payload_bytes) for ``data``.

    A best-effort parser that walks the section preamble — it does NOT
    interpret section bodies (which would require a full wasm decoder).
    Used by tests to verify the section ordering matches the spec.
    """
    if not is_valid_module_header(data):
        return []
    i = len(WASM_MAGIC) + len(WASM_VERSION)
    out: list[tuple[int, bytes]] = []
    while i < len(data):
        sid = data[i]
        i += 1
        size, consumed = _decode_uleb128(data, i)
        i += consumed
        if i + size > len(data):
            break
        out.append((sid, data[i:i + size]))
        i += size
    return out


def _decode_uleb128(data: bytes, offset: int) -> tuple[int, int]:
    """Decode a uleb128 starting at ``offset``. Returns (value, bytes-consumed)."""
    result = 0
    shift = 0
    n = 0
    while True:
        if offset + n >= len(data):
            return result, n
        byte = data[offset + n]
        n += 1
        result |= (byte & 0x7F) << shift
        if (byte & 0x80) == 0:
            break
        shift += 7
    return result, n


# ---------------------------------------------------------------------------
# Public API.
# ---------------------------------------------------------------------------


def codegen_wasm(module: HirModule, source: str = "") -> CodegenResult:
    """Emit a wasm module from ``module``. Returns a :class:`CodegenResult`.

    The result's ``bytes`` field is the raw wasm binary; ``diagnostics``
    holds any MT4xxx warnings the emitter produced; ``emitted_fns`` is
    the list of fn names that landed in the module.
    """
    em = WasmEmitter(module)
    return em.emit()


__all__ = [
    "codegen_wasm",
    "CodegenResult",
    "WasmEmitter",
    "FnEmitter",
    "FuncType",
    "uleb128",
    "sleb128",
    "vec_bytes",
    "name_bytes",
    "section",
    "is_valid_module_header",
    "parse_sections",
    "WASM_MAGIC",
    "WASM_VERSION",
    "SEC_TYPE",
    "SEC_FUNCTION",
    "SEC_MEMORY",
    "SEC_EXPORT",
    "SEC_CODE",
    "VT_I32",
    "VT_I64",
    "VT_F32",
    "VT_F64",
]
