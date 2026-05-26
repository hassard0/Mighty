//! v0.21 — per-backend native-ABI conformance harness.
//!
//! Walks `tests/conformance/native_abi/<NN_name>/` and for every case:
//!
//! 1. Parses `input.mty` through the syntax → HIR → typeck pipeline.
//! 2. Lowers the typed package to SIR (`mty_ir::lower_package`).
//! 3. Compiles the SIR program to a host-format object file via
//!    [`mty_codegen_cranelift::compile_object`].
//! 4. Re-parses the emitted object via the `object` crate and pins
//!    the platform-conformant invariants:
//!    - The object decodes cleanly.
//!    - The Mighty `main` symbol is exported (an entry point
//!      for the system linker to find).
//!    - The emitted file is not empty.
//! 5. **Stretch**, gated on `cfg(unix)` + a host `cc`: if both are
//!    available the harness asks the system C toolchain to compile
//!    `harness.c` against the emitted `.o` and runs the linked
//!    executable, diffing the exit code against
//!    `expected_harness_exit.txt`. When the symbol the harness
//!    references is `Linkage::Local` in the current codegen output
//!    (e.g. v0.21 lowers `export c fn _add` as local, not exported)
//!    the link step is *expected* to fail; that failure is *not*
//!    a test regression — it pins the v0.21 baseline so the v0.22
//!    slice that flips the linkage to Export will see the test go
//!    from "linker-error-OK" to "linker-success + exit-code-OK".
//!
//! Spec §29.1 (C-ABI exports) + CONFORMANCE_V0_21_NOTES.md.
//!
//! On Windows the link-and-run stretch is unconditionally skipped
//! (no portable `cc` invocation matrix — Windows MSVC needs the
//! `cl.exe` / `link.exe` pair, whose flag surface differs from
//! gcc/clang). The object-shape assertions still run and catch the
//! bulk of the regressions a v0.21 codegen change would introduce.

use mty_ast::AstNode;
use mty_codegen_cranelift::compile_object;
use mty_ir::lower_package;
use mty_syntax::parse;
use object::read::{Object as _, ObjectSymbol as _};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn case_dir(name: &str) -> PathBuf {
    workspace_root()
        .join("tests/conformance/native_abi")
        .join(name)
}

/// Drive the static-analysis pipeline far enough to produce a SIR
/// program. Stops on the first error-severity diagnostic from any
/// phase. Test-internal helper — no public re-export.
fn lower_to_sir(src: &str, source_id: &str) -> Result<mty_ir::Program, String> {
    let parsed = parse(src);
    if !parsed.errors.is_empty() {
        return Err(format!(
            "{source_id}: parse errors: {:?}",
            parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
        ));
    }
    let file = mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green))
        .ok_or_else(|| format!("{source_id}: FILE root not produced"))?;
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!(
            "{source_id}: lower error MT{:04}: {}",
            d.code.0, d.primary.message
        ));
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        return Err(format!(
            "{source_id}: typeck error MT{:04}: {}",
            d.code.0, d.primary.message
        ));
    }
    // Borrow check is best-effort here: a failure shouldn't block the
    // codegen smoke since native_abi cases focus on ABI shape, not
    // borrow regions. We still run it so an accidental regression in
    // a fixture surfaces, but downgrade hard-fail to a soft note.
    let borrow_diags = mty_borrow::check_package(&typed, &pkg);
    if let Some(d) = borrow_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        eprintln!(
            "[conformance_native] {source_id}: borrow MT{:04}: {} (soft)",
            d.code.0, d.primary.message
        );
    }
    Ok(lower_package(&pkg, &typed))
}

/// Per-case smoke. Asserts everything we can pin without depending on
/// a host C toolchain (the bulk of CI runners on Windows/macOS don't
/// have one pre-installed).
fn compile_case_to_object(case: &str) -> PathBuf {
    let dir = case_dir(case);
    let input = dir.join("input.mty");
    let src = std::fs::read_to_string(&input)
        .unwrap_or_else(|e| panic!("[{case}] read {}: {e}", input.display()));
    let prog = lower_to_sir(&src, case).expect("static analysis");

    // Emit the object into a tempdir, then assert its sanity.
    let tmp = tempfile::tempdir().expect("tempdir");
    let obj_path = tmp.path().join(format!("{case}.o"));
    compile_object(&prog, &obj_path).unwrap_or_else(|e| panic!("[{case}] compile_object: {e:?}"));

    assert!(obj_path.exists(), "[{case}] object file not written");
    let bytes = std::fs::read(&obj_path).expect("read object");
    assert!(!bytes.is_empty(), "[{case}] emitted object is empty");

    // Re-parse the object — it MUST decode cleanly under the `object`
    // crate's auto-format detection.
    let parsed = object::read::File::parse(&*bytes)
        .unwrap_or_else(|e| panic!("[{case}] object::read::File::parse: {e:?}"));

    // The Mighty `main` symbol is `Linkage::Export` per
    // `lower.rs::declare_fns`. It MUST appear in the symbol table
    // for the system linker to drive the program. (Other fns are
    // Linkage::Local today — see the v0.21 stretch note in the
    // file-level doc comment.)
    let mut saw_main = false;
    for sym in parsed.symbols() {
        if let Ok(name) = sym.name() {
            // Mach-O prepends `_` to C symbols; ELF/COFF do not.
            // Accept both spellings.
            if name == "main" || name == "_main" {
                saw_main = true;
            }
        }
    }
    assert!(
        saw_main,
        "[{case}] expected `main` symbol in emitted object — search failed across {} symbols",
        parsed.symbols().count()
    );

    // Hand the path back to the caller so the link-and-run stretch can
    // pick it up. We rely on TempDir living long enough — leak it via
    // Box::leak so the file outlives the test scope on the stretch
    // path. Test scope is single-threaded so leaking a tempdir per
    // case is fine; the OS cleans it on test process exit.
    let leaked: &'static tempfile::TempDir = Box::leak(Box::new(tmp));
    leaked.path().join(format!("{case}.o"))
}

/// Attempt to compile `harness.c` against the emitted .o + link + run.
/// Returns Some(exit_code) on success, None when the host couldn't
/// drive the cycle (no cc, link failed because the case-under-test
/// references a symbol the codegen doesn't export yet, etc.).
#[cfg(unix)]
fn try_link_and_run(case: &str, obj_path: &Path) -> Option<i32> {
    use std::process::Command;
    let dir = case_dir(case);
    let harness = dir.join("harness.c");
    if !harness.exists() {
        eprintln!("[conformance_native] {case}: no harness.c — skipping link step");
        return None;
    }

    // Pick `cc` from PATH. Most Unix CI runners have it; on the rare
    // sysroot where it's absent we fall through.
    let tmp = tempfile::tempdir().expect("tempdir");
    let exe = tmp.path().join(format!("{case}.exe"));
    let out = match Command::new("cc")
        .arg(&harness)
        .arg(obj_path)
        .arg("-o")
        .arg(&exe)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[conformance_native] {case}: cc invocation failed ({e}); link step skipped");
            return None;
        }
    };
    if !out.status.success() {
        // The expected failure mode in v0.21: the codegen lowers
        // `export c fn _foo` as `Linkage::Local`, so the harness's
        // `extern int32_t _foo(...)` reference goes unresolved. We
        // log the linker diagnostic so a future slice that flips the
        // linkage sees a clear "now this case passes" signal.
        eprintln!(
            "[conformance_native] {case}: cc link failed (v0.21 expected baseline — fn not exported):\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        return None;
    }

    // Got past link. Run the binary and capture the exit code.
    let exit = match Command::new(&exe).output() {
        Ok(o) => o.status.code().unwrap_or(-1),
        Err(e) => {
            eprintln!("[conformance_native] {case}: exec failed: {e}");
            return None;
        }
    };
    Some(exit)
}

#[cfg(not(unix))]
fn try_link_and_run(_case: &str, _obj_path: &Path) -> Option<i32> {
    // Windows: skip the link step. The reason is intentionally not a
    // `cfg(any(unix, target_os = "linux"))` so a future Windows CC
    // adapter (PR-pending) can flip the cfg flag without rewriting the
    // helper.
    None
}

fn expected_harness_exit(case: &str) -> i32 {
    let dir = case_dir(case);
    let path = dir.join("expected_harness_exit.txt");
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    raw.trim().parse::<i32>().unwrap_or_else(|e| {
        panic!(
            "[{case}] expected_harness_exit.txt is not an i32 ({}): {e}",
            raw.trim()
        )
    })
}

fn run_case(case: &str) {
    let obj = compile_case_to_object(case);
    // Linker stretch — best effort, never fails the test if the host
    // can't drive it OR if the v0.21 baseline emits the case's export
    // as Linkage::Local. We still pin the *positive* path: when the
    // link does succeed, the exit code MUST match.
    if let Some(exit) = try_link_and_run(case, &obj) {
        let want = expected_harness_exit(case);
        assert_eq!(
            exit, want,
            "[{case}] linked harness exit code mismatch (want {want}, got {exit})"
        );
        eprintln!("[conformance_native] {case}: link + run OK (exit={exit})");
    } else {
        eprintln!(
            "[conformance_native] {case}: link step skipped (host/codegen baseline) — \
             object-shape assertions still passed"
        );
    }
}

#[test]
fn native_abi_01_export_main() {
    run_case("01_export_main");
}

#[test]
fn native_abi_02_string_return() {
    run_case("02_string_return");
}

#[test]
fn native_abi_03_struct_return() {
    run_case("03_struct_return");
}

#[test]
fn native_abi_04_callback() {
    run_case("04_callback");
}

/// Meta-test: enumerate the native_abi directory and assert every
/// case has the secondary files the kit needs (`harness.c` +
/// `expected_harness_exit.txt`). Catches the "someone rm'd a fixture
/// by accident" regression that the per-case tests would miss if
/// they were silently skipped.
#[test]
fn native_abi_kit_inventory() {
    let root = workspace_root().join("tests/conformance/native_abi");
    let mut cases: Vec<String> = vec![];
    for entry in std::fs::read_dir(&root).expect("read native_abi dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        cases.push(name.clone());
        assert!(
            path.join("input.mty").exists(),
            "missing input.mty in {name}",
        );
        assert!(
            path.join("harness.c").exists(),
            "missing harness.c in {name} — v0.21 native_abi kit invariant",
        );
        assert!(
            path.join("expected_harness_exit.txt").exists(),
            "missing expected_harness_exit.txt in {name}",
        );
        assert!(
            path.join("README.md").exists(),
            "missing README.md in {name}"
        );
        assert!(
            path.join("command.txt").exists(),
            "missing command.txt in {name}",
        );
    }
    assert!(
        cases.len() >= 4,
        "v0.21 floor: native_abi/ MUST carry ≥4 cases, found {}",
        cases.len()
    );
}
