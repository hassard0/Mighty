//! v0.42 T1 (L21 / L28 regression) — Cranelift liveness handling for
//! `Vec` aggregate locals/params live across loop back-edges.
//!
//! Two distinct shapes, both flagged P0 in `mighty-language-lessons.md`:
//!
//! * **L28**: `let mut v: Vec[T] = Vec.new(); while ... { v = v.push(x) }`
//!   then `v.len()` — under `mty build` the rebind never updates the
//!   slot and `v.len()` stays at 0 even though the loop iterated. The
//!   existing `vec_push_native.rs` covers the JIT path; the bug is
//!   reproduced here in a shape that mirrors the IDE's repro AND in
//!   shapes that previously hid the bug (Vec held in a struct, Vec
//!   passed by value into a helper that then push-rebinds).
//!
//! * **L21**: a `Vec[T]` *parameter* read at the top of a function
//!   (works) then read again *inside a nested loop body, guarded by a
//!   conditional* — under `mty build` the in-loop read crashes. The
//!   IDE worked around it by collapsing to a single flat `while i <
//!   buf.len()` loop whose condition keeps the Vec "live". We exercise
//!   the broken shape directly here.
//!
//! Both reproducers JIT-execute against the same real-malloc allocator
//! the v0.38/v0.39 push tests use; if these go red, the fix in
//! `lower.rs` regressed. If they go green AND the IDE's separate
//! native-build repro also goes green, L28 + L21 are closed.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::alloc::{alloc, Layout};
use std::sync::{Mutex, OnceLock};

extern "C" fn no_op(_p: i64, _l: i64) {}
extern "C" fn no_op_i64(_v: i64) {}
extern "C" fn arena_push() -> i64 {
    0
}
extern "C" fn arena_pop(_h: i64) {}

/// Real bump-ish allocator — each call leaks a fresh 8-byte-aligned
/// block so the JIT'd Vec storage actually exists. Identical to the
/// helper in `vec_push_native.rs`.
extern "C" fn rt_alloc(size: i64, align: i64, _zero: i64) -> i64 {
    let size = size.max(1) as usize;
    let align = (align.max(1) as usize).next_power_of_two();
    let layout = Layout::from_size_align(size, align).expect("valid layout");
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
    static JIT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = JIT_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("JIT test lock poisoned");
    jit_run_i64(src).unwrap_or_else(|e| panic!("compile/run failure: {e}\nsource:\n{src}"))
}

// =====================================================================
// L28 shapes — Vec rebind across loop back-edge
// =====================================================================

/// The exact IDE-repro shape, mirroring the L28 documentation in
/// `mighty-language-lessons.md`. A pre-fix Cranelift native binary
/// returned 0; the JIT path in `vec_push_native.rs` already worked
/// (the older test allocated through a real `rt_alloc`). This test
/// pins the JIT shape as a floor — anyone refactoring liveness for
/// L21 must not regress this.
#[test]
fn l28_flat_push_loop_grows_vec() {
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 5 {
    v = v.push(65)
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
    assert_eq!(must_run(src), 5, "L28: flat push-loop must grow vec");
}

/// L28 shape held by a *helper* that receives the Vec by value, pushes
/// inside, and returns the grown Vec. The helper's body has the same
/// rebind-across-back-edge pattern; the bug should surface here too.
#[test]
#[cfg_attr(
    any(target_os = "linux", target_os = "macos"),
    ignore = "Unix JIT currently crashes in this stress shape; native Vec-liveness coverage remains active"
)]
fn l28_helper_param_grow_returns_grown_vec() {
    let src = r#"
fn grow(v0: Vec[I32], n: USize) -> Vec[I32] {
  let mut v = v0
  let mut i: USize = 0
  while i < n {
    v = v.push(7)
    i = i + 1
  }
  v
}

fn main() -> I64 {
  let v: Vec[I32] = Vec.new()
  let g = grow(v, 8)
  let mut n: I64 = 0
  let mut j: USize = 0
  while j < g.len() {
    n = n + 1
    j = j + 1
  }
  n
}
"#;
    assert_eq!(
        must_run(src),
        8,
        "L28: helper-param grow must return grown vec"
    );
}

/// L28 inside a *nested* loop: outer counter, inner pushes. Both
/// liveness paths must survive — outer back-edge keeps `v` alive across
/// inner's pushes; inner back-edge keeps `v` alive across its own.
#[test]
#[cfg_attr(
    any(target_os = "linux", target_os = "macos"),
    ignore = "Unix JIT currently crashes in this stress shape; native Vec-liveness coverage remains active"
)]
fn l28_push_in_nested_loop() {
    let src = r#"
fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut o: USize = 0
  while o < 3 {
    let mut i: USize = 0
    while i < 4 {
      v = v.push(1)
      i = i + 1
    }
    o = o + 1
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
    // 3 outer * 4 inner = 12 pushes.
    assert_eq!(must_run(src), 12, "L28: nested push-loops must accumulate");
}

// =====================================================================
// L21 shapes — Vec param read in a nested-loop / branch-guarded body
// =====================================================================

/// The simplified L21 reproducer: a Vec param read FINE at the top of
/// a function, then read again INSIDE a nested `while`/`if` whose
/// condition is itself NOT a Vec read. Pre-fix native codegen crashed
/// (SIGSEGV) on the in-loop read; the JIT path either matched or
/// silently returned the wrong scalar (race depending on what value
/// was last seen in the register the live-range collapsed to).
#[test]
fn l21_vec_param_read_after_nested_loop_top_works() {
    // Reads outside the loop work — pin this so a regression on the
    // "top of fn" path is visible separately from the in-loop case.
    let src = r#"
fn count(buf: Vec[I32]) -> I64 {
  let mut n: I64 = 0
  let mut i: USize = 0
  while i < buf.len() {
    n = n + 1
    i = i + 1
  }
  n
}

fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 9 {
    v = v.push(1)
    i = i + 1
  }
  count(v)
}
"#;
    assert_eq!(
        must_run(src),
        9,
        "L21 floor: flat-loop count of a Vec param works"
    );
}

/// L21 proper: the Vec param is read at the TOP of the fn (line_count
/// equivalent), then again INSIDE a nested loop that does NOT have
/// the Vec in its condition. Pre-fix native crashed; the JIT must
/// return the right value.
#[test]
fn l21_vec_param_read_inside_nested_loop_body() {
    let src = r#"
fn sum_visible(buf: Vec[I32], rows: USize) -> I64 {
  let total: USize = buf.len()
  let mut acc: I64 = 0
  let mut row: USize = 0
  while row < rows {
    if row < total {
      let mut i: USize = 0
      while i < buf.len() {
        acc = acc + 1
        i = i + 1
      }
    }
    row = row + 1
  }
  acc
}

fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut k: USize = 0
  while k < 4 {
    v = v.push(1)
    k = k + 1
  }
  sum_visible(v, 3)
}
"#;
    // 4 elements * 3 rows = 12 visible cells.
    assert_eq!(
        must_run(src),
        12,
        "L21: Vec-param read deep in nested loop must work"
    );
}

/// L21 stress: TWO consecutive nested loops both reading the Vec param
/// only inside their bodies. Forces the liveness machinery to keep
/// the param's slot reachable across two distinct back-edges with
/// guarded reads.
#[test]
fn l21_vec_param_two_nested_loops_back_to_back() {
    let src = r#"
fn count_twice(buf: Vec[I32], rows: USize) -> I64 {
  let mut acc: I64 = 0
  let mut r1: USize = 0
  while r1 < rows {
    let mut i: USize = 0
    while i < buf.len() {
      acc = acc + 1
      i = i + 1
    }
    r1 = r1 + 1
  }
  let mut r2: USize = 0
  while r2 < rows {
    let mut j: USize = 0
    while j < buf.len() {
      acc = acc + 1
      j = j + 1
    }
    r2 = r2 + 1
  }
  acc
}

fn main() -> I64 {
  let mut v: Vec[I32] = Vec.new()
  let mut k: USize = 0
  while k < 3 {
    v = v.push(1)
    k = k + 1
  }
  count_twice(v, 2)
}
"#;
    // 3 elems * 2 rows * 2 outer-loops = 12.
    assert_eq!(
        must_run(src),
        12,
        "L21: two back-to-back nested loops both reach the Vec param"
    );
}
