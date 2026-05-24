# Runtime conformance corpus

Each subdirectory contains:

- `input.sd` — Mighty source to run through `mty run`
- `expected.txt` — exact stdout the interpreter must produce
  (trailing newline preserved). The special string `__TRAP__` means
  the test expects the interpreter to trap (any trap; non-zero exit).
- `expected.code` (optional) — single line with the expected exit
  code. Defaults to `0`. (Slice 6 leaves this unused; reserved.)

The test harness lives in `crates/mty-driver/tests/conformance_runtime.rs`
and discovers every directory under `tests/conformance/runtime/`.
