# 03 derive_copy_bad

Positive-fire for **MT4040 DERIVE_COPY_FIELD_NOT_COPY**. Spec v1.0-RC §19 (derive).

`#[derive(Copy)]` on `Holds` requires every field to be Copy. The
`String` field `s` is not Copy, so the type checker rejects the derive
with MT4040.
