#![cfg(feature = "host-toolchain")]
//! v0.42 T6 (L22 diagnostics polish) — end-to-end tests for `mty check`
//! covering the three lesson-L22 fixes:
//!
//!   * Fix 1 — type errors carry the real expression span, not the
//!     enclosing fn's 1:1 header. Two errors in the same fn must report
//!     two distinct `:line:col` positions.
//!   * Fix 2 — `NO_COLOR=1` and `TERM=dumb` suppress every ANSI SGR
//!     escape in the report so the IDE can stop stripping ANSI from
//!     `mty check` output.
//!   * Fix 3 — `mty check` surfaces parse glitches (`let = 42;`) and
//!     undefined-identifier uses (`log(undefined_thing)`) instead of
//!     printing `ok:`. Both must exit non-zero with a recognisable
//!     MT-code.
//!
//! Lesson source: `mighty-ide/docs/mighty-language-lessons.md` L22.

use std::path::PathBuf;
use std::process::Command;

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

fn write_tempfile(name: &str, src: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("mty_check_t6_{}_{}.mty", std::process::id(), name));
    std::fs::write(&p, src).expect("write temp .mty");
    p
}

/// Spawn `mty check <path>` with an explicit env. Returns (exit_code,
/// stdout, stderr).
fn run_check(path: &PathBuf, env: &[(&str, &str)]) -> (i32, String, String) {
    let mut cmd = Command::new(mty_bin());
    cmd.arg("check").arg(path);
    // Always start from a known-clean env w.r.t. our two flags so a
    // hosting CI worker that exports `NO_COLOR=1` doesn't poison the
    // colored-default tests below.
    cmd.env_remove("NO_COLOR");
    cmd.env_remove("TERM");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn mty check");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// L22 fix 1: a file with two distinct type errors in the same fn must
/// produce two different `:line:col` positions instead of collapsing
/// both onto the fn header (which was the pre-v0.42 behavior — both
/// landed at `…:1:1`).
#[test]
fn type_errors_report_distinct_positions() {
    let path = write_tempfile(
        "distinct_spans",
        "fn demo() {\n    let x: I32 = \"hello\";\n    let y: Str = 42;\n}\n",
    );
    let (code, _out, err) = run_check(&path, &[("NO_COLOR", "1")]);
    assert_eq!(code, 1, "expected non-zero exit on type errors: {err}");
    // Pre-v0.42 T6 both diagnostics anchored at `:1:1` (the `fn` line).
    // Post-fix they must point at the literal on lines 2 and 3.
    assert!(
        err.contains(":2:") && err.contains(":3:"),
        "expected per-line spans for the two type errors, got: {err}"
    );
    // And neither error should collapse to the fn header.
    let header_hits = err.matches(":1:1").count();
    assert_eq!(
        header_hits, 0,
        "no diagnostic should anchor at the fn header (`1:1`); got: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

/// L22 fix 2: `NO_COLOR=1` must produce output free of ANSI SGR escape
/// sequences. The IDE currently strips ANSI via `mui-sys/src/diagnostics.rs::strip_ansi`;
/// once this test passes, that helper is a no-op safety net.
#[test]
fn no_color_env_suppresses_ansi_escapes() {
    let path = write_tempfile("no_color", "fn demo() {\n    let x: I32 = \"hello\";\n}\n");
    let (code, _out, err) = run_check(&path, &[("NO_COLOR", "1")]);
    assert_eq!(code, 1, "expected non-zero exit: {err}");
    assert!(
        !err.contains('\x1b'),
        "expected no ANSI escape (0x1B) under NO_COLOR=1, got: {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

/// L22 fix 2 (cont.): `TERM=dumb` is the other conventional opt-out.
/// Same expectation as `NO_COLOR=1`.
#[test]
fn term_dumb_env_suppresses_ansi_escapes() {
    let path = write_tempfile("term_dumb", "fn demo() {\n    let x: I32 = \"hello\";\n}\n");
    let (code, _out, err) = run_check(&path, &[("TERM", "dumb")]);
    assert_eq!(code, 1, "expected non-zero exit: {err}");
    assert!(
        !err.contains('\x1b'),
        "expected no ANSI escape (0x1B) under TERM=dumb, got: {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

/// Sanity for fix 2: without either opt-out, the colored default still
/// emits at least one SGR escape. Guards against an over-zealous strip.
#[test]
fn default_env_still_emits_ansi_escapes() {
    let path = write_tempfile(
        "colored_default",
        "fn demo() {\n    let x: I32 = \"hello\";\n}\n",
    );
    // Don't set NO_COLOR / TERM. (We also wipe any inherited values in
    // `run_check`, so this is the pristine "colored default" path.)
    let (code, _out, err) = run_check(&path, &[]);
    assert_eq!(code, 1, "expected non-zero exit: {err}");
    assert!(
        err.contains('\x1b'),
        "expected at least one ANSI escape in colored output, got: {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

/// L22 fix 3a: `let = 42;` is a parse glitch (no binding pattern after
/// `let`). Pre-v0.42 the parser silently produced a `LET_STMT` with no
/// pattern child and `mty check` printed `ok:`. Post-fix it must emit
/// MT0001 and exit non-zero.
#[test]
fn parse_error_let_eq_surfaces_in_check() {
    let path = write_tempfile("parse_let_eq", "fn demo() {\n    let = 42;\n}\n");
    let (code, _out, err) = run_check(&path, &[("NO_COLOR", "1")]);
    assert_eq!(
        code, 1,
        "expected non-zero exit for `let = 42`, got stderr: {err}"
    );
    assert!(
        err.contains("MT0001"),
        "expected MT0001 (parser error) in stderr, got: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

/// L22 fix 3b: `log(undefined_thing)` is an unresolved-identifier use
/// in a top-level fn body. Pre-v0.42 the type checker silently typed
/// the arg as a fresh inference variable (slice-3 A21 permissive
/// fallback) and `mty check` printed `ok:`. Post-fix `mty check`'s
/// `strict_resolution` opt-in promotes the case to MT2021.
#[test]
fn undefined_identifier_surfaces_in_check() {
    let path = write_tempfile("undef_ident", "fn demo() {\n    log(undefined_thing);\n}\n");
    let (code, _out, err) = run_check(&path, &[("NO_COLOR", "1")]);
    assert_eq!(
        code, 1,
        "expected non-zero exit for undefined identifier, got stderr: {err}"
    );
    assert!(
        err.contains("MT2021"),
        "expected MT2021 (unresolved value) in stderr, got: {err}"
    );
    assert!(
        err.contains("undefined_thing"),
        "expected the offending name in stderr, got: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

/// Regression guard for fix 3: a clean file still prints `ok:` and
/// exits 0. The strict_resolution widening must not affect well-formed
/// programs.
#[test]
fn clean_file_still_reports_ok() {
    let path = write_tempfile(
        "clean",
        "fn demo() {\n    let x: I32 = 42;\n    log(\"hi\");\n}\n",
    );
    let (code, out, err) = run_check(&path, &[("NO_COLOR", "1")]);
    assert_eq!(code, 0, "expected zero exit on clean file. stderr: {err}");
    assert!(out.contains("ok:"), "expected `ok:` on stdout, got: {out}");
    let _ = std::fs::remove_file(&path);
}
