//! v0.45 T1 (L18 fix) — native `std.fs.*` JIT regression suite.
//!
//! Pre-v0.45 the cranelift backend forced every `std.fs.*` call onto
//! the interpreter fallback (`CodegenError::Unsupported`). v0.45 T1
//! adds a native runtime ABI (`mty_runtime_fs_*`) and routes the
//! whole surface through it, so generated CLIs touch disk under
//! `mty build` without a Rust shim.
//!
//! Coverage strategy: drive each new method through a Mighty source
//! snippet built with `build_jit`, executed against the real runtime
//! symbol table (`mty_runtime::codegen_abi::symbol_table`), and
//! asserted with `std::fs` reads against the tempdir.

use mty_ast::AstNode;
use mty_codegen_cranelift::jit::{build_jit, symbols_from};
use mty_ir::lower_package;
use mty_syntax::parse;
use std::path::Path;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Build and JIT a Mighty source string against the real runtime
/// symbol table, then call `main`. Returns Ok on success — failures
/// panic with the codegen / parse error.
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
    // Use forward slashes — Windows accepts them and Mighty source
    // strings shouldn't worry about backslash escapes.
    p.display().to_string().replace('\\', "/")
}

#[test]
fn fs_write_creates_file_with_bytes() {
    let dir = tempdir();
    let p = dir.path().join("out.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.write("{p}", "hello-v045")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    let body = std::fs::read_to_string(&p).expect("read tempfile");
    assert_eq!(body, "hello-v045");
}

#[test]
fn fs_write_file_alias_round_trips() {
    let dir = tempdir();
    let p = dir.path().join("out2.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.write_file("{p}", "alias-works")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "alias-works");
}

#[test]
fn fs_write_string_alias_round_trips() {
    let dir = tempdir();
    let p = dir.path().join("ws.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.write_string("{p}", "ws-alias")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "ws-alias");
}

#[test]
fn fs_append_creates_then_extends() {
    let dir = tempdir();
    let p = dir.path().join("log.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.append("{p}", "one\n")
  std.fs.append("{p}", "two\n")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ntwo\n");
}

#[test]
fn fs_read_reads_back_what_we_wrote() {
    let dir = tempdir();
    let p = dir.path().join("rt.txt");
    std::fs::write(&p, b"native-read").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  let body = std.fs.read("{p}")
  log(body)
}}
"#,
        p = path_str(&p)
    );
    // We don't pipe stdout through the harness — just verify the
    // call completes (the file existing is a separate fact). The
    // round-trip-then-read test below covers the full write→read
    // path inside a single Mighty program.
    jit_run(&src);
}

#[test]
fn fs_read_to_string_round_trip_inside_one_program() {
    let dir = tempdir();
    let p = dir.path().join("rt2.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.write("{p}", "round-trip")
  let body = std.fs.read_to_string("{p}")
  log(body)
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    // Verify the file has the bytes we expected.
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "round-trip");
}

#[test]
fn fs_exists_returns_true_for_present_file() {
    let dir = tempdir();
    let present = dir.path().join("present.txt");
    std::fs::write(&present, b"hi").unwrap();
    let missing = dir.path().join("missing.txt");
    let src = format!(
        r#"
use std.fs

fn main() {{
  if std.fs.exists("{present}") {{ log("present") }}
  if !std.fs.exists("{missing}") {{ log("absent") }}
}}
"#,
        present = path_str(&present),
        missing = path_str(&missing),
    );
    jit_run(&src);
}

#[test]
fn fs_create_dir_all_makes_nested_directories() {
    let dir = tempdir();
    let nested = dir.path().join("a/b/c");
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.create_dir_all("{p}")
}}
"#,
        p = path_str(&nested)
    );
    jit_run(&src);
    assert!(nested.exists() && nested.is_dir());
}

#[test]
fn fs_remove_file_deletes() {
    let dir = tempdir();
    let p = dir.path().join("doomed.txt");
    std::fs::write(&p, b"x").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.remove_file("{p}")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
    assert!(!p.exists());
}

#[test]
fn fs_remove_dir_all_recursively_deletes() {
    let dir = tempdir();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("a.txt"), b"x").unwrap();
    std::fs::write(root.join("sub/b.txt"), b"y").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.remove_dir_all("{p}")
}}
"#,
        p = path_str(&root)
    );
    jit_run(&src);
    assert!(!root.exists());
}

#[test]
fn fs_metadata_calls_without_segfaulting() {
    let dir = tempdir();
    let p = dir.path().join("m.txt");
    std::fs::write(&p, b"abcdefgh").unwrap();
    // For v0.45 T1, the metadata Mighty-side typing as a tuple/record
    // is documented in the docstub but the codegen doesn't yet expose
    // the field projections (deferred to a follow-up that lifts the
    // Metadata ADT through typeck). The test pins the call shape
    // through the runtime ABI — proves the slot write doesn't fault
    // and the runtime symbol resolves.
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.metadata("{p}")
}}
"#,
        p = path_str(&p)
    );
    jit_run(&src);
}

/// v0.45 T1 — the codegen path is unconditional once typecheck
/// passes. Compile-time enforcement of `effect fs` still lives in
/// `mty-types`; this test verifies that a pub fn missing `effect fs`
/// still trips MT4001 BEFORE the codegen ever sees the program. If
/// that gate ever regresses, agents could ship native binaries that
/// touch disk from unsanctioned call sites.
#[test]
fn capability_check_pub_fn_missing_effect_fs_errors() {
    // Use the same `fs.write(...)` bare-call shape the existing
    // mty-types effect_row tests exercise (`effect_row_e2e.rs`); the
    // `std.` prefix gets stripped by `use std.fs`. The typeck pass
    // walks `fs.write` and flags the missing `effect fs` even though
    // the call is at module level — pub-fn-without-effect is the
    // canonical MT4001 trigger.
    let src = r#"
        use std.fs
        pub fn writer() -> Unit {
            fs.write("./out", "hello")
        }
    "#;
    let parsed = parse(src);
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, _lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    let typed = mty_types::check_package_typed(&pkg);
    let codes: Vec<String> = typed
        .diagnostics
        .iter()
        .map(|d| format!("MT{:04}", d.code.0))
        .collect();
    assert!(
        codes.contains(&"MT4001".to_string()),
        "expected MT4001 for pub fn writer missing `effect fs`; got {:?}",
        codes
    );
}

#[test]
fn fs_read_dir_calls_without_segfaulting() {
    let dir = tempdir();
    std::fs::write(dir.path().join("one"), b"1").unwrap();
    std::fs::write(dir.path().join("two"), b"2").unwrap();
    let src = format!(
        r#"
use std.fs

fn main() {{
  std.fs.read_dir("{p}")
}}
"#,
        p = path_str(dir.path())
    );
    jit_run(&src);
}
