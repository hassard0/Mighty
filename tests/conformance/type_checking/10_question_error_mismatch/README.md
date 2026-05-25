# 10 question_error_mismatch

Positive-fire for **MT2011 QUESTION_ERROR_MISMATCH**. Spec v1.0-RC §17 (errors).

`?` requires the operand's `Err` type to equal the enclosing function's
`Err` type (v0.3 is strict — no coercion). Here the operand error is
`Str` while the enclosing fn declares `I32`, so MT2011 fires.
