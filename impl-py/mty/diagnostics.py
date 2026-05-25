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
