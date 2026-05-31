//! v0.42 T4 (L23 fix) — typed-arg `log(...)` regression suite.
//!
//! Pre-fix the cranelift backend's `log` lowering only accepted Str
//! operands. `log(42)` for an `I32` silently broke (the codegen
//! rejected the call or, worse, mis-lowered it to the string path
//! and read garbage). v0.42 T4 adds a typed runtime surface
//! (`mty_runtime_log_i32`/`_u32`/`_i64`/`_f64`/`_bool`/...) and the
//! codegen now dispatches on each operand's SIR type, so `log(42)`,
//! `log(3.5_f64)`, `log(true)`, and multi-arg `log("count=", n)`
//! all just work and produce the right bytes on stdout.
//!
//! Test strategy: the existing `dynamic_log.rs` harness intercepts
//! `mty_runtime_log` to capture (ptr, len) pairs. Here we also
//! intercept every typed variant and record the formatted value so we
//! can assert the codegen routed each call to the right symbol with
//! the right scalar value (or, for multi-arg, the right sequence of
//! `print_*` + `print_sep` + `print_newline` calls).

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::sync::Mutex;

/// Sequence of (symbol-name, formatted-value) captured during a run.
/// Order matters: it's how we verify the multi-arg lowering emits
/// `print_*, print_sep, print_*, ..., print_newline` in that order.
static CAPTURE: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());
static TEST_LOCK: Mutex<()> = Mutex::new(());

extern "C" fn cap_log_i32(v: i32) {
    CAPTURE.lock().unwrap().push(("log_i32", v.to_string()));
}
extern "C" fn cap_log_i64(v: i64) {
    CAPTURE.lock().unwrap().push(("log_i64", v.to_string()));
}
extern "C" fn cap_log_u32(v: u32) {
    CAPTURE.lock().unwrap().push(("log_u32", v.to_string()));
}
extern "C" fn cap_log_u64(v: u64) {
    CAPTURE.lock().unwrap().push(("log_u64", v.to_string()));
}
extern "C" fn cap_log_usize(v: i64) {
    CAPTURE
        .lock()
        .unwrap()
        .push(("log_usize", (v as u64).to_string()));
}
extern "C" fn cap_log_f32(v: f32) {
    CAPTURE.lock().unwrap().push(("log_f32", v.to_string()));
}
extern "C" fn cap_log_f64(v: f64) {
    CAPTURE.lock().unwrap().push(("log_f64", v.to_string()));
}
extern "C" fn cap_log_bool(v: i8) {
    CAPTURE
        .lock()
        .unwrap()
        .push(("log_bool", (v != 0).to_string()));
}
extern "C" fn cap_print_i32(v: i32) {
    CAPTURE.lock().unwrap().push(("print_i32", v.to_string()));
}
extern "C" fn cap_print_i64(v: i64) {
    CAPTURE.lock().unwrap().push(("print_i64", v.to_string()));
}
extern "C" fn cap_print_u32(v: u32) {
    CAPTURE.lock().unwrap().push(("print_u32", v.to_string()));
}
extern "C" fn cap_print_u64(v: u64) {
    CAPTURE.lock().unwrap().push(("print_u64", v.to_string()));
}
extern "C" fn cap_print_usize(v: i64) {
    CAPTURE
        .lock()
        .unwrap()
        .push(("print_usize", (v as u64).to_string()));
}
extern "C" fn cap_print_f32(v: f32) {
    CAPTURE.lock().unwrap().push(("print_f32", v.to_string()));
}
extern "C" fn cap_print_f64(v: f64) {
    CAPTURE.lock().unwrap().push(("print_f64", v.to_string()));
}
extern "C" fn cap_print_bool(v: i8) {
    CAPTURE
        .lock()
        .unwrap()
        .push(("print_bool", (v != 0).to_string()));
}
extern "C" fn cap_print_sep() {
    CAPTURE.lock().unwrap().push(("print_sep", String::new()));
}
extern "C" fn cap_print_newline() {
    CAPTURE
        .lock()
        .unwrap()
        .push(("print_newline", String::new()));
}
extern "C" fn cap_log(ptr: i64, len: i64) {
    let s = if ptr == 0 || len == 0 {
        String::new()
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    CAPTURE.lock().unwrap().push(("log_str", s));
}
extern "C" fn cap_print(ptr: i64, len: i64) {
    let s = if ptr == 0 || len == 0 {
        String::new()
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    CAPTURE.lock().unwrap().push(("print_str", s));
}
extern "C" fn no_op_2(_p: i64, _l: i64) {}
extern "C" fn no_op_3(_p: i64, _l: i64, _z: i64) -> i64 {
    0
}
extern "C" fn no_op_ret() -> i64 {
    0
}
extern "C" fn no_op_log_i64(_v: i64) {}

fn jit_run_capture(src: &str) -> Result<Vec<(&'static str, String)>, String> {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    CAPTURE.lock().unwrap().clear();
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
        // String log/print path
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
        // Typed log/print surface — v0.42 T4
        ("mty_runtime_log_i32", cap_log_i32 as *const u8),
        ("mty_runtime_log_i64", cap_log_i64 as *const u8),
        ("mty_runtime_log_u32", cap_log_u32 as *const u8),
        ("mty_runtime_log_u64", cap_log_u64 as *const u8),
        ("mty_runtime_log_usize", cap_log_usize as *const u8),
        ("mty_runtime_log_f32", cap_log_f32 as *const u8),
        ("mty_runtime_log_f64", cap_log_f64 as *const u8),
        ("mty_runtime_log_bool", cap_log_bool as *const u8),
        ("mty_runtime_print_i32", cap_print_i32 as *const u8),
        ("mty_runtime_print_i64", cap_print_i64 as *const u8),
        ("mty_runtime_print_u32", cap_print_u32 as *const u8),
        ("mty_runtime_print_u64", cap_print_u64 as *const u8),
        ("mty_runtime_print_usize", cap_print_usize as *const u8),
        ("mty_runtime_print_f32", cap_print_f32 as *const u8),
        ("mty_runtime_print_f64", cap_print_f64 as *const u8),
        ("mty_runtime_print_bool", cap_print_bool as *const u8),
        ("mty_runtime_print_sep", cap_print_sep as *const u8),
        ("mty_runtime_print_newline", cap_print_newline as *const u8),
        // fmt_* & str_concat are wired but not exercised by these
        // log-only tests; supply stubs so the jit symbol table is
        // complete.
        ("mty_runtime_fmt_i32", no_op_3 as *const u8),
        ("mty_runtime_fmt_i64_to_slot", no_op_3 as *const u8),
        ("mty_runtime_fmt_u32", no_op_3 as *const u8),
        ("mty_runtime_fmt_u64", no_op_3 as *const u8),
        ("mty_runtime_fmt_usize", no_op_3 as *const u8),
        ("mty_runtime_fmt_f32", no_op_3 as *const u8),
        ("mty_runtime_fmt_f64", no_op_3 as *const u8),
        ("mty_runtime_fmt_bool", no_op_3 as *const u8),
        ("mty_runtime_str_concat", no_op_3 as *const u8),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    let _ = jc.call_main();
    let out = CAPTURE.lock().unwrap().drain(..).collect();
    drop(jc);
    Ok(out)
}

fn must_capture(src: &str) -> Vec<(&'static str, String)> {
    jit_run_capture(src).unwrap_or_else(|e| panic!("compile/run failure: {e}\nsource:\n{src}"))
}

// ---- single-arg shapes -------------------------------------------------

#[test]
fn log_i32_literal_dispatches_to_log_i32() {
    let log = must_capture(r#"fn main() { log(42_i32) }"#);
    assert_eq!(log, vec![("log_i32", "42".to_string())]);
}

#[test]
fn log_default_int_literal_dispatches_to_log_i32() {
    // Unsuffixed integer literals default to I32.
    let log = must_capture(r#"fn main() { log(7) }"#);
    assert_eq!(log, vec![("log_i32", "7".to_string())]);
}

#[test]
fn log_i64_literal_dispatches_to_log_i64() {
    let log = must_capture(r#"fn main() { log(9000000000_i64) }"#);
    assert_eq!(log, vec![("log_i64", "9000000000".to_string())]);
}

#[test]
fn log_u32_literal_dispatches_to_log_u32() {
    let log = must_capture(r#"fn main() { log(42_u32) }"#);
    assert_eq!(log, vec![("log_u32", "42".to_string())]);
}

#[test]
fn log_u64_literal_dispatches_to_log_u64() {
    let log = must_capture(r#"fn main() { log(18000000000_u64) }"#);
    assert_eq!(log, vec![("log_u64", "18000000000".to_string())]);
}

#[test]
fn log_bool_literal_dispatches_to_log_bool() {
    let log = must_capture(r#"fn main() { log(true) }"#);
    assert_eq!(log, vec![("log_bool", "true".to_string())]);
}

#[test]
fn log_f64_literal_dispatches_to_log_f64() {
    let log = must_capture(r#"fn main() { log(3.5_f64) }"#);
    assert_eq!(log, vec![("log_f64", "3.5".to_string())]);
}

// ---- computed values ---------------------------------------------------

#[test]
fn log_of_local_i32_computed_value() {
    let src = r#"
        fn main() {
          let n: I32 = 1 + 2 + 3
          log(n)
        }
    "#;
    let log = must_capture(src);
    assert_eq!(log, vec![("log_i32", "6".to_string())]);
}

#[test]
fn log_of_negative_i32() {
    let src = r#"
        fn main() {
          let n: I32 = 0 - 7
          log(n)
        }
    "#;
    let log = must_capture(src);
    assert_eq!(log, vec![("log_i32", "-7".to_string())]);
}

#[test]
fn log_of_fn_return_i32() {
    let src = r#"
        fn count() -> I32 { 41 + 1 }
        fn main() {
          log(count())
        }
    "#;
    let log = must_capture(src);
    assert_eq!(log, vec![("log_i32", "42".to_string())]);
}

// ---- multi-arg log -----------------------------------------------------

#[test]
fn log_multi_arg_str_and_i32_uses_print_path_with_newline_terminator() {
    let src = r#"
        fn main() {
          let n: I32 = 42
          log("count=", n)
        }
    "#;
    let log = must_capture(src);
    // Expected sequence: print("count="), print_sep, print_i32(42),
    // print_newline.
    assert_eq!(
        log,
        vec![
            ("print_str", "count=".to_string()),
            ("print_sep", String::new()),
            ("print_i32", "42".to_string()),
            ("print_newline", String::new()),
        ]
    );
}

#[test]
fn log_multi_arg_three_values() {
    let src = r#"
        fn main() {
          let a: I32 = 1
          let b: I32 = 2
          let c: I32 = 3
          log(a, b, c)
        }
    "#;
    let log = must_capture(src);
    assert_eq!(
        log,
        vec![
            ("print_i32", "1".to_string()),
            ("print_sep", String::new()),
            ("print_i32", "2".to_string()),
            ("print_sep", String::new()),
            ("print_i32", "3".to_string()),
            ("print_newline", String::new()),
        ]
    );
}

// ---- print() shares the same path -------------------------------------

#[test]
fn print_of_i32_dispatches_to_print_i32_no_newline() {
    let src = r#"fn main() { print(42_i32) }"#;
    let log = must_capture(src);
    assert_eq!(log, vec![("print_i32", "42".to_string())]);
}

#[test]
fn print_multi_arg_no_trailing_newline() {
    let src = r#"
        fn main() {
          let n: I32 = 5
          print("n=", n)
        }
    "#;
    let log = must_capture(src);
    assert_eq!(
        log,
        vec![
            ("print_str", "n=".to_string()),
            ("print_sep", String::new()),
            ("print_i32", "5".to_string()),
        ]
    );
}

// ---- Regression: pre-fix this would have errored or mis-lowered -------

#[test]
fn log_of_computed_int_does_not_raise_unsupported() {
    let src = r#"
        fn double(x: I32) -> I32 { x + x }
        fn main() {
          log(double(21))
        }
    "#;
    let result = jit_run_capture(src);
    assert!(
        result.is_ok(),
        "log of computed int must compile; got {result:?}"
    );
    assert_eq!(
        result.unwrap(),
        vec![("log_i32", "42".to_string())],
        "value must reach the typed runtime symbol intact"
    );
}
