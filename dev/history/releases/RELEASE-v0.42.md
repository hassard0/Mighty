# Mighty v0.42 — Release Notes

**Tag:** `v0.42.0`
**Date:** 2026-05-30
**Status:** SHIPPED — IDE-blocker closure release.

**Headline:** IDE-blocking codegen + diagnostics + tooling polish;
L19/L20/L22/L23/L26 closed, L28/L21 verified-fixed-and-locked.

## Summary

v0.42 closes the last 5 IDE-blocking lessons from the dogfooding queue.
Numeric `as` casts actually convert; paren-juxtaposition no longer
mis-parses as a call; native `log()` accepts computed args; `mty fmt`
stopped being destructive; type-error spans + NO_COLOR + parse/name
errors all polished in `mty check`. T1 was the planned marquee but the
underlying Cranelift Vec bug was discovered to be already fixed by
v0.41 T3's auto-arena-push at `main` entry — T1 ports the same fix to
LLVM and locks it in with 10 regression tests so the IDE can finally
re-home its editor TextModel.

## What's fixed (correctness)

- **L28 + L21 (T1).** Verified fixed by v0.41 T3's auto-arena-push;
  T1 ports the same fix into the LLVM backend and pins it down with
  6 JIT regression tests + 4 native-binary end-to-end tests in
  `crates/mty-codegen-cranelift/tests/vec_liveness_v042.rs` and
  `crates/mty-driver/tests/vec_liveness_native_v042.rs`, new
  `examples/44_vec_growth_in_loop.mty`, and conformance FLOOR bumped
  from 27 → 28 so any future regression breaks CI.
- **L19 (T2).** `expr as T` numeric casts now actually convert across
  all four back-ends. Cranelift uses
  `sextend`/`uextend`/`ireduce`/`fcvt_from_*`/`fcvt_to_*_sat`/
  `fpromote`/`fdemote`; LLVM uses `sext`/`zext`/`trunc`/
  `sitofp`/`uitofp` + `llvm.fpto[su]i.sat`; wasm uses the matching
  `i64.extend_i32_*` / `i32.wrap_i64` / `f*.convert_i*_*` /
  `i*.trunc_sat_f*_*` / `f64.promote_f32` / `f32.demote_f64`; interp
  follows the same policy. Float→Int overflow is saturating (NaN→0,
  ±inf clamp). 14 tests in `crates/mty-ir/tests/v042_numeric_casts.rs`;
  new `docs/reference/casts.md §v0.42 T2`.
- **L20 (T3).** `(a + b)(c)` now surfaces a clear MT0001 parse error
  instead of MT2008 "not callable". The Pratt postfix-`(` rule
  threads a `PrimaryShape::{Callable, NonCallable}` token through
  expression parsing so only callable-shaped primaries accept a
  following argument list. Edge cases preserved: `(f)(x)`, `g()()`,
  `(|| 1)()`, `obj.method(x)`, `(p as fn(...) -> ...)(args)`. 12
  tests in `crates/mty-syntax/tests/parse_exprs.rs`.
- **L23 (T4).** Native `log(...)` now lowers computed args via a typed
  runtime ABI (`mty_runtime_log_i32`/`_i64`/`_f32`/`_f64`/`_usize`).
  Scalar `to_str()` methods landed on the stdlib surface; String
  concat (`+`) operator landed alongside. Example 05 updated. Tests:
  `crates/mty-codegen-cranelift/tests/{to_str,typed_log}_v042_t4.rs`,
  `crates/mty-driver/tests/typed_log_v042_t4.rs`,
  `crates/mty-ir/tests/to_str_v042_t4.rs`.
- **L22 (T6, three sub-fixes).** (a) Type-error diagnostics carry real
  expression spans instead of collapsing to the enclosing fn start.
  (b) `NO_COLOR=1` and `TERM=dumb` are honored. (c) `mty check`
  widened to surface parse + name-resolution errors. E2E coverage in
  `crates/mty-cli/tests/cmd_check.rs`.
- **L26 partial (T5).** `mty fmt` (+ `--check`/`--stdin`) refuses
  non-.mty extensions, parse-failed inputs, and empty-tree-on-
  non-trivial-input. Destructive truncation eliminated. 10 tests +
  agent surface; all 65 existing `.mty` files still pass `--check`.

## What's new (tools / process)

- Examples conformance FLOOR 27 → 28 alongside `44_vec_growth_in_loop.mty`.
- Scalar `to_str()` + String `+` on the stdlib surface.
- `NO_COLOR` and `TERM=dumb` honored by the ariadne renderer.

## Known issues / v0.43 priorities

- **L18 (P1):** `std.fs` still Rust-internal, not Mighty-callable.
- **L26 follow-up:** the actual formatter is still a no-op on `.mty`;
  v0.42 T5 only landed the safety pass. v0.43 picks up the formatter
  proper once the 65+ pre-push-gated files have a reformat path.
- **Pending:** #253 SWE-bench Verified publishing; #262 BOLT
  training-profile path; new lessons added to the IDE log mid-cycle.

## Acknowledgements

Every fix this release was surfaced by dogfooding **Mighty IDE**
(C:\Users\ihass\mighty-ide, MIT). The living lessons log at
`mighty-ide/docs/mighty-language-lessons.md` continues to drive the
release queue; v0.42 closes the IDE-blocking entries from v0.41's
known-issues list.
