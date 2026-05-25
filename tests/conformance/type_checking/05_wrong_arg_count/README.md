# 05 wrong_arg_count

Positive-fire for **MT2005 WRONG_ARG_COUNT**. Spec v1.0-RC §10 (functions).

`add` is declared with 2 parameters; the call site supplies 1. The
type checker reports MT2005 at the call.
