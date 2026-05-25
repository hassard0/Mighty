# 15 missing_struct_field

Positive-fire for **MT2013 MISSING_STRUCT_FIELD**. Spec v1.0-RC §6 (types).

The struct literal initialises only `x`; field `y` is missing. The
type checker reports MT2013 on the literal.
