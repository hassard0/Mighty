# 06 unknown_field

Positive-fire for **MT2006 UNKNOWN_FIELD**. Spec v1.0-RC §6 (types).

The struct literal lists `missing` but the `User` declaration does not
contain that field. The type checker reports MT2006 at the literal.
