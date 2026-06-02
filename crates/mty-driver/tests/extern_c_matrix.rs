//! v0.36 Track T2 — extern c signature-matrix integration tests.
//!
//! Each row drives the full flow:
//!   1. Compile `impl.c` to an object via the system C compiler.
//!   2. Bundle it into a static archive (`libmtyrow.a` on Unix,
//!      `mtyrow.lib` on Windows MSVC; we always use `ar` / `llvm-ar`
//!      when available because clang + lld accept both forms).
//!   3. Build the row's `app.mty` via the driver's native pipeline,
//!      passing a synthetic `[[extern_lib]]` set that names the
//!      vendored archive.
//!   4. Run the produced executable.
//!   5. Assert the row-specific marker line appears in stdout.
//!
//! Rows that don't fit a stdout-capture shape (e.g. the
//! struct-by-value rows) still exercise the link step — the assertion
//! collapses to "executable runs and exits 0" because Mighty's `main`
//! returns Unit (i.e. process exit code 0).
//!
//! The matrix file (`docs/internals/extern-c-matrix.md`) holds the
//! human-readable status table; this test file is the machine-checked
//! mirror. If a row is added there, add a fn here too.

use std::path::{Path, PathBuf};
use std::process::Command;

use mty_codegen_cranelift::artifact::BuildMode;
use mty_driver::manifest::ExternLib;
use mty_driver::{build_native, BuildOptions, BuildOutcome, BuildTarget};

/// Walk up from the driver's `CARGO_MANIFEST_DIR` to the repo root,
/// then into `tests/extern_c_matrix/`.
fn matrix_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.push("tests");
    p.push("extern_c_matrix");
    p
}

/// Locate a C compiler. We prefer `clang` (matches the driver's linker
/// auto-detection on most hosts) then fall through to `cc` and `gcc`.
///
/// On Windows the rustup-bundled LLVM frequently lives at
/// `C:\Program Files\LLVM\bin\clang.exe` without being on PATH, so we
/// probe that location too — otherwise the matrix would silently skip
/// on a stock Windows dev box.
fn find_cc() -> Option<String> {
    for cand in ["clang", "clang.exe", "cc", "gcc", "gcc.exe", "cc.exe"] {
        if Command::new(cand).arg("--version").output().is_ok() {
            return Some(cand.into());
        }
    }
    for abs in [
        r"C:\Program Files\LLVM\bin\clang.exe",
        r"C:\Program Files (x86)\LLVM\bin\clang.exe",
    ] {
        if std::path::Path::new(abs).exists() {
            return Some(abs.into());
        }
    }
    None
}

/// Locate `ar` (or `llvm-ar`). Static archives on COFF can also be
/// built with `lib.exe` on MSVC; we only support `ar`-style archivers
/// because that's what `clang` + `lld` happily consume on all hosts
/// the harness targets in v0.36.
fn find_ar() -> Option<String> {
    for cand in ["ar", "llvm-ar", "ar.exe", "llvm-ar.exe"] {
        if Command::new(cand).arg("--version").output().is_ok() {
            return Some(cand.into());
        }
    }
    for abs in [
        r"C:\Program Files\LLVM\bin\llvm-ar.exe",
        r"C:\Program Files (x86)\LLVM\bin\llvm-ar.exe",
    ] {
        if std::path::Path::new(abs).exists() {
            return Some(abs.into());
        }
    }
    None
}

/// Decide whether we can run the row. Returns `Some(reason)` to skip,
/// `None` to proceed. We skip when:
/// - no C compiler is on PATH
/// - no `ar` is on PATH
/// - the driver itself cannot locate a linker (would skip codegen output)
///
/// Side-effect: if the driver's linker auto-detection fails and the
/// caller hasn't already set `STARDUST_LINKER`, we point it at the
/// same C compiler we found. clang knows how to drive its own linker
/// so this lets the row succeed on hosts where the LLVM toolchain
/// lives off-PATH (e.g. rustup's bundled `C:\Program Files\LLVM\`
/// directory on Windows).
fn maybe_skip_row() -> Option<&'static str> {
    let cc = find_cc();
    if cc.is_none() {
        return Some("no C compiler on PATH");
    }
    if find_ar().is_none() {
        return Some("no `ar` on PATH");
    }
    if mty_codegen_cranelift::object::find_linker().is_none() {
        // Try the C compiler — clang can act as its own linker driver.
        if let Some(c) = cc {
            // Race-tolerable: STARDUST_LINKER is read each time the
            // driver wants to link, and every row in this binary uses
            // the same compiler.
            std::env::set_var("STARDUST_LINKER", &c);
            if mty_codegen_cranelift::object::find_linker().is_none() {
                return Some("no linker on PATH");
            }
        }
    }
    None
}

/// Build a static archive carrying no-op implementations of every
/// `mty_runtime_*` symbol the cranelift backend pre-declares. Without
/// it, the host linker rejects every `.o` cranelift emits (the codegen
/// pre-declares every runtime import in `declare_fns`, and even
/// unused declarations show up as undefined symbol references in the
/// object's symbol table).
///
/// Returns the archive path so the test harness can pass it as a
/// second `[[extern_lib]]` entry alongside the row's own
/// `libmtyrow.a`.
///
/// This is *not* the production runtime — it's a shim that exists
/// solely to satisfy the linker. The matrix tests never call any of
/// these symbols from the Mighty side, so the stub bodies suffice.
fn build_runtime_stub(work_dir: &Path, cc: &str, ar: &str) -> PathBuf {
    let src = work_dir.join("mty_runtime_stub.c");
    std::fs::write(
        &src,
        r#"
/* v0.36 Track T2 — runtime symbol stub for the FFI matrix.
 *
 * The cranelift backend pre-declares every `mty_runtime_*` import in
 * declare_fns; even unused declarations land in the .o's symbol
 * table. We satisfy each one with a no-op so the host linker is
 * happy. The matrix's Mighty sources never call any of these.
 */
#include <stdint.h>

void mty_runtime_log(int64_t p, int64_t l) { (void)p; (void)l; }
void mty_runtime_print(int64_t p, int64_t l) { (void)p; (void)l; }
void mty_runtime_panic(int64_t p, int64_t l) { (void)p; (void)l; }
int64_t mty_runtime_arena_push(void) { return 0; }
void mty_runtime_arena_pop(int64_t k) { (void)k; }
int64_t mty_runtime_alloc(int64_t a, int64_t b, int64_t c) {
    (void)a; (void)b; (void)c;
    return 0;
}
int8_t mty_runtime_budget_charge(int64_t n) { (void)n; return 1; }
void mty_runtime_send(int64_t a, int64_t b, int64_t c) {
    (void)a; (void)b; (void)c;
}
int64_t mty_runtime_ask(int64_t a, int64_t b, int64_t c, int64_t d) {
    (void)a; (void)b; (void)c; (void)d;
    return 0;
}
int64_t mty_runtime_spawn(int64_t a) { (void)a; return 0; }
int64_t mty_runtime_extern_call(int64_t a, int64_t b, int64_t c) {
    (void)a; (void)b; (void)c;
    return 0;
}
void mty_runtime_log_i64(int64_t v) { (void)v; }
/* v0.42 T4 (L23 fix) — typed log/print/format runtime surface. The
 * matrix tests never call any of these from Mighty, but every entry
 * in `RUNTIME_IMPORTS` lands in the .o symbol table so the linker
 * still needs a no-op definition. */
void mty_runtime_log_i32(int32_t v) { (void)v; }
void mty_runtime_log_u32(int32_t v) { (void)v; }
void mty_runtime_log_u64(int64_t v) { (void)v; }
void mty_runtime_log_usize(int64_t v) { (void)v; }
void mty_runtime_log_f32(float v) { (void)v; }
void mty_runtime_log_f64(double v) { (void)v; }
void mty_runtime_log_bool(int8_t v) { (void)v; }
void mty_runtime_print_i32(int32_t v) { (void)v; }
void mty_runtime_print_i64(int64_t v) { (void)v; }
void mty_runtime_print_u32(int32_t v) { (void)v; }
void mty_runtime_print_u64(int64_t v) { (void)v; }
void mty_runtime_print_usize(int64_t v) { (void)v; }
void mty_runtime_print_f32(float v) { (void)v; }
void mty_runtime_print_f64(double v) { (void)v; }
void mty_runtime_print_bool(int8_t v) { (void)v; }
void mty_runtime_print_sep(void) {}
void mty_runtime_print_newline(void) {}
void mty_runtime_fmt_i32(int32_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_i64_to_slot(int64_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_u32(int32_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_u64(int64_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_usize(int64_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_f32(float v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_f64(double v, int64_t d) { (void)v; (void)d; }
void mty_runtime_fmt_bool(int8_t v, int64_t d) { (void)v; (void)d; }
void mty_runtime_str_concat(int64_t a, int64_t al, int64_t b, int64_t bl, int64_t d) {
    (void)a; (void)al; (void)b; (void)bl; (void)d;
}
/* v0.45 T1 — native std.fs runtime surface. Same rationale as above:
 * every entry in RUNTIME_IMPORTS lands in the .o symbol table even
 * for matrix tests whose Mighty source never touches std.fs. The
 * Windows MSVC linker (clang+lld-link) is strict about unresolved
 * imports, so the stub MUST cover every fs symbol or the link step
 * fails with exit 1120. */
void mty_runtime_fs_read(int64_t p, int64_t pl, int64_t d) { (void)p; (void)pl; (void)d; }
void mty_runtime_fs_read_to_string(int64_t p, int64_t pl, int64_t d) { (void)p; (void)pl; (void)d; }
void mty_runtime_fs_read_dir(int64_t p, int64_t pl, int64_t d) { (void)p; (void)pl; (void)d; }
int32_t mty_runtime_fs_write(int64_t p, int64_t pl, int64_t d, int64_t dl) { (void)p; (void)pl; (void)d; (void)dl; return 1; }
int32_t mty_runtime_fs_write_string(int64_t p, int64_t pl, int64_t s, int64_t sl) { (void)p; (void)pl; (void)s; (void)sl; return 1; }
int32_t mty_runtime_fs_append(int64_t p, int64_t pl, int64_t d, int64_t dl) { (void)p; (void)pl; (void)d; (void)dl; return 1; }
int32_t mty_runtime_fs_exists(int64_t p, int64_t pl) { (void)p; (void)pl; return 0; }
int32_t mty_runtime_fs_metadata(int64_t p, int64_t pl, int64_t d) { (void)p; (void)pl; (void)d; return 1; }
int32_t mty_runtime_fs_create_dir_all(int64_t p, int64_t pl) { (void)p; (void)pl; return 1; }
int32_t mty_runtime_fs_remove_file(int64_t p, int64_t pl) { (void)p; (void)pl; return 1; }
int32_t mty_runtime_fs_remove_dir_all(int64_t p, int64_t pl) { (void)p; (void)pl; return 1; }
"#,
    )
    .unwrap();

    let obj = work_dir.join("mty_runtime_stub.o");
    let mut cmd = Command::new(cc);
    cmd.args(["-c", "-O0"]);
    if !cfg!(target_env = "msvc") {
        cmd.arg("-fPIC");
    }
    let status = cmd
        .arg(&src)
        .arg("-o")
        .arg(&obj)
        .status()
        .unwrap_or_else(|e| panic!("invoke {cc}: {e}"));
    assert!(status.success(), "stub cc failed");

    let archive = work_dir.join("libmtyruntimestub.a");
    let status = Command::new(ar)
        .args(["rcs"])
        .arg(&archive)
        .arg(&obj)
        .status()
        .unwrap_or_else(|e| panic!("invoke {ar}: {e}"));
    assert!(status.success(), "stub ar failed");
    archive
}

/// Build a row.
///
/// `row_dir`: the row's fixture dir (contains `impl.c` and `app.mty`).
/// `marker`: optional stdout substring; when `Some`, the test asserts
/// the produced executable prints something containing it.
fn run_row(row_dir: &Path, marker: Option<&str>) {
    if let Some(reason) = maybe_skip_row() {
        eprintln!("skipping row {}: {reason}", row_dir.display());
        return;
    }
    let cc = find_cc().unwrap();
    let ar = find_ar().unwrap();

    let work = tempfile::tempdir().expect("tempdir");
    let work_dir = work.path();

    // 1. Compile impl.c -> impl.o
    let impl_c = row_dir.join("impl.c");
    let impl_o = work_dir.join("impl.o");
    let mut cmd = Command::new(&cc);
    cmd.args(["-c", "-O0"]);
    // `-fPIC` is unsupported by clang's MSVC target. Position-independent
    // code is the default on linux-x86_64 with `-O0` anyway; emitting
    // it explicitly only matters for shared-library output and we ship
    // archives.
    if !cfg!(target_env = "msvc") {
        cmd.arg("-fPIC");
    }
    let status = cmd
        .arg(&impl_c)
        .arg("-o")
        .arg(&impl_o)
        .status()
        .unwrap_or_else(|e| panic!("invoke {cc}: {e}"));
    assert!(status.success(), "cc failed for {}", impl_c.display());

    // 2. Archive into libmtyrow.a
    let archive = work_dir.join("libmtyrow.a");
    let status = Command::new(&ar)
        .args(["rcs"])
        .arg(&archive)
        .arg(&impl_o)
        .status()
        .unwrap_or_else(|e| panic!("invoke {ar}: {e}"));
    assert!(status.success(), "ar failed");

    // 2b. Runtime symbol stub: every Mighty .o references the
    // `mty_runtime_*` symbol set that codegen pre-declares as imports.
    // Without a stub archive these would surface as undefined-symbol
    // errors at link time — see `build_runtime_stub` for the rationale.
    let runtime_archive = build_runtime_stub(work_dir, &cc, &ar);

    // 3. Build app.mty
    let app_src = std::fs::read_to_string(row_dir.join("app.mty")).expect("app.mty");
    let opts = BuildOptions {
        target: BuildTarget::Native,
        mode: BuildMode::Release,
        out_dir: work_dir.to_path_buf(),
        binary_name: "mty_row_bin".into(),
        no_component: false,
        wasi_preview: mty_driver::build::WasiPreview::default(),
        user_wit: None,
        extern_libs: vec![
            ExternLib {
                name: "mtyrow".into(),
                kind: "static".into(),
                path: Some(archive.to_string_lossy().into_owned()),
                ..Default::default()
            },
            ExternLib {
                name: "mtyruntimestub".into(),
                kind: "static".into(),
                path: Some(runtime_archive.to_string_lossy().into_owned()),
                ..Default::default()
            },
        ],
        manifest_dir: Some(work_dir.to_path_buf()),
        build_config: None,
    };
    let outcome = build_native(app_src, row_dir.display().to_string(), &opts);
    let bin = match outcome {
        BuildOutcome::NativeOk(p) => p,
        BuildOutcome::NativeOkNoLinker { object_path: p, .. } => {
            // No linker was discovered during the driver build. The
            // matrix needs a runnable binary, so try the link step
            // directly and surface its exact error instead of silently
            // skipping execution.
            let exe = work_dir.join(if cfg!(windows) {
                "mty_row_bin.exe"
            } else {
                "mty_row_bin"
            });
            match mty_codegen_cranelift::object::link_executable_with_libs(
                &mty_codegen_cranelift::object::ObjectArtifact {
                    object_path: p.clone(),
                    triple: target_lexicon::Triple::host(),
                },
                &exe,
                BuildMode::Release,
                &mty_driver::build::build_linker_args(
                    &opts.extern_libs,
                    opts.manifest_dir.as_deref(),
                ),
            ) {
                Ok(a) => a.binary_path,
                Err(e) => panic!(
                    "linker failed for {} (STARDUST_LINKER={:?}): {e}",
                    row_dir.display(),
                    std::env::var("STARDUST_LINKER").ok(),
                ),
            }
        }
        BuildOutcome::NativeLinkError { object_path, error } => panic!(
            "linker failed for {} after writing {}: {error}",
            row_dir.display(),
            object_path.display()
        ),
        BuildOutcome::FrontendError => panic!("frontend error in {}", row_dir.display()),
        BuildOutcome::BackendError(e) => panic!("backend error: {e}"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome shape"),
    };

    // 4. Execute the produced binary, capture stdout
    let out = Command::new(&bin).output().unwrap_or_else(|e| {
        panic!("run {}: {e}", bin.display());
    });
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    // Mighty's `main` returns the tail-expression value as a register;
    // the cranelift wrapper currently forwards that to the process
    // exit code (so `let _ = mty_row01()` ends up exiting with 42).
    // We therefore don't require exit-zero — the row's contract is
    // "binary runs to completion and the stdout marker appears".
    // Catch the genuine pathological cases (segfault, abort) via a
    // negative status check.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let bin_path = bin.display();
        assert!(
            out.status.signal().is_none(),
            "binary {bin_path} terminated by signal {:?}\nstdout: {stdout}\nstderr: {stderr}",
            out.status.signal(),
        );
    }
    #[cfg(windows)]
    {
        // On Windows, negative status codes correspond to NTSTATUS
        // error codes (access violation, stack overflow, etc.). Any
        // status above 0x80000000 signals a hard fault.
        if let Some(code) = out.status.code() {
            let bin_path = bin.display();
            assert!(
                (code as u32) < 0x80000000,
                "binary {bin_path} crashed with NTSTATUS {code:#x}\nstdout: {stdout}\nstderr: {stderr}",
            );
        }
    }
    if let Some(m) = marker {
        assert!(
            stdout.contains(m),
            "expected marker {m:?} in stdout for {}\nstdout: {stdout}\nstderr: {stderr}",
            row_dir.display()
        );
    }
    let _ = stderr;
}

#[test]
fn row_01_no_args() {
    let mut p = matrix_root();
    p.push("row_01_no_args");
    run_row(&p, Some("row01:42"));
}

#[test]
fn row_02_two_i32() {
    let mut p = matrix_root();
    p.push("row_02_two_i32");
    run_row(&p, Some("row02:7+35=42"));
}

#[test]
fn row_03_ptr_in() {
    let mut p = matrix_root();
    p.push("row_03_ptr_in");
    run_row(&p, Some("row03:"));
}

#[test]
fn row_04_out_ptr() {
    let mut p = matrix_root();
    p.push("row_04_out_ptr");
    run_row(&p, Some("row04:"));
}

#[test]
fn row_05_struct_by_value() {
    let mut p = matrix_root();
    p.push("row_05_struct_by_value");
    run_row(&p, Some("row05:"));
}

#[test]
fn row_06_struct_by_ptr() {
    let mut p = matrix_root();
    p.push("row_06_struct_by_ptr");
    run_row(&p, Some("row06:"));
}

#[test]
fn row_07_return_struct() {
    let mut p = matrix_root();
    p.push("row_07_return_struct");
    run_row(&p, Some("row07:"));
}

#[test]
fn row_08_array_ptr() {
    let mut p = matrix_root();
    p.push("row_08_array_ptr");
    run_row(&p, Some("row08:"));
}

#[test]
fn row_09_str_in() {
    let mut p = matrix_root();
    p.push("row_09_str_in");
    run_row(&p, Some("row09:"));
}

#[test]
fn row_10_str_out() {
    let mut p = matrix_root();
    p.push("row_10_str_out");
    run_row(&p, Some("row10:"));
}

#[test]
fn row_11_fn_ptr() {
    let mut p = matrix_root();
    p.push("row_11_fn_ptr");
    run_row(&p, Some("row11:"));
}

#[test]
fn row_12_str_slice_ascii() {
    // v0.46 T3 (L52 fix): `extern c fn f(s: Str)` expands to
    // `void f(const char*, size_t)` at the call site. The Mighty
    // source `mty_row12_echo("hello-from-mighty")` passes the literal's
    // ptr and 17 (the byte count) — the C side prints the echo'd bytes
    // and the marker line lands in stdout.
    let mut p = matrix_root();
    p.push("row_12_str_slice");
    run_row(&p, Some("row12:echo='hello-from-mighty',len=17"));
}

#[test]
fn row_12_str_slice_empty() {
    // Empty string round trip — len=0, ptr arg is irrelevant on the C
    // side. Pins that the call still survives the trip without
    // dereferencing whatever the codegen picks as the ptr operand.
    let mut p = matrix_root();
    p.push("row_12_str_slice");
    run_row(&p, Some("row12:empty,len=0"));
}

#[test]
fn row_12_str_slice_utf8() {
    // UTF-8 multi-byte round trip — pins that `len` is the BYTE count,
    // not the codepoint count. "héllo" is 6 bytes.
    let mut p = matrix_root();
    p.push("row_12_str_slice");
    run_row(&p, Some("row12:utf8='héllo',bytes=6"));
}
