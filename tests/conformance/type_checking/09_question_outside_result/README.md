# 09 question_outside_result

Positive-fire for **MT2010 QUESTION_OUTSIDE_RESULT**. Spec v1.0-RC §17 (errors).

The `?` operator requires the enclosing function's return type to be
`Result[T, E]`. Here `get` returns plain `I64`, so MT2010 fires at the
`?` site.
