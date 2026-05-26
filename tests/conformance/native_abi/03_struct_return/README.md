# native_abi/03_struct_return

Pins the struct-return shape: a Mighty struct that fits in two
i32 fields MUST be returned as a single by-value record matching
the C struct layout. Implementations that pack/pad struct fields
differently from the C ABI on the target platform fail this case.

Spec §29.3 (struct layout & return).
