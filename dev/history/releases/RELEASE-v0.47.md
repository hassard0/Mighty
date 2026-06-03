# Mighty v0.47 Release Notes

**Tag:** `v0.47.0`
**Date:** 2026-06-02
**Status:** SHIPPED — clears the entire v0.46 carry-forward list:
mutable OUT-param FFI, LLVM projection stores, ABI version macros,
DirIter auto-Drop, and LSP `documentChanges`.

**Headline:** Mighty v0.47 — caller-allocated FFI buffers + LLVM
field-write codegen + resource auto-Drop (marquee: C-writes-back FFI
with no scalar-staging dance).

## Summary

v0.47 is a carry-forward sweep: every deferral the v0.46 notes listed
lands here. T1 gives `extern c` the caller-allocated OUT-buffer
pattern (`mut Vec[U8]`) the IDE had been faking with scalar staging.
T2 lifts the long-standing `Unsupported("llvm projection-store TBD")`
bail so struct-field writes compile on the LLVM backend at parity with
Cranelift. T3 turns the runtime ABI header into something a C
preprocessor can reason about (numeric `MAJOR/MINOR/PATCH` macros +
`@since`/`@deprecated` markers + a stability tier). T4 closes the
DirIter resource-leak gap with a generic scope-end auto-Drop and
retires the deprecated `read_dir_lines` shim. T5 migrates the LSP
`rename`/`codeAction` edits to the versioned `documentChanges` shape.

## Shipped

- **PR #35 (T1) — mutable OUT params for FFI (`mut Vec[U8]`, marquee).**
  `extern c fn f(out: mut Vec[U8])` lowers to a `(ptr, capacity,
  *len)` ABI triple at the call site so the C side writes back into a
  caller-allocated buffer and the Mighty `Vec.len` (VEC_LEN_OFF == 0)
  updates by reference. New parser/HIR/type surface for `mut` params
  (MT2031 rejects non-`Vec[U8]` mut), IR mut-flag plumbing, and
  Cranelift + LLVM call-site lowering. Removes the last scalar-staging
  workaround for C-writes-back FFI.
- **PR #36 (T2) — LLVM `lower_assign` projection-into-aggregate.**
  Projection LHS routes through `place_addr` + `store_scalar`;
  aggregate-constructing rvalues (`AdtInit`/`TupleInit`) write into a
  lazily-alloca'd backing buffer via `emit_adt_init_into` /
  `emit_tuple_init_into`, mirroring the Cranelift backend. Struct
  field-write + readback (the L15 metadata workload) now compiles on
  `--features llvm`. Projection-store tests on both backends.
- **PR #37 (T3) — ABI header stability markers + numeric version
  macros.** The generated runtime ABI header now emits
  `MTY_RUNTIME_ABI_VERSION_MAJOR/MINOR/PATCH` (+ a combined
  `_NUMBER`) for C-preprocessor compat checks, an
  `MTY_RUNTIME_ABI_STABILITY` tier ("experimental" pre-1.0), and
  per-symbol `@since` / `@deprecated` markers parsed from doc comments
  by `build.rs` and surfaced in `mty abi list`. A drift-gate warning
  flags any `#[no_mangle]` fn missing a `@since` tag.
- **PR #38 (T4) — std.fs DirIter scope-end auto-Drop + `read_dir_lines`
  removal.** ADTs registered in `DefMap::mty_drop_fns` get a
  `Stmt::Drop` injected before every fn-exit terminator by the IR
  post-pass `inject_auto_drop_stmts`; the backends lower it to the
  registered runtime close symbol (`DirIter ->
  mty_runtime_fs_dir_close`), so a forgotten `.close()` no longer
  leaks the handle. Explicit `.close()` + auto-Drop stays idempotent
  (handle zeroed after close, runtime no-ops on `0`). The deprecated
  `read_dir_lines` stdlib surface is removed (now a typecheck error
  with a replacement hint); the `mty_runtime_fs_read_dir` runtime
  symbol stays exported for v0.45-built-binary link compatibility.
- **PR #39 (T5) — LSP `WorkspaceEdit` documentChanges migration.**
  `rename` and `codeAction` emit the LSP-3.16 versioned
  `documentChanges` shape (`TextDocumentEdit` +
  `OptionalVersionedTextDocumentIdentifier`) when the client
  advertises `workspace.workspaceEdit.documentChanges`, falling back
  to the legacy `changes` map otherwise. Capability negotiated at
  `initialize`.

## Integration notes — two bugs caught before tagging

Honest-correctness: the T4 work shipped two latent defects that the
integration validation pass caught and fixed (both folded into PR #38):

1. **DirIter auto-Drop double-free (heap corruption).** The post-pass
   dropped *every* `DirIter`-typed local — including compiler
   temporaries that alias the same `i64` handle (the `read_dir` result
   temp, the `.next()`/`.close()` receiver copies) — closing the one
   `Box<DirIterState>` more than once (`STATUS_HEAP_CORRUPTION`). Fix:
   only drop `LocalSource::UserLet` bindings (the sole owner), and
   exclude bindings whose handle is moved out via a direct `Use(Move)`
   rebind. The fix lives in the shared IR pass, so it corrects
   Cranelift + LLVM + interpreter at once.
2. **`read_dir_lines` removal was incomplete.** The removed-name deny
   check was wired only into `synth_method_call`, but
   `std.fs.read_dir_lines(p)` lowers to a qualified *path* call whose
   callee resolves permissively (`std` is a Module → fresh var), so the
   removed name still typechecked. Fix: a shared `REMOVED_STD` table +
   `removed_std_path_diag` consulted early in `synth_call`.

## Carry-forward priorities

- LLVM dynamic-Str path (v0.42 T4 limitation; still pending for
  non-literal Str args on `--features llvm`).
- Cranelift/LLVM array-index projection stores (`Projection::Index`
  still `Unsupported` on both backends).
- DirIter auto-Drop on the panic-unwind path (Mighty aborts on panic
  today, so a panicking scope does not run Drop — revisit when
  unwinding lands).
- LSP `semanticTokens` delta encoding (still returns the full token
  array).
- Pending tasks #253 (SWE-bench), #262 (BOLT training profile path).

## Acknowledgements

Driven by the mighty-ide dogfooding lessons log
(`mighty-ide/docs/mighty-language-lessons.md`). The auto-Drop
double-free is recorded there as a resource-lifetime lesson: scope-end
Drop must target owning bindings, never aliasing temporaries.
