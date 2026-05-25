# 01 type_mismatch

Positive-fire for **MT2001 TYPE_MISMATCH**. Spec v1.0-RC §6 (type system).

A function declared `want_str(s: Str)` is called with an `I64` literal
(`42`). The type checker unifies the argument type with the parameter
type, fails, and reports MT2001 at the call-site.
