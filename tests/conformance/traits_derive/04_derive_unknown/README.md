# 04 derive_unknown

Positive-fire for **MT4041 DERIVE_UNKNOWN**. Spec v1.0-RC §19 (derive).

`#[derive(Foo)]` names a derive that the v0.3+ checker doesn't
recognize (only Copy/Hash/Eq/Sendable). The check reports MT4041.
