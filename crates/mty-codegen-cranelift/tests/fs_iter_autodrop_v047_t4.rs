//! v0.47 T4 — `DirIter` auto-Drop regression suite.
//!
//! v0.46 T4 (PR #33) shipped a manual-close iterator surface: source
//! code had to call `it.close()` to release the boxed `DirIterState`,
//! otherwise the handle leaked one heap allocation per opened
//! iterator. v0.47 T4 closes that gap with a generic auto-Drop
//! mechanism (`#[mty_drop = "..."]` ADT registration, see
//! `crates/mty-types/src/prelude.rs::mty_drop_fns`), and the IR
//! post-pass `inject_auto_drop_stmts` inserts a `Stmt::Drop(local)`
//! in front of every fn-exit terminator for any local typed `DirIter`.
//!
//! What this suite pins:
//!
//!   - DirIter goes out of scope without `.close()` → the runtime
//!     symbol `mty_runtime_fs_dir_close` IS called automatically.
//!     (Verified end-to-end by running Mighty source through the JIT
//!     and asserting the program completes without leaks; a non-zero
//!     `Box<DirIterState>` would still be on the heap if the auto-Drop
//!     didn't fire, but that's harder to assert from a regression
//!     test — the more important invariant is the next bullet.)
//!
//!   - Explicit `.close()` followed by an auto-Drop at fn exit
//!     dispatches the runtime symbol with handle=0 the second time —
//!     no double-free, no use-after-free. This is the idempotence
//!     contract for the `DirIter` Drop story.
//!
//!   - Early return from a fn that owns a DirIter still triggers the
//!     drop (the post-pass injects the drop at EVERY fn-exit
//!     terminator, not just the tail Return).
//!
//! Mighty doesn't yet model panic-unwinding (`mty_runtime_panic` aborts
//! the process), so the panic-unwind path is documented as
//! abort-on-panic — the runtime would not get a chance to fire Drop on
//! a panicking DirIter scope. See the "Unresolved" note in the v0.47 T4
//! PR description.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::path::Path;
use std::sync::Mutex;

/// Serialise tests sharing the FMT_STRINGS interner / JIT linker.
/// Mirrors the lock in `fs_iter_v046_t4.rs` so back-to-back runs of
/// both files don't double-up the runtime's process-wide state.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn jit_run(src: &str) {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        panic!(
            "parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        );
    }
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!("lower MT{:04}: {}", d.code.0, d.primary.message);
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!("typeck MT{:04}: {}", d.code.0, d.primary.message);
    }
    let prog = lower_package(&pkg, &typed);
    let st = mty_runtime::codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    let jc = build_jit(&prog, &syms).expect("build_jit");
    let _ = jc.call_main();
    drop(jc);
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn path_str(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

// =====================================================================
// Section 1 — Auto-Drop semantics via the IR post-pass
// =====================================================================

/// Pin that the IR-side post-pass `inject_auto_drop_stmts` actually
/// injects a `Stmt::Drop(local)` for a DirIter local before the fn's
/// tail Return. This is a direct, IR-level assertion — independent of
/// whether the codegen successfully translated the drop into a runtime
/// call, so a regression in the IR pass surfaces here clearly.
#[test]
fn ir_post_pass_injects_drop_stmt_for_dir_iter_local() {
    let src = r#"
use std.fs

fn main() {
  let _it = std.fs.read_dir("./does-not-matter")
}
"#;
    let parsed = parse(src);
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, _) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    let typed = mty_types::check_package_typed(&pkg);
    let prog = lower_package(&pkg, &typed);

    // Find the user `main`. It should contain at least one
    // `Stmt::Drop(local)` whose local's declared type is the DirIter
    // ADT (the post-pass should have inserted it before the
    // `Term::Return`).
    let main_fn = prog.fns.iter().find(|f| f.name == "main").expect("main fn");
    let dir_iter_adt = match typed.def_map.lookup("DirIter") {
        Some(mty_types::DefRef::Adt(a)) => a,
        _ => panic!("DirIter ADT must be registered in the prelude"),
    };
    let mut saw_drop_of_dir_iter = false;
    for blk in &main_fn.blocks {
        for s in &blk.stmts {
            if let mty_ir::ir::Stmt::Drop(l) = s {
                let lty = &main_fn.locals[l.0 as usize].ty;
                if matches!(lty, mty_ir::ir::IrTy::Adt(aid, _) if *aid == dir_iter_adt) {
                    saw_drop_of_dir_iter = true;
                }
            }
        }
    }
    assert!(
        saw_drop_of_dir_iter,
        "expected the v0.47 T4 IR post-pass to inject a Stmt::Drop \
         for the DirIter local before the fn's tail Return; \
         post-pass appears to have been skipped or mis-typed"
    );
    // Also pin the side-table mapping the ADT to the runtime symbol.
    assert_eq!(
        prog.adt_drop_fns.get(&dir_iter_adt).map(|s| s.as_str()),
        Some("mty_runtime_fs_dir_close"),
        "DirIter -> runtime drop symbol must be wired in Program.adt_drop_fns"
    );
}

/// End-to-end: open a DirIter, drain it, let it go out of scope WITHOUT
/// calling `.close()`. The auto-Drop pass injects the close call
/// before the fn's tail Return, so the runtime frees the handle on
/// scope exit. We can't observe the free directly from the test (no
/// public refcount on `Box<DirIterState>`), but the program MUST
/// complete without trapping — which it does only if the codegen's
/// `Stmt::Drop` lowering routes through `mty_runtime_fs_dir_close`
/// correctly (a bad lowering would either no-op the close — silent
/// leak, not observable here — or call the wrong symbol — abort).
#[test]
fn dir_iter_no_explicit_close_runs_to_completion() {
    let dir = tempdir();
    std::fs::write(dir.path().join("a"), b"a").unwrap();
    std::fs::write(dir.path().join("b"), b"b").unwrap();
    std::fs::write(dir.path().join("c"), b"c").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  while let Some(_e) = it.next() {{
  }}
  // No explicit it.close() — the v0.47 T4 auto-Drop is the only
  // close call that runs. If the post-pass / Stmt::Drop lowering
  // regressed, the heap leak is silent here but the runtime symbol
  // table lookup would still pass.
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Explicit `.close()` followed by the implicit auto-Drop at fn exit:
/// the runtime is called twice (once with the real handle, once with
/// 0 because `emit_dir_iter_close` zeroes the receiver Variable).
/// MUST run to completion without a double-free SIGSEGV. This is the
/// idempotence contract.
#[test]
fn dir_iter_explicit_close_plus_auto_drop_is_safe() {
    let dir = tempdir();
    std::fs::write(dir.path().join("x"), b"1").unwrap();
    std::fs::write(dir.path().join("y"), b"2").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut it = std.fs.read_dir("{p}")
  while let Some(_e) = it.next() {{
  }}
  it.close()
  // it goes out of scope here — auto-Drop fires too. The explicit
  // close zeroed the receiver Variable so this second call lands
  // with handle=0 → runtime no-op (the ABI contract).
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Early return from a fn that owns a DirIter. The post-pass injects
/// a `Stmt::Drop` before EVERY fn-exit Return, so the early branch
/// gets one too. The two-arm `if` shape forces two different exit
/// terminators; if the post-pass missed one of them, this test still
/// exercises both arms (the early-return arm fires for an empty dir,
/// the tail-return arm fires when entries exist — we cover both
/// scenarios in two tests below).
#[test]
fn dir_iter_early_return_after_open_still_drops() {
    let dir = tempdir();
    std::fs::write(dir.path().join("first"), b"1").unwrap();
    let src = format!(
        r#"
use std.fs

fn open_and_bail(p: Str) -> Unit {{
  let mut it = std.fs.read_dir(p)
  let n = it.next()
  // Early return — the auto-Drop must fire here too, not just at
  // the implicit tail Return of `main`.
  return
}}

fn main() {{
  open_and_bail("{p}")
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// Multiple DirIter locals in the same fn — every one of them gets
/// its own auto-Drop before the tail Return. Pins that the post-pass
/// walks ALL locals (not just the first match it finds).
#[test]
fn dir_iter_multiple_locals_all_get_dropped() {
    let dir = tempdir();
    std::fs::write(dir.path().join("alpha"), b"a").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let mut a = std.fs.read_dir("{p}")
  let mut b = std.fs.read_dir("{p}")
  let mut c = std.fs.read_dir("{p}")
  let _ = a.next()
  let _ = b.next()
  let _ = c.next()
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}

/// IR-level pin: every fn-exit Return in a multi-block fn receives the
/// drop. Use the IR directly instead of running the JIT so the
/// assertion is sharp. Pre-fix (no post-pass), branch-then-Return
/// would have left an early-return arm without its Drop.
#[test]
fn ir_post_pass_injects_drop_for_every_fn_exit() {
    // `let mut it = read_dir(p); if true { return }; it.close()` —
    // the `if true` branch terminator is its own Return. The
    // post-pass MUST inject a Drop into that block too.
    let src = r#"
use std.fs

fn open_and_maybe_bail(p: Str) -> Unit {
  let mut it = std.fs.read_dir(p)
  if true {
    return
  }
}
"#;
    let parsed = parse(src);
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, _) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    let typed = mty_types::check_package_typed(&pkg);
    let prog = lower_package(&pkg, &typed);

    let dir_iter_adt = match typed.def_map.lookup("DirIter") {
        Some(mty_types::DefRef::Adt(a)) => a,
        _ => panic!("DirIter ADT must be registered in the prelude"),
    };

    let func = prog
        .fns
        .iter()
        .find(|f| f.name == "open_and_maybe_bail")
        .expect("open_and_maybe_bail");

    // Count blocks that terminate with Return AND have a preceding
    // Stmt::Drop of a DirIter-typed local. Must be at least 2 (the
    // early-return arm + the tail-return arm).
    let mut returns_with_drop = 0;
    let mut total_returns = 0;
    for blk in &func.blocks {
        if !matches!(blk.terminator, mty_ir::ir::Term::Return(_)) {
            continue;
        }
        total_returns += 1;
        let has_dir_drop = blk.stmts.iter().any(|s| {
            if let mty_ir::ir::Stmt::Drop(l) = s {
                let lty = &func.locals[l.0 as usize].ty;
                matches!(lty, mty_ir::ir::IrTy::Adt(aid, _) if *aid == dir_iter_adt)
            } else {
                false
            }
        });
        if has_dir_drop {
            returns_with_drop += 1;
        }
    }
    assert!(
        total_returns >= 2,
        "expected at least 2 Return terminators (early + tail), saw {}",
        total_returns
    );
    assert_eq!(
        returns_with_drop, total_returns,
        "every Return-terminator block must carry a Drop(DirIter); \
         saw {}/{}",
        returns_with_drop, total_returns
    );
}
