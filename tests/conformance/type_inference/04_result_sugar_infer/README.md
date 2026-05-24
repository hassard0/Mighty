# 04 result_sugar_infer

`I64!FooErr` is sugar for `Result<I64, FooErr>`. The inference engine
must resolve `Ok(42)` against the desugared Result and `match` arms
must agree on the union arm type. Spec §6.4 + §6.5 (Result sugar) +
amendment A12 (Result-as-stdlib-ADT).
