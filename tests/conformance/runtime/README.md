# Runtime conformance corpus

Each subdirectory contains:

- `input.sd` — Stardust source to run through `sdust run`
- `expected.txt` — exact stdout the interpreter must produce
  (trailing newline preserved). The special string `__TRAP__` means
  the test expects the interpreter to trap (any trap; non-zero exit).
- `expected.code` (optional) — single line with the expected exit
  code. Defaults to `0`. (Slice 6 leaves this unused; reserved.)

The test harness lives in `crates/sdust-driver/tests/conformance_runtime.rs`
and discovers every directory under `tests/conformance/runtime/`.
