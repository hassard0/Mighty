"""Diagnostic dataclass and minimal MT-code registry.

Only the codes this front-end actually emits are enumerated here. The
authoritative diagnostic catalog lives in v1.0-RC2 §33 (numeric bands)
and the per-code messages live in ``docs/reference/diagnostics.md`` of
the reference workspace.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class Severity(Enum):
    ERROR = "error"
    WARNING = "warning"
    NOTE = "note"


@dataclass(frozen=True)
class Diagnostic:
    """A single compiler diagnostic.

    Attributes:
        code:     the ``MTxxxx`` numeric code (e.g. ``"MT0001"``).
        message:  the human-readable message.
        severity: severity classification.
        start:    byte-offset in source (inclusive) -- ``None`` if N/A.
        end:      byte-offset in source (exclusive) -- ``None`` if N/A.
        notes:    additional explanatory lines.
    """

    code: str
    message: str
    severity: Severity = Severity.ERROR
    start: Optional[int] = None
    end: Optional[int] = None
    notes: tuple[str, ...] = field(default_factory=tuple)

    def __str__(self) -> str:  # pragma: no cover - cosmetic
        loc = ""
        if self.start is not None:
            loc = f" @ {self.start}-{self.end}"
        head = f"{self.severity.value}[{self.code}]{loc}: {self.message}"
        if not self.notes:
            return head
        return "\n".join([head, *(f"  note: {n}" for n in self.notes)])


# Lexer / parser codes this implementation emits. The numeric bands
# follow §33 of v1.0-RC2: MT0xxx = lexer, MT1xxx = parser.
CODE_LEX_UNKNOWN_CHAR = "MT0001"       # unrecognised byte/char
CODE_LEX_BOM_REJECTED = "MT0002"       # leading UTF-8 BOM forbidden (per §3.1)
CODE_LEX_UNTERMINATED_STRING = "MT0003"
CODE_LEX_UNTERMINATED_BLOCK_COMMENT = "MT0004"
CODE_LEX_BAD_ESCAPE = "MT0005"
CODE_LEX_BAD_NUMBER = "MT0006"
CODE_LEX_BAD_CHAR_LITERAL = "MT0007"

CODE_PARSE_EXPECTED = "MT1001"          # generic "expected X, got Y"
CODE_PARSE_UNEXPECTED_EOF = "MT1002"
CODE_PARSE_BAD_ITEM = "MT1003"
CODE_PARSE_BAD_EXPR = "MT1004"

# Lowering (HIR) codes -- MT15xx band (between parser and typeck).
# Emitted by mty.lower; they cover the small set of name-resolution and
# duplicate-item conditions the v0.17 lowerer actually checks.
CODE_LOWER_UNRESOLVED_NAME = "MT1501"
CODE_LOWER_UNSUPPORTED_SHAPE = "MT1502"
CODE_LOWER_DUPLICATE_ITEM = "MT1503"

# Type-check codes -- MT2xxx band (per v1.0-RC2 §33 type errors).
# The exact numeric assignments here are this implementation's
# interpretation; the Rust reference may pick different numbers within
# the same band. See dev/history/notes/PYTHON_IMPL_V0_17_NOTES.md.
CODE_TYPECK_MISMATCH = "MT2001"             # generic "expected T, got U"
CODE_TYPECK_UNKNOWN_NAME = "MT2002"
CODE_TYPECK_ARITY_MISMATCH = "MT2003"       # wrong number of fn args
CODE_TYPECK_FIELD_MISMATCH = "MT2004"       # struct lit field mismatch
CODE_TYPECK_NOT_CALLABLE = "MT2005"
CODE_TYPECK_NOT_INDEXABLE = "MT2006"
CODE_TYPECK_BRANCH_MISMATCH = "MT2007"      # if/match branches disagree
CODE_TYPECK_RETURN_MISMATCH = "MT2008"
CODE_TYPECK_OPERATOR_TYPE = "MT2009"        # binop operand type bad
CODE_TYPECK_OCCURS_CHECK = "MT2010"         # infinite-type
# v0.19 — HM closure inference + generic constraints.
CODE_TYPECK_CLOSURE_ARITY = "MT2011"        # closure arg count != expected
CODE_TYPECK_BOUND_UNSATISFIED = "MT2012"    # generic bound unsatisfied
CODE_TYPECK_UNKNOWN_GENERIC = "MT2013"      # generic name not declared

# v0.22 — borrow-check codes (MT3xxx band per v1.0-RC2 §33). The
# Python 2nd-impl ships an NLL-flavoured subset; codes are this impl's
# interpretation and may not match the Rust reference numerically.
CODE_BORROW_MOVE_OF_BORROWED = "MT3001"     # moving a value while it is borrowed
CODE_BORROW_MOVE_OUT_OF_BORROW = "MT3002"   # moving out of a (&T) borrow
CODE_BORROW_MUT_SHARED_CONFLICT = "MT3003"  # mut and shared coexist
CODE_BORROW_USE_AFTER_MOVE = "MT3004"       # use after the value moved
CODE_BORROW_DOUBLE_MUT = "MT3005"           # two &mut borrows alive at once

# v0.22 — codegen codes (MT4xxx band). Emitted when a HIR construct
# cannot be lowered to the supported wasm subset (the codegen sketch
# defers ADTs/agents/macros).
CODE_CODEGEN_UNSUPPORTED = "MT4001"         # feature not in the codegen subset
CODE_CODEGEN_UNRESOLVED = "MT4002"          # unresolved name at codegen time
