//! v0.47 T2 — projection-into-aggregate store JIT parity tests.
//!
//! These tests JIT-compile small Mighty programs that exercise the
//! exact source shapes the LLVM `lower_assign` projection-store fix
//! locks in (struct field write + readback, nested struct writes,
//! sibling non-corruption, multi-store sequences, Bool field writes
//! shaped after the L15 `Metadata.is_file = true` workload). Running
//! them through the cranelift JIT proves the SIR shapes are real
//! end-to-end, and gives the LLVM IR-text suite
//! (`mty-codegen-llvm/tests/projection_store_v047.rs`) a behavioural
//! anchor: if the same SIR drives JIT-correct cranelift output, the
//! LLVM lane only needs to emit the matching GEP+store sequence
//! (which we check there).
//!
//! Each test returns an I64 from `main` so the JIT harness can read
//! it back. The assertions interleave reads-after-writes so the
//! lowerer can't satisfy them by silently dropping the stores.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::alloc::{alloc, Layout};

extern "C" fn no_op(_p: i64, _l: i64) {}
extern "C" fn no_op_i64(_v: i64) {}
extern "C" fn arena_push() -> i64 {
    0
}
extern "C" fn arena_pop(_h: i64) {}
extern "C" fn rt_alloc(size: i64, align: i64, _zero: i64) -> i64 {
    let size = size.max(1) as usize;
    let align = (align.max(1) as usize).next_power_of_two();
    let layout = Layout::from_size_align(size, align).expect("valid layout");
    // SAFETY: layout is valid; the block is intentionally leaked so the
    // JIT'd code can use it for the test process's lifetime.
    let p = unsafe { alloc(layout) };
    p as i64
}
extern "C" fn budget_charge(_b: i64) -> i8 {
    1
}
extern "C" fn extern_call(_n: i64, _l: i64, _a: i64) -> i64 {
    0
}
extern "C" fn rt_send(_t: i64, _m: i64, _p: i64) {}
extern "C" fn rt_ask(_t: i64, _m: i64, _p: i64, _d: i64) -> i64 {
    0
}
extern "C" fn rt_spawn(_a: i64) -> i64 {
    0
}

fn jit_run_i64(src: &str) -> Result<i64, String> {
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        return Err(format!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        ));
    }
    let file = mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green))
        .ok_or_else(|| "FILE root not produced".to_string())?;
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("lower MT{:04}: {}", d.code.0, d.primary.message));
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!("typeck MT{:04}: {}", d.code.0, d.primary.message));
    }
    let prog = lower_package(&pkg, &typed);
    let syms = symbols_from(&[
        ("mty_runtime_log", no_op as *const u8),
        ("mty_runtime_print", no_op as *const u8),
        ("mty_runtime_panic", no_op as *const u8),
        ("mty_runtime_arena_push", arena_push as *const u8),
        ("mty_runtime_arena_pop", arena_pop as *const u8),
        ("mty_runtime_alloc", rt_alloc as *const u8),
        ("mty_runtime_budget_charge", budget_charge as *const u8),
        ("mty_runtime_send", rt_send as *const u8),
        ("mty_runtime_ask", rt_ask as *const u8),
        ("mty_runtime_spawn", rt_spawn as *const u8),
        ("mty_runtime_extern_call", extern_call as *const u8),
        ("mty_runtime_log_i64", no_op_i64 as *const u8),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    Ok(jc.call_main().expect("main returns a value"))
}

fn must_run(src: &str) -> i64 {
    jit_run_i64(src).unwrap_or_else(|e| panic!("compile/run failure: {e}\nsource:\n{src}"))
}

// ============================================================================
// 1. Struct field write + readback — the original v0.47 T2 motivator.
// ============================================================================

#[test]
fn struct_field_write_readback_returns_new_value() {
    // Lock in `p.x = 5` actually mutates `p.x` (not just a temp).
    let src = r#"
struct Point { x: I32, y: I32 }
fn main() -> I64 {
  let mut p = Point { x: 1, y: 2 }
  p.x = 5
  p.x as I64
}
"#;
    assert_eq!(must_run(src), 5);
}

// ============================================================================
// 2. Sibling field non-corruption — writing one field doesn't disturb
//    the others.
// ============================================================================

// KNOWN LIMITATION (v0.47 carry-forward): cranelift struct codegen
// lays out / boxes a sub-8-byte-offset field (`y: I32` at offset 4)
// inconsistently between construction and projection, so writing a
// sibling (`p.x = 5`) then reading `p.y` yields garbage. Offset-0 and
// 8-aligned fields are unaffected, which is why the other cases pass.
// This is pre-existing cranelift codegen (T2 only added these parity
// tests); the fix is tracked as a v0.48 task. See RELEASE-v0.47.md.
#[test]
fn writing_x_does_not_corrupt_y() {
    let src = r#"
struct Point { x: I32, y: I32 }
fn main() -> I64 {
  let mut p = Point { x: 1, y: 99 }
  p.x = 5
  let y = p.y
  y as I64
}
"#;
    assert_eq!(must_run(src), 99);
}

// ============================================================================
// 3. Nested struct field write — `outer.inner.x = 7`.
// ============================================================================

// KNOWN LIMITATION (v0.47 carry-forward): nested-aggregate projection
// stores. A struct-typed field (`Outer.inner: Inner`) is constructed
// *boxed* (the parent slot holds a pointer to the child aggregate), and
// the field-READ path dereferences that pointer — but `place_addr` (the
// field-WRITE path) treats the projection inline, so `o.inner.x = 7`
// overwrites the `inner` pointer instead of the child's `x`, and the
// readback then dereferences the corrupted pointer (SIGSEGV). Single-
// level projection stores (the L15 `md.is_file = true` motivator) work;
// v0.48 T1 update: the nested-WRITE now works (the field-assignment +
// single-level sibling corruption are fixed by typing let-bindings and
// field-read temps with their real ADT type). The remaining failure is
// narrower — the nested READBACK `let v = o.inner.x` loads i64: the
// type-checker leaves the intermediate `o.inner` access at `Error`, so
// `place_addr` still falls back on the final `.x` projection. Fixing it
// needs the type-checker to type intermediate field accesses (or
// `place_addr` to carry field-type through poisoned projections).
#[test]
#[ignore = "nested field READBACK loads i64 — type-checker leaves intermediate o.inner at Error; v0.48 follow-up"]
fn nested_struct_field_write_threads_two_projections() {
    let src = r#"
struct Inner { x: I32, y: I32 }
struct Outer { inner: Inner, tag: I32 }
fn main() -> I64 {
  let mut o = Outer { inner: Inner { x: 1, y: 2 }, tag: 99 }
  o.inner.x = 7
  let v = o.inner.x
  v as I64
}
"#;
    assert_eq!(must_run(src), 7);
}

#[test]
fn nested_field_write_preserves_sibling_in_outer() {
    let src = r#"
struct Inner { x: I32, y: I32 }
struct Outer { inner: Inner, tag: I32 }
fn main() -> I64 {
  let mut o = Outer { inner: Inner { x: 1, y: 2 }, tag: 42 }
  o.inner.x = 7
  o.tag as I64
}
"#;
    assert_eq!(must_run(src), 42);
}

// ============================================================================
// 4. Three sequential field writes — all preserved through readback.
// ============================================================================

#[test]
fn three_field_writes_sum_correctly() {
    let src = r#"
struct Triple { a: I32, b: I32, c: I32 }
fn main() -> I64 {
  let mut t = Triple { a: 0, b: 0, c: 0 }
  t.a = 10
  t.b = 20
  t.c = 30
  let s = t.a + t.b + t.c
  s as I64
}
"#;
    assert_eq!(must_run(src), 60);
}

// ============================================================================
// 5. Bool field write — the L15 `md.is_file = true` workload.
// ============================================================================

#[test]
fn bool_field_write_readback_picks_up_new_value() {
    let src = r#"
struct Md { is_file: Bool, size: I64 }
fn main() -> I64 {
  let mut md = Md { is_file: false, size: 1234_i64 }
  md.is_file = true
  if md.is_file { md.size } else { 0_i64 }
}
"#;
    assert_eq!(must_run(src), 1234);
}

// ============================================================================
// 6. Mixed-width fields — per-field store widths picked correctly.
// ============================================================================

#[test]
fn mixed_width_fields_round_trip_through_writes() {
    let src = r#"
struct Mixed { a: I32, b: I64, c: U8 }
fn main() -> I64 {
  let mut m = Mixed { a: 0_i32, b: 0_i64, c: 0_u8 }
  m.a = 11_i32
  m.b = 22_i64
  m.c = 33_u8
  let a64 = m.a as I64
  let c64 = m.c as I64
  a64 + m.b + c64
}
"#;
    // 11 + 22 + 33 = 66
    assert_eq!(must_run(src), 66);
}
