# native_abi/

v0.20-populated. Cases pinning the native (C-ABI) backend's
import/export shape (spec §29, `crates/mty-codegen-cranelift`,
`docs/spec/conformance.md` §6.5).

The contract under test: when an implementation lowers a Mighty
function with the `export c` attribute, the emitted object's
exported symbol MUST:

1. Use the C calling convention for the target platform.
2. Accept a leading `*mut u8` cabi_realloc pointer for any return
   value that does not fit in a single register (e.g. `Str`, struct).
3. Match the canonical mangled symbol the linker can resolve from a
   plain C harness.

Each case ships:

```
NN_case_name/
  input.mty                       — the source program
  command.txt                     — `check` (asserts the program
                                    parses + type-checks)
  expected_diagnostics.txt        — usually empty (positive case)
  expected_exit_code.txt          — 0
  harness.c                       — C source that links against the
                                    emitted .o and exercises the
                                    exported entry point
  expected_harness_exit.txt       — exit code the C harness MUST
                                    produce when run after linking
  README.md                       — what the case proves
```

The conformance_full harness asserts the program parses + type-checks.
The link-and-run step lives in `crates/mty-driver/tests/native_abi.rs`
(v0.20 stretch) and `crates/mty-codegen-cranelift/tests/` (slice-8).

## Cases

| Case | Property under test |
|------|---------------------|
| `01_export_main` | `export c fn _main` — symbol is exported and callable |
| `02_string_return` | `Str` return triggers cabi_realloc copy-out |
| `03_struct_return` | struct return passed via cabi_realloc pointer |
| `04_callback` | accepts a `fn(I32) -> I32` pointer arg |
