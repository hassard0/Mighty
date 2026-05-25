# 11 binop_type_mismatch

Positive-fire for **MT2017 BINOP_TYPE_MISMATCH**. Spec v1.0-RC §11 (control flow / expressions).

The binary operator `+` is applied to operands of incompatible
types (`I64 + Str`); the type checker rejects with MT2017.
