//! v0.38 (L28 regression) — native growable `Vec` codegen.
//!
//! Before this fix the cranelift backend stubbed every `Vec` op
//! (`Vec.new`, `.push`, `.len`, `.get`) through `mty_runtime_extern_call`,
//! which returns 0. A `v = v.push(x)` loop therefore iterated the right
//! number of times but `v.len()` came back 0 under `mty build` (it worked
//! under the interpreter). See `mighty-language-lessons.md` entry L28.
//!
//! These tests JIT-compile a Mighty program whose `main()` returns the
//! observable result as an I64, wiring a *real* malloc-backed
//! `mty_runtime_alloc` so the native Vec storage actually exists. The
//! key assertion: a push loop grows the vec to the expected length.

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

/// Real bump-ish allocator for the JIT tests: each call leaks a fresh
/// 8-byte-aligned block. Matches the runtime contract (returns the
/// pointer as i64, 0 on failure). Leaking is fine for a test process.
extern "C" fn rt_alloc(size: i64, align: i64, _zero: i64) -> i64 {
    let size = size.max(1) as usize;
    let align = (align.max(1) as usize).next_power_of_two();
    let layout = Layout::from_size_align(size, align).expect("valid layout");
    // SAFETY: layout is valid; the block is intentionally leaked so the
    // JIT'd code can use it for the duration of the test.
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

/// Count a Vec's length into an I64 by walking indices (mirrors the L28
/// repro's `vec_len_i32`, but returns I64 so the JIT main can hand it
/// back directly). Kept identical across the tests below.
const VEC_LEN_HELPER: &str = r#"
fn vlen(v: Vec[I32]) -> I64 {
  let mut n: I64 = 0
  let mut i: USize = 0
  while i < v.len() {
    n = n + 1
    i = i + 1
  }
  n
}
"#;

#[test]
fn push_loop_grows_vec_to_five() {
    // The exact L28 shape: a flat while loop that capture-rebinds
    // `v = v.push(x)`. Pre-fix this returned 0; the fix grows the vec.
    let src = format!(
        r#"{VEC_LEN_HELPER}
fn main() -> I64 {{
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 5 {{
    v = v.push(65)
    i = i + 1
  }}
  vlen(v)
}}
"#
    );
    assert_eq!(must_run(&src), 5, "L28: push loop must grow vec to 5");
}

#[test]
fn push_then_len_directly() {
    // `.len()` read straight off the receiver after a push loop, without
    // the index-walk helper. Exercises emit_vec_len.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 9 {
    v = v.push(7)
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 9);
}

#[test]
fn push_then_index_read_sum() {
    // Push distinct values, then sum them back via `v[i]` index reads.
    // Verifies the element storage round-trips (emit_vec_push store +
    // IndexRead load through the data pointer), not just the length.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 6 {
    v = v.push(10)
    i = i + 1
  }
  let mut sum: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    sum = sum + 10
    j = j + 1
  }
  sum
}
"#;
    // 6 pushes of 10 → length 6 → sum walk adds 10 six times = 60.
    assert_eq!(must_run(src), 60);
}

#[test]
fn empty_vec_has_zero_len() {
    let src = r#"
fn main() -> I64 {
  let v: Vec[I32] = Vec.new()
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 0);
}

#[test]
fn growth_across_multiple_reallocs() {
    // 100 pushes forces several capacity doublings (4 → 8 → 16 → ...),
    // exercising emit_memcpy_dynamic on the live prefix each time.
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 100 {
    v = v.push(1)
    i = i + 1
  }
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(must_run(src), 100);
}
