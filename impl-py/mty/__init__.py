"""mty: a reference Python implementation of the Mighty front-end.

This package is the second independent implementation of the Mighty
language toolchain front-end (lexer + parser), produced from the
v1.0-RC2 specification *prose alone* with no source-peeking into the
Rust reference implementation under ``crates/mty-syntax`` or
``selfhost/``.

Public surface:

* :mod:`mty.lexer`        -- token producer
* :mod:`mty.parser`       -- recursive-descent parser
* :mod:`mty.diagnostics`  -- ``Diagnostic`` dataclass and ``MTxxxx`` codes

The reference implementation lives at the Mighty workspace's Rust
crates; this package exists to validate that the spec is implementable
from the document alone, per the v1.0 freeze plan.
"""

__version__ = "0.1.0"

from .lexer import Token, TokenKind, lex
from .parser import Parser, parse
from .diagnostics import Diagnostic, Severity

__all__ = [
    "Token",
    "TokenKind",
    "lex",
    "Parser",
    "parse",
    "Diagnostic",
    "Severity",
    "__version__",
]
