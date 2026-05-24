# 03 generic_id_infer

Generic identity function called at two distinct concrete types. The
inference engine must unify each call site separately
(`id::<I64>(7)` and `id::<Str>("hi")`). Spec §6.4 + §6.3 (generics).
