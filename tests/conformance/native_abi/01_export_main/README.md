# native_abi/01_export_main

Pins the simplest native-ABI shape: an integer-return fn marked
`export c` MUST be linkable from C as a C-calling-convention symbol
with the same name. The harness calls `_add(40, 2)` and expects 42.

Spec §29.1 (C-ABI exports).
