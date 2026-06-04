#![cfg(feature = "host-toolchain")]
//! v0.42 T1 — native-binary regression tests for the L28 + L21 Vec
//! liveness bugs (`docs/internals/native-vec-liveness-v042.md`).
//!
//! Pre-v0.41 T3 `mty build` of a plain `fn main()` that did
//! `let mut v: Vec[T] = Vec.new(); v = v.push(x)` came back with
//! `v.len() == 0` (L28) or SIGSEGV'd inside a nested loop that read a
//! Vec param (L21). The fix was the auto-push of an implicit
//! `mty_runtime_arena_push` at `main`-entry (the cranelift backend now
//! does this in `lower.rs::lower_blocks`; v0.42 T1 ported the same fix
//! to the LLVM backend). Without that push the runtime allocator
//! returned NULL on every `Vec.new()` / grow, and silently corrupted
//! state from then on.
//!
//! The JIT-side coverage lives in
//! `crates/mty-codegen-cranelift/tests/vec_push_native.rs` (L28) and
//! `crates/mty-codegen-cranelift/tests/vec_liveness_v042.rs` (L21).
//! Those tests pre-supply a real allocator through the JIT symbol
//! table, so they ALWAYS had a working `mty_runtime_alloc`. The bug
//! lived strictly on the AOT path where the runtime archive itself
//! drives allocator state. This file closes that gap by:
//!
//!  1. Building a tiny C "runtime + FFI printer" archive that exposes
//!     a real `malloc`-backed `mty_runtime_alloc` plus the rest of the
//!     `mty_runtime_*` import surface as no-ops, plus a
//!     `repro_print_i64` extern that the Mighty side calls to surface
//!     a computed value through native stdout (Mighty's own `log()`
//!     only accepts string literals — L23 — so a computed `v.len()`
//!     can't be printed without an FFI scalar printer).
//!  2. Routing the test Mighty programs through `build_native` with
//!     that archive as the only `[[extern_lib]]`.
//!  3. Executing the produced `.exe` and asserting stdout reports the
//!     expected value (`L28: 5`, `L21: 12`, …).
//!
//! If the auto-arena-push regresses (or anyone removes the L28 fix in
//! the cranelift backend), the test goes red because `mty_runtime_alloc`
//! returns 0 and the printed `v.len()` collapses to 0 (or the program
//! crashes outright on L21's nested-loop read).
//!
//! The harness skips itself on hosts without a C toolchain (`cc` / `clang`
//! / `gcc`) or without an `ar` (`llvm-ar`); we already gate the v0.36
//! T2 `extern_c_matrix` tests the same way. Adding a v0.42 floor here
//! doesn't ratchet up CI dependencies — every CI runner that already
//! runs `extern_c_matrix` runs this too.

use std::path::{Path, PathBuf};
use std::process::Command;

use mty_codegen_cranelift::artifact::BuildMode;
use mty_driver::manifest::ExternLib;
use mty_driver::{build_native, BuildOptions, BuildOutcome, BuildTarget};

/// Locate a C compiler. Same precedence as `extern_c_matrix.rs`: prefer
/// `clang` because the driver's linker auto-detection picks it on most
/// hosts; on Windows also probe the rustup-bundled LLVM dir.
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

/// Locate `ar` / `llvm-ar` — mirrors `extern_c_matrix.rs::find_ar`.
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

/// Side-effect: if the driver's linker auto-detection fails, point
/// `MTY_LINKER` at the C compiler we found (clang drives its own
/// linker). Mirrors the same hook in `extern_c_matrix.rs::maybe_skip_row`.
fn maybe_skip() -> Option<&'static str> {
    let cc = find_cc();
    if cc.is_none() {
        return Some("no C compiler on PATH");
    }
    if find_ar().is_none() {
        return Some("no `ar` on PATH");
    }
    if mty_codegen_cranelift::object::find_linker().is_none() {
        if let Some(c) = cc {
            std::env::set_var("MTY_LINKER", &c);
            if mty_codegen_cranelift::object::find_linker().is_none() {
                return Some("no linker on PATH");
            }
        }
    }
    None
}

/// Build a static archive carrying:
/// - arena-aware `mty_runtime_alloc`: returns 0 when no
///   `mty_runtime_arena_push` has been called, otherwise malloc-leaks
///   the requested block. This mirrors the production runtime's
///   `crates/mty-runtime/src/arena.rs::ArenaStack::alloc` shape
///   (returns `None` with no frame). Crucially, it MAKES THIS TEST
///   FAIL if the v0.41 T3 auto-arena-push at main-entry ever
///   regresses — without that push, the first `Vec.new()` returns 0,
///   subsequent stores write to NULL, and the L28 assertion fails
///   (or the binary crashes).
/// - `mty_runtime_arena_push`: increments a global depth counter so
///   the alloc check can tell "frame active" from "no frame".
/// - `mty_runtime_arena_pop`: decrements (saturating at 0).
/// - no-op stubs for the rest of the `mty_runtime_*` import surface
/// - `repro_print_i64`: print `tag=value\n` to stdout for the
///   Mighty-side assertions (Mighty's own `log()` only accepts
///   string literals — L23 — so a computed `v.len()` needs an FFI
///   scalar printer).
///
/// We deliberately do NOT free anything past pop: a test process
/// exits in milliseconds and a few hundred bytes of leak per push
/// iteration is acceptable. (The IDE's vendored real-bumpalo runtime
/// takes the same shortcut for its main fallback arena.)
fn build_runtime_and_printer(work_dir: &Path, cc: &str, ar: &str) -> PathBuf {
    let src = work_dir.join("rt_and_print.c");
    std::fs::write(
        &src,
        r#"/* v0.42 T1 — arena-aware runtime + FFI printer for the
 * Vec-liveness regression tests. The arena_push counter mirrors the
 * production runtime: alloc returns 0 if no frame is pushed, so the
 * v0.41 T3 auto-arena-push at main-entry is the contract this test
 * exercises. If that auto-push ever regresses, L28's first Vec.new()
 * returns NULL, the subsequent store crashes (or v.len() reads 0),
 * and the test fails LOUDLY rather than silently passing.
 */
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* arena depth counter — production has a per-thread `ArenaStack` of
 * bumpalo frames; the regression check we need is just "is there at
 * least one frame active?", so a single counter suffices. Process is
 * single-threaded across each test. */
static int64_t g_arena_depth = 0;

/* runtime surface */
void mty_runtime_log(int64_t p, int64_t l) { (void)p; (void)l; }
void mty_runtime_print(int64_t p, int64_t l) { (void)p; (void)l; }
void mty_runtime_panic(int64_t p, int64_t l) {
    /* tag the abort so a regression's failure mode is loud */
    fprintf(stderr, "mty_runtime_panic (len=%lld)\n", (long long)l);
    fflush(stderr);
    abort();
}
int64_t mty_runtime_arena_push(void) { g_arena_depth += 1; return g_arena_depth; }
void mty_runtime_arena_pop(int64_t h) {
    (void)h;
    if (g_arena_depth > 0) g_arena_depth -= 1;
}

/* Arena-aware allocator — NULL when no frame, malloc-leak otherwise.
 * Mirrors `crates/mty-runtime/src/arena.rs::ArenaStack::alloc`
 * shape: production returns `None` (= 0) on no-frame. */
int64_t mty_runtime_alloc(int64_t size, int64_t align, int64_t zero) {
    (void)align;
    if (g_arena_depth <= 0) return 0;
    size_t n = (size > 0) ? (size_t)size : 1;
    void *p = malloc(n);
    if (!p) return 0;
    if (zero) memset(p, 0, n);
    return (int64_t)(intptr_t)p;
}

int8_t mty_runtime_budget_charge(int64_t b) { (void)b; return 1; }
void mty_runtime_send(int64_t a, int64_t b, int64_t c) { (void)a; (void)b; (void)c; }
int64_t mty_runtime_ask(int64_t a, int64_t b, int64_t c, int64_t d) {
    (void)a; (void)b; (void)c; (void)d; return 0;
}
int64_t mty_runtime_spawn(int64_t a) { (void)a; return 0; }
int64_t mty_runtime_extern_call(int64_t a, int64_t b, int64_t c) {
    (void)a; (void)b; (void)c; return 0;
}
void mty_runtime_log_i64(int64_t v) { printf("%lld\n", (long long)v); fflush(stdout); }
void mty_runtime_log_i32(int32_t v) { (void)v; }
void mty_runtime_log_u32(uint32_t v) { (void)v; }
void mty_runtime_log_u64(uint64_t v) { (void)v; }
void mty_runtime_log_usize(uint64_t v) { (void)v; }
void mty_runtime_log_f32(float v) { (void)v; }
void mty_runtime_log_f64(double v) { (void)v; }
void mty_runtime_log_bool(int8_t v) { (void)v; }
void mty_runtime_print_i32(int32_t v) { (void)v; }
void mty_runtime_print_i64(int64_t v) { (void)v; }
void mty_runtime_print_u32(uint32_t v) { (void)v; }
void mty_runtime_print_u64(uint64_t v) { (void)v; }
void mty_runtime_print_usize(uint64_t v) { (void)v; }
void mty_runtime_print_f32(float v) { (void)v; }
void mty_runtime_print_f64(double v) { (void)v; }
void mty_runtime_print_bool(int8_t v) { (void)v; }
void mty_runtime_print_sep(void) {}
void mty_runtime_print_newline(void) {}
void mty_runtime_fmt_i32(int32_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_i64_to_slot(int64_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_u32(uint32_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_u64(uint64_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_usize(uint64_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_f32(float v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_f64(double v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_fmt_bool(int8_t v, int64_t slot) { (void)v; (void)slot; }
void mty_runtime_str_concat(
    int64_t lhs_ptr,
    int64_t lhs_len,
    int64_t rhs_ptr,
    int64_t rhs_len,
    int64_t out_slot
) {
    (void)lhs_ptr; (void)lhs_len; (void)rhs_ptr; (void)rhs_len; (void)out_slot;
}

/* FFI printer — Mighty's `log()` only accepts string literals (L23),
 * so a computed `v.len()` round-trips through this scalar printer.
 * The Mighty side passes the tag as a fixed integer constant so we
 * can write to stdout in a shape the harness greps for. */
void repro_print_i64(int64_t tag, int64_t value) {
    printf("L%lld=%lld\n", (long long)tag, (long long)value);
    fflush(stdout);
}
/* v0.46 T4 DirIter surface; v0.47 T4 auto-Drop makes the codegen
 * reference these in every program, so the Windows MSVC linker needs
 * them even though this Vec-liveness program never opens a dir. */
int64_t mty_runtime_fs_dir_open(int64_t p, int64_t pl) { (void)p; (void)pl; return 0; }
int32_t mty_runtime_fs_dir_next(int64_t h, int64_t d) { (void)h; (void)d; return 0; }
void mty_runtime_fs_dir_close(int64_t h) { (void)h; }
/* v0.49 — native std.crypto / std.encoding imports. Same MSVC rule:
 * the codegen references every runtime import, so each needs a stub. */
void mty_runtime_crypto_sha256(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
void mty_runtime_crypto_sha512(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
void mty_runtime_crypto_blake3(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
void mty_runtime_crypto_hmac_sha256(int64_t k, int64_t kl, int64_t m, int64_t ml, int64_t dst) { (void)k; (void)kl; (void)m; (void)ml; (void)dst; }
void mty_runtime_encoding_hex_encode(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
void mty_runtime_encoding_base64_encode(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
void mty_runtime_encoding_base64_encode_url_no_pad(int64_t d, int64_t dl, int64_t dst) { (void)d; (void)dl; (void)dst; }
"#,
    )
    .unwrap();

    let obj = work_dir.join("rt_and_print.o");
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
    assert!(status.success(), "rt cc failed");

    let archive = work_dir.join("libmtyrt_and_print.a");
    let status = Command::new(ar)
        .args(["rcs"])
        .arg(&archive)
        .arg(&obj)
        .status()
        .unwrap_or_else(|e| panic!("invoke {ar}: {e}"));
    assert!(status.success(), "rt ar failed");
    archive
}

/// Build a Mighty source string into a native binary, link against
/// the runtime + printer archive, and run it. Returns (exit_code,
/// stdout).
fn build_and_run(name: &str, src: &str) -> Option<(i32, String)> {
    if let Some(reason) = maybe_skip() {
        eprintln!("[vec_liveness_native_v042] skipping {name}: {reason}");
        return None;
    }
    let cc = find_cc().expect("cc");
    let ar = find_ar().expect("ar");

    let work = tempfile::tempdir().expect("tempdir");
    let work_dir = work.path();
    let archive = build_runtime_and_printer(work_dir, &cc, &ar);

    // Manifest-style extern_lib. Resolves to the bare archive path
    // when given a `path` (see `build_linker_args`); the linker
    // accepts archives positionally.
    let archive_str = archive.to_string_lossy().into_owned();
    let lib = ExternLib {
        name: "mtyrt_and_print".into(),
        kind: "static".into(),
        path: Some(archive_str),
        link_args: Vec::new(),
        link_args_linux: Vec::new(),
        link_args_macos: Vec::new(),
        link_args_windows: Vec::new(),
    };

    let opts = BuildOptions {
        target: BuildTarget::Native,
        mode: BuildMode::Debug,
        out_dir: work_dir.to_path_buf(),
        binary_name: name.into(),
        no_component: false,
        wasi_preview: Default::default(),
        user_wit: None,
        extern_libs: vec![lib],
        manifest_dir: None,
        build_config: None,
    };

    let outcome = build_native(src.to_string(), format!("{name}.mty"), &opts);
    let exe_path = match outcome {
        BuildOutcome::NativeOk(p) => p,
        BuildOutcome::NativeOkNoLinker { object_path: p, .. } => {
            eprintln!(
                "[vec_liveness_native_v042] {name}: only emitted .o ({}); skipping execute step",
                p.display()
            );
            return None;
        }
        BuildOutcome::NativeLinkError { object_path, error } => {
            eprintln!(
                "[vec_liveness_native_v042] {name}: linker failed after emitting .o ({}); skipping execute step: {error}",
                object_path.display()
            );
            return None;
        }
        BuildOutcome::BackendError(e) => panic!("[{name}] build_native: {e}"),
        BuildOutcome::FrontendError => panic!("[{name}] frontend rejected the source"),
        BuildOutcome::WasmOk(_) => panic!("[{name}] wrong outcome (wasm) for native build"),
    };

    let out = Command::new(&exe_path)
        .output()
        .unwrap_or_else(|e| panic!("[{name}] spawn {}: {e}", exe_path.display()));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code().unwrap_or(-1);
    Some((code, stdout))
}

// =====================================================================
// L28 — `v = v.push(x)` rebind across loop back-edge
// =====================================================================

/// The IDE-equivalent shape: a flat `while` loop calling
/// `v = v.push(x)` then counting `v.len()` after exiting. Pre-fix
/// (without v0.41 T3 auto-arena-push) this printed `L28=0`.
#[test]
fn l28_push_loop_grows_native_binary() {
    let src = r#"
extern c {
  fn repro_print_i64(tag: I32, value: I32) -> I32
}

fn main() {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 5 {
    v = v.push(65)
    i = i + 1
  }
  let mut n: I32 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  let _ = repro_print_i64(28, n)
}
"#;
    let Some((code, stdout)) = build_and_run("l28_push_loop_grows", src) else {
        return; // host can't build — already logged in build_and_run
    };
    assert_eq!(code, 0, "L28 binary exit code mismatch — stdout:\n{stdout}");
    assert!(
        stdout.contains("L28=5"),
        "L28 regression: expected `L28=5` in stdout, got:\n{stdout}"
    );
}

/// Same shape across MANY iterations, forcing several capacity
/// doublings (4 → 8 → 16 → 32 → 64 → 128). Exercises
/// `emit_memcpy_dynamic_bytes` on the live prefix per grow.
#[test]
fn l28_push_loop_through_multiple_reallocs_native() {
    let src = r#"
extern c {
  fn repro_print_i64(tag: I32, value: I32) -> I32
}

fn main() {
  let mut v: Vec[I32] = Vec.new()
  let mut i: USize = 0
  while i < 100 {
    v = v.push(1)
    i = i + 1
  }
  let mut n: I32 = 0
  let mut j: USize = 0
  while j < v.len() {
    n = n + 1
    j = j + 1
  }
  let _ = repro_print_i64(280, n)
}
"#;
    let Some((code, stdout)) = build_and_run("l28_push_loop_reallocs", src) else {
        return;
    };
    assert_eq!(
        code, 0,
        "L28-reallocs exit code mismatch — stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("L280=100"),
        "L28-reallocs regression: expected `L280=100` in stdout, got:\n{stdout}"
    );
}

// =====================================================================
// L21 — Vec param read inside a nested-loop body's branch arm
// =====================================================================

/// The simplified L21 reproducer: a Vec param read at the top of the
/// fn AND inside a nested loop body that does NOT have the Vec in its
/// condition. Pre-fix native codegen segfaulted on the in-loop read;
/// post-fix it returns the right value.
#[test]
fn l21_vec_param_read_in_nested_loop_native_binary() {
    let src = r#"
extern c {
  fn repro_print_i64(tag: I32, value: I32) -> I32
}

fn sum_visible(buf: Vec[I32], rows: USize) -> I32 {
  let total: USize = buf.len()
  let mut acc: I32 = 0
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

fn main() {
  let mut v: Vec[I32] = Vec.new()
  let mut k: USize = 0
  while k < 4 {
    v = v.push(1)
    k = k + 1
  }
  let _ = repro_print_i64(21, sum_visible(v, 3))
}
"#;
    let Some((code, stdout)) = build_and_run("l21_vec_param_nested_loop", src) else {
        return;
    };
    assert_eq!(code, 0, "L21 exit code mismatch — stdout:\n{stdout}");
    // 4 elements * 3 rows = 12.
    assert!(
        stdout.contains("L21=12"),
        "L21 regression: expected `L21=12` in stdout, got:\n{stdout}"
    );
}

/// L21 stress: two consecutive nested loops both reading the same Vec
/// param ONLY inside their inner bodies. If a future regalloc/spill
/// change drops the Vec param's slot across either back-edge, this
/// trips before a user-facing IDE workflow does.
#[test]
fn l21_two_nested_loops_back_to_back_native() {
    let src = r#"
extern c {
  fn repro_print_i64(tag: I32, value: I32) -> I32
}

fn count_twice(buf: Vec[I32], rows: USize) -> I32 {
  let mut acc: I32 = 0
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

fn main() {
  let mut v: Vec[I32] = Vec.new()
  let mut k: USize = 0
  while k < 3 {
    v = v.push(1)
    k = k + 1
  }
  let _ = repro_print_i64(210, count_twice(v, 2))
}
"#;
    let Some((code, stdout)) = build_and_run("l21_two_nested_loops", src) else {
        return;
    };
    assert_eq!(code, 0, "L21-two exit code mismatch — stdout:\n{stdout}");
    // 3 elems * 2 rows * 2 outer-loops = 12.
    assert!(
        stdout.contains("L210=12"),
        "L21-two regression: expected `L210=12` in stdout, got:\n{stdout}"
    );
}
