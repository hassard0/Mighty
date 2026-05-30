# Mighty v0.41 — Release Notes

**Tag:** `v0.41.0`
**Date:** 2026-05-30
**Status:** SHIPPED — honest correctness release.

**Headline:** honest correctness release — IDE-dogfooding loop pays
down 5 P0 compiler bugs.

## Summary

v0.41 fixes 5 P0 compiler bugs surfaced by dogfooding Mighty IDE: struct
field reads collapsed to field 0, package-level module resolution was
missing from `mty test` / `mty check`, the native (Cranelift) backend
diverged from the interpreter on 5 Option/aggregate shapes, manifest-driven
native linking was unspecified, and top-level `const` evaluated to its
type's default at runtime. No new stdlib surface this release — every
track is a correctness fix or a tools/process upgrade.

## What's fixed

- **L15 — struct field reads (T1).** `mty-ir` lower had two bugs:
  read path's multi-segment resolver fell back to field 0; assign
  path stored to a fresh temp instead of the field's slot. 10 tests in
  `crates/mty-ir/tests/struct_fields.rs`.
- **L13 — package-level module resolution (T2).** `mty test` /
  `mty check` / `mty run` on a single PATH now assemble every
  `src/**/*.mty` into one HIR Package before lower/typecheck/run.
  New diagnostics MT2029 UNRESOLVED_MODULE + MT2030
  SYMBOL_NOT_IN_MODULE. 5 conformance tests.
- **L1 — 5 native-codegen parity gaps (T3).** All in Cranelift
  lowering, all caused segfaults under `mty build` / JIT while
  `mty run --legacy-interp` worked: (1) `v.get(i)` now returns a real
  `Option[T]` aggregate; (2) `v.pop()` same fix; (3) implicit arena
  push at `main` entry (so `let v = Vec.new()` without `arena {}`
  stops null-derefing); (4) `String.clear()` / `push_str()` stop
  routing through `Vec.*` (was reading the (ptr,len) pair as a Vec
  header and looping); (5) `stream.next()` on opaque receivers
  synthesises a real `None`. New examples conformance suite at
  `crates/mty-cli/tests/conformance_examples.rs` runs every example
  through both interp and JIT and diffs.
- **L2 — manifest-driven native linking (T4).** `mighty.toml` grows
  a `[build]` section: `native-libs`, `link-search`, `frameworks`,
  `link-args`. Linker-flavor detection (gnu/msvc) with
  `MTY_LINKER_FLAVOR` override + MSVC arg rewrite table. New
  `crates/mty-driver/src/link_flavor.rs` (245 lines, 11 unit tests) +
  14 integration tests + new `examples/extern_c_with_manifest/`.
- **L16 — top-level `const` (T6).** Wired through HIR → DefMap →
  resolve → typecheck → IR lower with inline-at-use.
- **L14 — alloc-effect diagnostic (T6).** Per-effect hint + docs link.

## What's new

- **T5 — surface audit + CI gate.** Hover catalog 565 → 388 honest
  entries. Whole modules deleted (`collections`, `iter`, `error`,
  `process`). 38 entries kept as concept-docs via a new `# concept-doc`
  marker. New `crates/mty-doc/src/surface_audit.rs` (880 lines).
  `mty doc check --check-surface` extended; CI gate added in
  `.github/workflows/ci.yml`.
- **T4 — manifest linking schema.** See L2 above — same change is
  also a tools win for agents.
- **T6 — pre-push hook honors `CARGO_TARGET_DIR`.** Was hardcoded
  to `target/release/mty.exe`, breaking every parallel-worktree
  track that set a per-worktree target dir.

## Known issues / v0.42 priorities

- **L28 (P0):** native `mty build` Vec growth still broken under
  capture-rebind `v = v.push(x)`; works under interp.
- **L21 (P0):** Vec param read in nested loops SIGSEGVs under native
  codegen — likely same liveness/spill family as L28.
- **L19 (P0):** `expr as T` numeric casts don't actually convert
  (Char cast shipped v0.40 T3; int/float widening still broken).
- **L20 (P1):** `(a)(b)` parses as call → MT2008 not callable.
- **L23 (P1):** native `log(...)` only takes string literals.
- **L18 (P1):** `std.fs` is Rust-internal, not Mighty-callable.
- **L26 (sharp):** `mty fmt` no-op stub on `.mty`; DESTRUCTIVE on
  non-`.mty` input (truncates).
- **L22 (P2):** type-error spans collapse to enclosing fn start;
  ANSI always on; check ≠ full lint.
- **Pending:** #253 SWE-bench numbers, #262 BOLT training profile path.

## Acknowledgements

Every fix this release was surfaced by dogfooding **Mighty IDE**
(C:\Users\ihass\mighty-ide, MIT) — a native GPU IDE written in Mighty
itself. Living lessons log at
`mighty-ide/docs/mighty-language-lessons.md`; v0.41 is the first
release that consumes it as a triage queue.
