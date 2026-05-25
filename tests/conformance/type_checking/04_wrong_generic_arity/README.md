# 04 wrong_generic_arity

Positive-fire for **MT2004 WRONG_GENERIC_ARITY**. Spec v1.0-RC §6.2 (generics).

`Map[K, V]` requires two generic arguments. Supplying only one
(`Map[Str]`) triggers MT2004 at the type ref.
