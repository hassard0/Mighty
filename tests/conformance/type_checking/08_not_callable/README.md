# 08 not_callable

Positive-fire for **MT2008 NOT_CALLABLE**. Spec v1.0-RC §10 (functions).

The binding `x: I64` is applied as if it were a function (`x(1, 2)`).
Since `I64` does not have a function type, the type checker reports
MT2008.
