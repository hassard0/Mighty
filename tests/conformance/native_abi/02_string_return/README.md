# native_abi/02_string_return

Pins the `Str`-return shape. A Mighty fn that returns a `Str` MUST
emit a C-callable entry that returns a `{ptr, len}` record (12
bytes here for "hello, world"). The allocation is owned by the
cabi_realloc-provided allocator; the caller MUST NOT free.

Spec §29.2 (cabi_realloc string convention).
