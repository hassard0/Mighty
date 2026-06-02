# Mighty v0.45 - Draft Release Notes

**Tag:** `v0.45.0`
**Date:** TBD
**Status:** DRAFT - agent-built app shipping ergonomics.

**Headline:** reduce the remaining shim code agents need when building
real apps in Mighty.

## Direction

v0.45 picks up the carry-forward work from v0.44: make host-backed
stdlib behavior available through native capability ABI paths, continue
the formatter rollout without destructive rewrites, and replace
stringly/scalar command plumbing with structured result surfaces that
agents can inspect, test, and regenerate safely.

## Candidate tracks

- **Native capability ABI:** move `std.fs` beyond interpreter fallback
  for JIT/AOT output while preserving capability checks and clear
  diagnostics. **T1 IN FLIGHT (codex/v045-fs-native).**
- **Formatter rollout:** expand syntax-aware formatting from safe
  top-level `const` items into more item kinds with regression tests
  for comments and whitespace preservation.
- **Agent command surfaces:** prefer structured result values over
  sentinel strings and mirrored IDs in CLI, LSP, and runtime control
  paths.
- **Release hygiene:** keep README, changelog, release notes, and
  `mty --version` aligned before every tag.

## In flight — T1 native std.fs

PR `codex/v045-fs-native` ships the marquee fix: every `std.fs.*`
method now lowers to a dedicated runtime ABI symbol on the cranelift
JIT/AOT and LLVM backends, so generated CLIs touch disk under
`mty build` without a Rust shim.

New runtime ABI surface (registered in
`mty_runtime::codegen_abi::symbol_table` and declared as cranelift /
LLVM imports):

| Symbol                              | Shape                                        |
|-------------------------------------|----------------------------------------------|
| `mty_runtime_fs_read`               | `(path_ptr, path_len, dst_24B_slot)`         |
| `mty_runtime_fs_read_to_string`     | `(path_ptr, path_len, dst_24B_slot)`         |
| `mty_runtime_fs_read_dir`           | `(path_ptr, path_len, dst_24B_slot)`         |
| `mty_runtime_fs_write`              | `(path, data) -> i32 (1=ok, -errno)`         |
| `mty_runtime_fs_write_string`       | `(path, str) -> i32`                         |
| `mty_runtime_fs_append`             | `(path, data) -> i32` (NEW alias)            |
| `mty_runtime_fs_exists`             | `(path) -> i32 (1/0)`                        |
| `mty_runtime_fs_metadata`           | `(path, dst_24B_slot) -> i32`                |
| `mty_runtime_fs_create_dir_all`     | `(path) -> i32`                              |
| `mty_runtime_fs_remove_file`        | `(path) -> i32`                              |
| `mty_runtime_fs_remove_dir_all`     | `(path) -> i32`                              |

Read/read_dir slot layout: `(ptr@+0, len@+8, ok_flag@+16)`. Metadata
slot layout: `(size:u64@+0, mtime_ms:i64@+8, is_file:i8@+16,
is_dir:i8@+17)`.

Capability check stays compile-time. `pub fn` callers missing
`effect fs` still trip MT4001 at typeck before the codegen runs.

## Validation plan

- Keep full CI green before every release tag.
- Add focused smoke tests for each native capability ABI expansion.
- Keep Mighty IDE lessons as the priority source for release-gate
  fixes.
