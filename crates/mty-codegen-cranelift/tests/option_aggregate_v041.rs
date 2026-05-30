//! v0.41 T3 — `Vec.get` / `Vec.pop` / opaque-`next` synthesise an
//! `Option[T]` **aggregate**, not a bare scalar.
//!
//! L1 (the "honest correctness" lesson) called out that the native
//! Cranelift backend lags the interpreter. The lessons doc names two
//! example-level repros — `examples/05_match_expr.mty` and
//! `examples/17_unsafe.mty` — but the conformance sweep added in
//! v0.41 T3 surfaced the actual root cause behind a different cluster
//! of segfaults (examples 26 / 30 / 42 / 43):
//!
//!   1. `v.get(i)` lowered to a bounds-check-or-panic + scalar element
//!      load. But the SIR signature is `Vec[T] -> Option[T]`, so the
//!      consumer reads the result through a `switch_variant` (i.e.
//!      treats the i64 as the address of an Option aggregate). With
//!      the bare scalar shape, the next `match` dereferences a scalar
//!      element value as if it were a pointer → segfault.
//!   2. `v.pop()` had the same shape: returned an i64 (0 on empty),
//!      but the consumer expects an Option aggregate. Same crash.
//!   3. Opaque-receiver method calls (e.g. `stream.next()`) fell
//!      through to the `mty_runtime_extern_call` bridge, which returns
//!      i64 = 0. The interpreter returns `None` for any opaque
//!      receiver's `.next()` (see `mty-ir::interp::run::eval_method`
//!      "next" arm). Native silently fed 0 into the consuming
//!      `switch_variant`, which dereferences address 0 → segfault.
//!
//! Each test JIT-compiles a minimal source and asserts the
//! match-arm sees the expected variant. We can't probe the
//! Option payload from outside the compiled fn (no side-effecting
//! API for it), so we route observability through `log` from the
//! Some/None arms and capture the resulting stdout buffer.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::sync::Mutex;

static LOG_CAPTURE: Mutex<Vec<(i64, i64)>> = Mutex::new(Vec::new());
static TEST_LOCK: Mutex<()> = Mutex::new(());

extern "C" fn capture_log(ptr: i64, len: i64) {
    LOG_CAPTURE.lock().unwrap().push((ptr, len));
}
extern "C" fn no_op(_p: i64, _l: i64) {}

/// Bumpalo-backed arena bridge so `Vec.new()` / `String.from_str` allocs
/// don't return null. v0.41 T3 also auto-pushes an arena at `main`
/// entry, so this stays a smoke-only stub.
extern "C" fn arena_push_stub() -> i64 {
    1
}
extern "C" fn arena_pop_stub(_h: i64) {}
extern "C" fn alloc_stub(size: i64, _align: i64, _zero: i64) -> i64 {
    // Single-shot bump allocator into a fixed-size leak. The JIT'd
    // code only allocates a tiny header + small element buffers in
    // these tests, so a 64 KiB leak is plenty.
    const CAP: usize = 65536;
    static OFFSET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    // `Box::leak` once: stable raw pointer for the rest of the
    // process lifetime, no `static mut` aliasing UB.
    use std::sync::OnceLock;
    static BUF: OnceLock<usize> = OnceLock::new();
    let base = *BUF.get_or_init(|| {
        let v = vec![0u8; CAP].into_boxed_slice();
        Box::leak(v).as_ptr() as usize
    });
    let n = size as usize;
    let prev = OFFSET.fetch_add(n, std::sync::atomic::Ordering::SeqCst);
    if prev + n > CAP {
        return 0;
    }
    (base + prev) as i64
}

fn jit_run_collecting_logs(src: &str) -> Result<Vec<String>, String> {
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
        ("mty_runtime_arena_push", arena_push_stub as *const u8),
        ("mty_runtime_arena_pop", arena_pop_stub as *const u8),
        ("mty_runtime_alloc", alloc_stub as *const u8),
        ("mty_runtime_budget_charge", no_op as *const u8),
        ("mty_runtime_send", no_op as *const u8),
        ("mty_runtime_ask", no_op as *const u8),
        ("mty_runtime_spawn", no_op as *const u8),
        ("mty_runtime_extern_call", no_op as *const u8),
        ("mty_runtime_log_i64", no_op as *const u8),
    ]);
    let jc = build_jit(&prog, &syms).map_err(|e| format!("jit: {e:?}"))?;
    let _ = jc.call_main();
    let mut out = Vec::new();
    for (ptr, len) in LOG_CAPTURE.lock().unwrap().drain(..) {
        if ptr == 0 || len == 0 {
            out.push(String::new());
            continue;
        }
        // SAFETY: the JIT-emitted .rodata buffer lives for the lifetime
        // of `jc` (held in scope below).
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) }.to_vec();
        out.push(String::from_utf8_lossy(&bytes).into_owned());
    }
    drop(jc);
    Ok(out)
}

fn run(src: &str) -> Vec<String> {
    jit_run_collecting_logs(src).unwrap_or_else(|e| panic!("jit run: {e}\nsource:\n{src}"))
}

// ---- Gap #1: v.get(i) returns Option, not scalar -------------------------

/// `v.get(0)` on a Vec with one element → `Some(_)` arm fires.
#[test]
fn vec_get_in_bounds_yields_some() {
    let src = r#"
        fn main() {
          let mut v: Vec[I32] = Vec.new()
          v = v.push(42)
          match v.get(0) {
            Some(_) => log("some")
            None => log("none")
          }
        }
    "#;
    assert_eq!(run(src), vec!["some".to_string()]);
}

/// `v.get(i)` where `i >= len` → `None` arm fires. Pre-v0.41 this
/// trapped via the bounds-check panic; v0.41 makes it match the
/// interpreter and fall into `None`.
#[test]
fn vec_get_out_of_bounds_yields_none() {
    let src = r#"
        fn main() {
          let v: Vec[I32] = Vec.new()
          match v.get(0) {
            Some(_) => log("some")
            None => log("none")
          }
        }
    "#;
    assert_eq!(run(src), vec!["none".to_string()]);
}

/// Same shape but on `Vec[U8]` — the typed-slot path (1-byte slots) is
/// a different vec_load_elem branch.
#[test]
fn vec_get_u8_in_bounds_yields_some() {
    let src = r#"
        fn main() {
          let mut v: Vec[U8] = Vec.new()
          v = v.push(0xAB_u8)
          match v.get(0) {
            Some(_) => log("some")
            None => log("none")
          }
        }
    "#;
    assert_eq!(run(src), vec!["some".to_string()]);
}

// ---- Gap #2: v.pop() returns Option ---------------------------------------

#[test]
fn vec_pop_non_empty_yields_some() {
    let src = r#"
        fn main() {
          let mut v: Vec[I32] = Vec.new()
          v = v.push(7)
          match v.pop() {
            Some(_) => log("some")
            None => log("none")
          }
        }
    "#;
    assert_eq!(run(src), vec!["some".to_string()]);
}

#[test]
fn vec_pop_empty_yields_none() {
    let src = r#"
        fn main() {
          let v: Vec[I32] = Vec.new()
          match v.pop() {
            Some(_) => log("some")
            None => log("none")
          }
        }
    "#;
    assert_eq!(run(src), vec!["none".to_string()]);
}

/// `while let Some(_) = v.pop()` — pop drains the vec one element at
/// a time. Three pushes → three loop iterations.
#[test]
fn vec_pop_drives_while_let_loop() {
    let src = r#"
        fn main() {
          let mut v: Vec[I32] = Vec.new()
          v = v.push(1)
          v = v.push(2)
          v = v.push(3)
          while let Some(_x) = v.pop() {
            log("iter")
          }
          log("done")
        }
    "#;
    assert_eq!(
        run(src),
        vec![
            "iter".to_string(),
            "iter".to_string(),
            "iter".to_string(),
            "done".to_string(),
        ]
    );
}

// ---- Gap #5: opaque .next() returns Option, not scalar 0 -----------------

/// `stream.next()` on an opaque receiver — the interpreter's match
/// arm catches `None` immediately. The JIT, pre-fix, fed 0 into the
/// consuming `switch_variant`, which dereferenced address 0 →
/// segfault.
#[test]
fn opaque_next_yields_none_via_method_name_heuristic() {
    let src = r#"
        fn _drain(stream: MessageStream) {
          while let Some(_delta) = stream.next() {
            log("delta")
          }
        }
        fn make_stream() -> MessageStream {
          MessageStream.empty()
        }
        fn main() {
          let s = make_stream()
          _drain(s)
          log("done")
        }
    "#;
    // No "delta" lines — the empty stream's first `.next()` is None,
    // so the loop exits immediately. The lone "done" pins the
    // post-loop reachability.
    assert_eq!(run(src), vec!["done".to_string()]);
}

// ---- Gap #3: implicit arena push at main entry ---------------------------

/// `Vec.new()` from a plain `fn main()` (no `arena {}` block) needs
/// a live arena frame, otherwise `mty_runtime_alloc` returns null
/// and the subsequent header writes segfault. v0.41 T3 auto-pushes
/// an arena at main entry to fix this.
#[test]
fn vec_new_in_plain_main_does_not_segfault() {
    let src = r#"
        fn main() {
          let _v: Vec[I32] = Vec.new()
          log("ok")
        }
    "#;
    assert_eq!(run(src), vec!["ok".to_string()]);
}

// ---- Gap #4: String methods don't dispatch as Vec methods ----------------

/// `s.clear()` on a String must NOT lower to `emit_vec_clear` (which
/// would zero a String byte at the (ptr,len) pair's start, breaking
/// invariants and causing infinite loops in the surrounding code).
/// Pre-fix this lowering was triggered for ANY receiver — String,
/// Vec, whatever.
#[test]
fn string_clear_does_not_route_through_vec_clear() {
    let src = r#"
        fn main() {
          let mut s = String.with_capacity(8)
          s.push_str("a")
          s.clear()
          log("ok")
        }
    "#;
    assert_eq!(run(src), vec!["ok".to_string()]);
}
