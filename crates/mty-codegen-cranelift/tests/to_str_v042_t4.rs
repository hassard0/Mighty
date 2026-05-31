//! v0.42 T4 (L23 fix) — `n.to_str()` on scalar receivers + String
//! concat with `+` regression suite.
//!
//! Pre-fix the cranelift backend's `MethodCall` arm fell through to
//! the generic extern bridge for any non-Vec receiver, so
//! `42_i32.to_str()` returned the integer 0 — `log(s)` on the result
//! then printed nothing (or worse, the JIT segfaulted dereferencing
//! the address 0 as a Str aggregate). v0.42 T4 wires `to_str` /
//! `to_string` on scalars (`I8`/`I16`/`I32`/`I64`/`U*`/`F32`/`F64`/
//! `Bool`/`Char`) to typed `mty_runtime_fmt_*` runtime helpers, and
//! `String + String` to `mty_runtime_str_concat`.
//!
//! The harness intercepts `mty_runtime_log` to capture the resulting
//! (ptr, len) pair, then reads the bytes back through the captured
//! pointer — exactly the strategy used by `dynamic_log.rs`. Because
//! `to_str` rides through the per-process `FMT_STRINGS` interner the
//! pointer remains valid as long as the test holds a strong
//! reference to the interner, which the runtime crate guarantees by
//! storing each formatted byte buffer in a `Box<str>`.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::sync::Mutex;

static LOG_CAPTURE: Mutex<Vec<(i64, i64)>> = Mutex::new(Vec::new());
static TEST_LOCK: Mutex<()> = Mutex::new(());

extern "C" fn cap_log(ptr: i64, len: i64) {
    LOG_CAPTURE.lock().unwrap().push((ptr, len));
}
extern "C" fn cap_print(ptr: i64, len: i64) {
    LOG_CAPTURE.lock().unwrap().push((ptr, len));
}
extern "C" fn no_op_2(_p: i64, _l: i64) {}
extern "C" fn no_op_3(_p: i64, _l: i64, _z: i64) -> i64 {
    0
}
extern "C" fn no_op_ret() -> i64 {
    0
}
extern "C" fn no_op_log_i64(_v: i64) {}

// We DO want the real fmt_* + str_concat impls to run so the (ptr,
// len) we capture in `mty_runtime_log` points at real formatted
// bytes. Use the runtime's actual symbols (they live in
// `mty_runtime::codegen_abi` which is a normal Rust dependency of
// this test crate).
use mty_runtime::codegen_abi::{
    mty_runtime_fmt_bool, mty_runtime_fmt_f32, mty_runtime_fmt_f64, mty_runtime_fmt_i32,
    mty_runtime_fmt_i64_to_slot, mty_runtime_fmt_u32, mty_runtime_fmt_u64, mty_runtime_fmt_usize,
    mty_runtime_str_concat,
};

fn jit_run_collect_strs(src: &str) -> Result<Vec<String>, String> {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    LOG_CAPTURE.lock().unwrap().clear();
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        return Err(format!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        ));
    }
    let file = mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green))
        .ok_or_else(|| "FILE root".to_string())?;
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
        ("mty_runtime_log", cap_log as *const u8),
        ("mty_runtime_print", cap_print as *const u8),
        ("mty_runtime_panic", no_op_2 as *const u8),
        ("mty_runtime_arena_push", no_op_ret as *const u8),
        ("mty_runtime_arena_pop", no_op_log_i64 as *const u8),
        ("mty_runtime_alloc", no_op_3 as *const u8),
        ("mty_runtime_budget_charge", no_op_log_i64 as *const u8),
        ("mty_runtime_send", no_op_3 as *const u8),
        ("mty_runtime_ask", no_op_3 as *const u8),
        ("mty_runtime_spawn", no_op_log_i64 as *const u8),
        ("mty_runtime_extern_call", no_op_3 as *const u8),
        ("mty_runtime_log_i32", no_op_2 as *const u8),
        ("mty_runtime_log_i64", no_op_log_i64 as *const u8),
        ("mty_runtime_log_u32", no_op_2 as *const u8),
        ("mty_runtime_log_u64", no_op_log_i64 as *const u8),
        ("mty_runtime_log_usize", no_op_log_i64 as *const u8),
        ("mty_runtime_log_f32", no_op_2 as *const u8),
        ("mty_runtime_log_f64", no_op_log_i64 as *const u8),
        ("mty_runtime_log_bool", no_op_2 as *const u8),
        ("mty_runtime_print_i32", no_op_2 as *const u8),
        ("mty_runtime_print_i64", no_op_log_i64 as *const u8),
        ("mty_runtime_print_u32", no_op_2 as *const u8),
        ("mty_runtime_print_u64", no_op_log_i64 as *const u8),
        ("mty_runtime_print_usize", no_op_log_i64 as *const u8),
        ("mty_runtime_print_f32", no_op_2 as *const u8),
        ("mty_runtime_print_f64", no_op_log_i64 as *const u8),
        ("mty_runtime_print_bool", no_op_2 as *const u8),
        ("mty_runtime_print_sep", no_op_ret as *const u8),
        ("mty_runtime_print_newline", no_op_ret as *const u8),
        // Real fmt + concat (we want the formatted bytes to land in
        // the runtime's per-process interner so our `log` interceptor
        // sees real (ptr,len) pairs).
        ("mty_runtime_fmt_i32", mty_runtime_fmt_i32 as *const u8),
        (
            "mty_runtime_fmt_i64_to_slot",
            mty_runtime_fmt_i64_to_slot as *const u8,
        ),
        ("mty_runtime_fmt_u32", mty_runtime_fmt_u32 as *const u8),
        ("mty_runtime_fmt_u64", mty_runtime_fmt_u64 as *const u8),
        ("mty_runtime_fmt_usize", mty_runtime_fmt_usize as *const u8),
        ("mty_runtime_fmt_f32", mty_runtime_fmt_f32 as *const u8),
        ("mty_runtime_fmt_f64", mty_runtime_fmt_f64 as *const u8),
        ("mty_runtime_fmt_bool", mty_runtime_fmt_bool as *const u8),
        (
            "mty_runtime_str_concat",
            mty_runtime_str_concat as *const u8,
        ),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    let _ = jc.call_main();
    let mut out = Vec::new();
    for (ptr, len) in LOG_CAPTURE.lock().unwrap().drain(..) {
        if ptr == 0 || len == 0 {
            out.push(String::new());
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec();
        out.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    drop(jc);
    Ok(out)
}

fn must_strs(src: &str) -> Vec<String> {
    jit_run_collect_strs(src).unwrap_or_else(|e| panic!("compile/run failure: {e}"))
}

// ---- to_str on integer receivers --------------------------------------

#[test]
fn i32_to_str_renders_decimal() {
    let src = r#"
        fn main() {
          let n: I32 = 42
          log(n.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["42".to_string()]);
}

#[test]
fn negative_i32_to_str_renders_minus_sign() {
    let src = r#"
        fn main() {
          let n: I32 = 0 - 7
          log(n.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["-7".to_string()]);
}

#[test]
fn i64_to_str_works() {
    let src = r#"
        fn main() {
          let n: I64 = 9000000000_i64
          log(n.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["9000000000".to_string()]);
}

#[test]
fn u32_to_str_works() {
    let src = r#"
        fn main() {
          let n: U32 = 255_u32
          log(n.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["255".to_string()]);
}

#[test]
fn bool_to_str_works() {
    let src = r#"
        fn main() {
          let b = true
          log(b.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["true".to_string()]);
}

// ---- to_str on float receivers ----------------------------------------

#[test]
fn f32_to_str_renders_exact_value() {
    let src = r#"
        fn main() {
          let x: F32 = 3.5_f32
          log(x.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["3.5".to_string()]);
}

#[test]
fn f64_to_str_renders_exact_value() {
    let src = r#"
        fn main() {
          let x: F64 = 2.5_f64
          log(x.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["2.5".to_string()]);
}

// ---- to_string alias ---------------------------------------------------

#[test]
fn to_string_aliases_to_str() {
    let src = r#"
        fn main() {
          let n: I32 = 123
          log(n.to_string())
        }
    "#;
    assert_eq!(must_strs(src), vec!["123".to_string()]);
}

// ---- String + concat ---------------------------------------------------

#[test]
fn str_plus_str_concatenates() {
    let src = r#"
        fn main() {
          let a: Str = "hello, "
          let b: Str = "world"
          log(a + b)
        }
    "#;
    assert_eq!(must_strs(src), vec!["hello, world".to_string()]);
}

#[test]
fn str_plus_int_to_str_realistic_trace() {
    // The motivating use case from L23: `log("count=" + n.to_str())`.
    let src = r#"
        fn main() {
          let n: I32 = 42
          log("count=" + n.to_str())
        }
    "#;
    assert_eq!(must_strs(src), vec!["count=42".to_string()]);
}
