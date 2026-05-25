# type_checking/16_wrong_variant_arity

MT2012 positive-fire (promotes auxiliary → covered). Variant `P(I32,
I32)` declares 2 payload fields; pattern `P(a, b, c)` binds 3 → the
pattern-matcher fires `diag::wrong_variant_arity`
(`mty-types/src/check.rs:1545`).

Spec ref: §6 type system, enum payload arity.
