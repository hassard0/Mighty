# Mighty v0.46 Release Notes

**Tag:** `v0.46.0`
**Date:** 2026-06-02
**Status:** SHIPPED — official runtime ABI + extern c strings + LSP structured results.

**Headline:** Mighty v0.46 — official runtime ABI + extern c strings
+ LSP structured results (marquee: shim-less app linking)

## Summary

v0.46 T1 turns the runtime ABI surface into a first-class artifact:
downstream `extern c` callers link against a generated header +
per-platform staticlib tarball instead of hand-mirroring the
`mty_runtime_*` symbol list and discovering drift at link time. T3
expands the extern c boundary so Mighty strings cross as the
`(ptr, len)` pair the C side already expects, removing the
staging-buffer dance every agent-authored binding had to
re-implement. T5 migrates five LSP surfaces to LSP structured-result
types, so VS Code / Helix / Zed clients drop the ariadne text
parsing they were running on hover bodies and signatureHelp args.

## Shipped

- **PR #29 (T3) — `(ptr, len)` FFI for Mighty strings (L52).**
  `extern c fn f(s: Str)` lowers as the (ptr, len) pair the C ABI
  expects, transparently expanded at every call site. Removes the
  staging-buffer + `to_c_str` boilerplate L52 surfaced in agent-built
  bindings; dynamic-Str fallback preserved for the v0.42 T4 LLVM
  limitation. 17 new tests.
- **PR #30 (T2) — `mty build` exit code + `MTY_LINKER` Windows path
  support (L50).** `mty build` now exits non-zero on diagnostic-fail
  (parity with `mty check`), and `MTY_LINKER` accepts
  Windows-absolute paths (`C:\...`, quoted spaces) without
  misparsing them as args. 5 new tests.
- **PR #31 (T1) — official runtime ABI artifact (marquee).**
  Publishes the `mty_runtime_*` C-ABI surface as a first-class
  artifact: generated C header at
  `crates/mty-runtime/include/mty_runtime_abi.h` (regen on every
  build by `build.rs` parsing `src/codegen_abi.rs`); `mty-runtime`
  ships as `staticlib + rlib`; `release.yml` packages
  `mty-runtime-abi-<version>-<triple>.tar.gz` per platform; `mty abi
  {list, version, header}` lets tooling verify against the ground
  truth; drift gate in `tests/runtime_abi_header.rs` fails if the
  in-tree header lags. 13 new tests.
- **PR #32 (T5) — LSP structured-result surfaces (5).**
  `signatureHelp` ships real param names (no more `arg0/arg1`);
  `definition` returns `LocationLink` for cross-file origin context;
  `completion` splits `detail`/`documentation` into first-class
  fields; `hover` returns `MarkedString[]` so signature / doc /
  capability / taint sections can be styled independently; `mty
  agent lsp` reports each via structured JSON. 71/71 mty-lsp + 45/45
  mty-cli agent tests.
- **PR #33 (T4) — `std.fs.read_dir` iterator + Metadata field
  projection.** Closes v0.45 T1 deferrals. `read_dir` returns an
  iterator handle so callers stream entries instead of materialising
  a whole-Vec snapshot; `Metadata` exposes field-projection for
  `size`/`mtime`/`kind`/`permissions`. `read_dir_lines` deprecated
  (still works, warns). 12 new tests.

## Carry-forward priorities

- LLVM dynamic-Str path (v0.42 T4 limitation, surfaced again under
  T3 — `(ptr, len)` lowering needs the LLVM backend's dynamic case
  before non-literal Str args work on `--features llvm`).
- Per-symbol stability tier markers (`@since` / `@deprecated`) in
  the runtime ABI header (T1 follow-up so downstream linkers can
  pin a minimum version).
- Numeric `MAJOR/MINOR/PATCH` macros in the runtime ABI header (T1
  follow-up so C preprocessor checks beat the string compare).
- Mutable OUT params (`mut Vec[U8]` caller-allocated buffers) for
  FFI (T3 deferral; only the in-Mighty fast path lands in v0.46).
- LSP `textDocument/rename` `documentChanges` migration (T5
  deferral; still uses legacy `WorkspaceEdit.changes`).
- LSP `semanticTokens` delta encoding (T5 deferral).
- DirIter auto-Drop (T4 deferral; explicit `close()` for now).
- LLVM `lower_assign` projection-into-aggregate (resurfaces in T4
  Metadata field access on the LLVM backend).
- Pending tasks #253 (SWE-bench), #262 (BOLT training profile path).

## Acknowledgements

Driven by the mighty-ide dogfooding lessons log
(`mighty-ide/docs/mighty-language-lessons.md`) — L50/L51/L52 from IDE
FFI integration friction.
