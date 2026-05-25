# 02 dyn_unsafe

Positive-fire for **MT4023 DYN_REQUIRES_OBJECT_SAFE**. Spec v1.0-RC §19 (dyn).

`dyn Clone` requires Clone to be object-safe. Because `fn clone(self) -> Self`
mentions `Self` in the return position, the trait is not object-safe;
the type checker reports MT4023 on the `dyn Clone` use.
