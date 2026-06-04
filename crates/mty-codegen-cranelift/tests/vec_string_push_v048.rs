//! #297 layer 4 — Vec-of-String element-push JIT parity.
//!
//! A `Vec[String]` element is a 16-byte `(ptr@+0, len@+8)` pair, but a
//! pushed String operand is NOT a uniform slot address: a string literal
//! materialises as an inline `(ptr,len)` pair (no backing slot), so the
//! old `emit_vec_push` path — which memcpy'd `elem_size` bytes treating
//! the operand as the source aggregate's ADDRESS — read past the
//! literal's bytes and corrupted the heap (SIGSEGV). The fix routes
//! String/Str/Bytes elements through `string_pair` (correct for both the
//! literal fast-path and the slot-backed dynamic case) and stores both
//! halves explicitly.
//!
//! These tests push String *literals* (a valid String operand that does
//! not depend on the still-unimplemented native `String.from_str` /
//! `String.len()` surface) and read back the Vec length, which exercises
//! the header + grow + element-store path end to end without crashing.

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
    // SAFETY: valid layout; intentionally leaked for the test's lifetime.
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

/// Pushing String literals into a `Vec[String]` must not corrupt the
/// heap — pre-fix this SIGSEGV'd while sizing/storing the 16-byte
/// element. The Vec length is the observable contract here.
#[test]
fn vec_of_string_literal_push_counts_correctly() {
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[String] = Vec.new()
  v.push("alpha")
  v.push("beta")
  v.push("gamma")
  v.len() as I64
}
"#;
    assert_eq!(must_run(src), 3);
}

/// Growth across the initial capacity boundary (cap 0 → 4 → 8) with
/// 16-byte aggregate elements: the grow-buffer must be sized by the real
/// element width and the live prefix copied byte-granularly.
#[test]
fn vec_of_string_grows_past_capacity() {
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[String] = Vec.new()
  v.push("a")
  v.push("b")
  v.push("c")
  v.push("d")
  v.push("e")
  v.len() as I64
}
"#;
    assert_eq!(must_run(src), 5);
}

/// Push-only inference (#297 layer 1): a `Vec` whose element is pinned
/// only by `.push(x)` (no annotation) still sizes its scalar slots
/// correctly and counts right.
#[test]
fn vec_push_only_inference_counts_correctly() {
    let src = r#"
fn main() -> I64 {
  let mut v = Vec.new()
  v.push(10_u32)
  v.push(20_u32)
  v.push(30_u32)
  v.len() as I64
}
"#;
    assert_eq!(must_run(src), 3);
}
