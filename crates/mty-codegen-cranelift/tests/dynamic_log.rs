//! v0.36 T1 — dynamic-`log` regression suite.
//!
//! Before the fix the cranelift backend's `string_pair` helper only
//! accepted `Operand::Const(Const::Str(_))` — i.e. literal strings.
//! Any non-literal Str operand (a local, a fn return, a struct field
//! load) raised `CodegenError::Unsupported`. This blocked
//! `log(format!(...))`, `log(s)` where `s` is a `let`-bound Str, etc.
//!
//! These tests JIT-compile programs that exercise the new path and
//! verify both: (1) the program compiles cleanly through the cranelift
//! backend, and (2) the runtime `log` symbol receives a (ptr, len)
//! pair matching the dynamic string.
//!
//! The captured-stdout harness uses a thread-local Mutex to record
//! every `log` invocation's (ptr, len) — the test then reads the
//! captured bytes from the recorded ptr address.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::sync::Mutex;

// We capture log() invocations in a process-global Mutex so we can
// verify the dynamic-string path actually delivers the right bytes.
static LOG_CAPTURE: Mutex<Vec<(i64, i64)>> = Mutex::new(Vec::new());
// Serialize tests since `capture_log` writes to the shared mutex.
// (cargo test runs in-process by default, so without this two
// concurrent tests would interleave their log entries.)
static TEST_LOCK: Mutex<()> = Mutex::new(());

extern "C" fn capture_log(ptr: i64, len: i64) {
    let mut g = LOG_CAPTURE.lock().unwrap();
    g.push((ptr, len));
}

extern "C" fn no_op(_p: i64, _l: i64) {}

fn jit_run_and_collect_logs(src: &str) -> Result<Vec<String>, String> {
    // Hold the test-serialization lock for the whole test body so the
    // process-wide `LOG_CAPTURE` Mutex stays uncontended.
    let _guard = TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
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
        ("mty_runtime_log", capture_log as *const u8),
        ("mty_runtime_print", capture_log as *const u8),
        ("mty_runtime_panic", no_op as *const u8),
        ("mty_runtime_arena_push", no_op as *const u8),
        ("mty_runtime_arena_pop", no_op as *const u8),
        ("mty_runtime_alloc", no_op as *const u8),
        ("mty_runtime_budget_charge", no_op as *const u8),
        ("mty_runtime_send", no_op as *const u8),
        ("mty_runtime_ask", no_op as *const u8),
        ("mty_runtime_spawn", no_op as *const u8),
        ("mty_runtime_extern_call", no_op as *const u8),
        ("mty_runtime_log_i64", no_op as *const u8),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    let _ = jc.call_main();
    // Convert each (ptr,len) to a String by reading the raw bytes.
    let mut out = Vec::new();
    for (ptr, len) in LOG_CAPTURE.lock().unwrap().drain(..) {
        if ptr == 0 || len == 0 {
            out.push(String::new());
            continue;
        }
        // SAFETY: the JIT-emitted .rodata buffer lives for the lifetime
        // of the JitCompiled (the closure runs while `jc` is alive in
        // this scope).
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec();
        out.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    // Keep `jc` alive for the entire string-read window.
    drop(jc);
    Ok(out)
}

fn must_log(src: &str) -> Vec<String> {
    jit_run_and_collect_logs(src).unwrap_or_else(|e| {
        panic!("compile/run failure: {e}\nsource:\n{src}");
    })
}

// ---- 1. Baseline: literal log -----------------------------------------

#[test]
fn literal_log_still_works() {
    let logs = must_log(r#"fn main() { log("hello") }"#);
    assert_eq!(logs, vec!["hello".to_string()]);
}

#[test]
fn literal_log_long_string() {
    let logs = must_log(r#"fn main() { log("the quick brown fox jumps over the lazy dog") }"#);
    assert_eq!(
        logs,
        vec!["the quick brown fox jumps over the lazy dog".to_string()]
    );
}

// ---- 2. Dynamic log: locally-bound Str --------------------------------

#[test]
fn log_of_local_str() {
    let src = r#"
        fn main() {
          let s = "hello, dyn"
          log(s)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["hello, dyn".to_string()]);
}

#[test]
fn log_of_local_str_explicit_type() {
    let src = r#"
        fn main() {
          let s: Str = "explicit"
          log(s)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["explicit".to_string()]);
}

#[test]
fn log_of_local_str_passes_through_let() {
    let src = r#"
        fn main() {
          let a = "first"
          let b = a
          log(b)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["first".to_string()]);
}

// ---- 3. Dynamic log: fn return ---------------------------------------

#[test]
fn log_of_fn_return() {
    let src = r#"
        fn name() -> Str { "world" }
        fn main() {
          log(name())
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["world".to_string()]);
}

#[test]
fn log_of_fn_return_via_local() {
    let src = r#"
        fn greeting() -> Str { "g'day" }
        fn main() {
          let g = greeting()
          log(g)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["g'day".to_string()]);
}

// ---- 4. Multiple log calls of different shapes -----------------------

#[test]
fn mixed_literal_and_dynamic_logs() {
    let src = r#"
        fn dyn_str() -> Str { "from-fn" }
        fn main() {
          log("static")
          let s = "from-let"
          log(s)
          log(dyn_str())
        }
    "#;
    let logs = must_log(src);
    assert_eq!(
        logs,
        vec![
            "static".to_string(),
            "from-let".to_string(),
            "from-fn".to_string(),
        ]
    );
}

#[test]
fn log_two_separate_locals() {
    let src = r#"
        fn main() {
          let a = "one"
          let b = "two"
          log(a)
          log(b)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["one".to_string(), "two".to_string()]);
}

// ---- 5. Dynamic log: conditional / branching path --------------------

#[test]
fn log_in_if_branch_dynamic() {
    let src = r#"
        fn main() {
          let s = "true-branch"
          let t = "false-branch"
          if true {
            log(s)
          } else {
            log(t)
          }
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["true-branch".to_string()]);
}

// ---- 6. Dynamic log: print() builtin shares the path -----------------

#[test]
fn print_of_dynamic_str_works() {
    let src = r#"
        fn main() {
          let s = "via-print"
          print(s)
        }
    "#;
    // print uses the same capture function in the test harness.
    let logs = must_log(src);
    assert_eq!(logs, vec!["via-print".to_string()]);
}

// ---- 7. Pre-fix regression: this would have been "Unsupported" -------

#[test]
fn log_of_local_does_not_raise_unsupported() {
    // Pre-fix: `log(s)` where `s` is a Str local raised
    // `CodegenError::Unsupported("non-literal string in log/print")`.
    // The whole program would fail to JIT. Now it must compile.
    let src = r#"
        fn make() -> Str { "dynamic" }
        fn main() {
          let s = make()
          log(s)
        }
    "#;
    let result = jit_run_and_collect_logs(src);
    assert!(
        result.is_ok(),
        "dynamic log compile must succeed; got {result:?}"
    );
}

// ---- 8. Repeated log of same local should still produce the right
//        bytes both times (no aliasing bug).

#[test]
fn log_twice_same_local() {
    let src = r#"
        fn main() {
          let s = "twice"
          log(s)
          log(s)
        }
    "#;
    let logs = must_log(src);
    assert_eq!(logs, vec!["twice".to_string(), "twice".to_string()]);
}
