# type_inference

Spec §6.4 + slice-3 design. Exercises the bidirectional + HM-lite
inference engine: local `let` inference, generic function inference,
struct/variant inference, and the `T!E` (Result) sugar.

Each sub-case is `command.txt = check` and expects exit code 0 (no
errors). A successful `mty check` proves the inference machinery
resolved every binding to a concrete type without manual annotation.

## Cases

- `01_local_let_infer` — let-bound integer literal inferred without annotation.
- `02_struct_field_infer` — struct constructor inferred from field literals.
- `03_generic_id_infer` — generic `fn id<T>(x: T) -> T` inferred at call site.
- `04_result_sugar_infer` — `T!E` Result sugar resolves to `Result<T, E>`.
- `05_match_arm_infer` — match expression infers result type from arms.
