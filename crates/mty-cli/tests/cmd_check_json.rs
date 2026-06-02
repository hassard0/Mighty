#![cfg(feature = "host-toolchain")]
//! v0.45 T3 — end-to-end tests for `mty check --json`, the new
//! structured-result document surface.
//!
//! The flag emits ONE JSON document on stdout (not NDJSON, not pretty
//! text) with the shape:
//!
//! ```json
//! {
//!   "ok": false,
//!   "path": "...",
//!   "diagnostics": [
//!     {
//!       "code": "MT2001",
//!       "severity": "error",
//!       "message": "...",
//!       "span": {"file":"...","line":N,"col":N,"end_line":N,"end_col":N}
//!     }
//!   ]
//! }
//! ```
//!
//! Covered:
//!
//! 1. Clean file → `{ok:true, diagnostics:[]}`, exit 0, no ariadne bytes.
//! 2. File with two type errors → 2 distinct line:col diagnostics, exit 1.
//! 3. File referencing an undefined name → MT2021, exit 1.
//! 4. File with a parse glitch → MT0xxx, exit 1.
//! 5. Back-compat: without `--json`, ariadne pretty output is unchanged.
//!
//! T6 covers (1) (3) (4) (5) for the pretty path; this file mirrors
//! the four cases under `--json`.

use std::path::PathBuf;
use std::process::Command;

fn mty_bin() -> &'static str {
    env!("CARGO_BIN_EXE_mty")
}

fn write_tempfile(name: &str, src: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mty_check_v45t3_{}_{}.mty",
        std::process::id(),
        name
    ));
    std::fs::write(&p, src).expect("write temp .mty");
    p
}

/// Spawn `mty check --json <path>`. Returns `(exit_code, stdout, stderr)`.
fn run_check_json(path: &PathBuf) -> (i32, String, String) {
    let mut cmd = Command::new(mty_bin());
    cmd.arg("check").arg("--json").arg(path);
    cmd.env("NO_COLOR", "1"); // Belt-and-braces — JSON path emits no ANSI anyway.
    let out = cmd.output().expect("spawn mty check --json");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Spawn `mty check <path>` (no `--json`) so we can confirm the
/// ariadne pretty path is unchanged.
fn run_check_pretty(path: &PathBuf) -> (i32, String, String) {
    let mut cmd = Command::new(mty_bin());
    cmd.arg("check").arg(path);
    cmd.env("NO_COLOR", "1");
    let out = cmd.output().expect("spawn mty check");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

// ---- (1) clean file ----

#[test]
fn json_clean_file_is_ok_zero_exit() {
    let path = write_tempfile("clean", "fn demo() {\n    log(\"hi\");\n}\n");
    let (code, out, err) = run_check_json(&path);
    assert_eq!(code, 0, "clean file should exit 0. stderr: {err}");
    // No ariadne text on stderr — clean parseable output only.
    assert!(
        !err.contains('\x1b'),
        "no ANSI escapes expected under --json. stderr: {err:?}"
    );
    let doc: serde_json::Value =
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON: {e}; stdout: {out}"));
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["path"].as_str().unwrap(), path.display().to_string());
    assert!(doc["diagnostics"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_file(&path);
}

// ---- (2) two type errors, distinct line:col ----

#[test]
fn json_two_type_errors_distinct_positions() {
    let path = write_tempfile(
        "two_errs",
        "fn demo() {\n    let x: I32 = \"hello\";\n    let y: Str = 42;\n}\n",
    );
    let (code, out, err) = run_check_json(&path);
    assert_eq!(code, 1, "two type errors should exit 1. stderr: {err}");
    let doc: serde_json::Value =
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON: {e}; stdout: {out}"));
    assert_eq!(doc["ok"], false);
    let diags = doc["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diags.len() >= 2,
        "expected ≥2 diagnostics, got {}: {out}",
        diags.len()
    );
    // The two type errors must report DIFFERENT (line, col) — not both
    // collapsed onto the fn header.
    let mut positions: Vec<(u64, u64)> = diags
        .iter()
        .filter(|d| d["severity"] == "error")
        .map(|d| {
            (
                d["span"]["line"].as_u64().unwrap(),
                d["span"]["col"].as_u64().unwrap(),
            )
        })
        .collect();
    positions.sort();
    positions.dedup();
    assert!(
        positions.len() >= 2,
        "expected ≥2 distinct error positions, got {positions:?} from {out}"
    );
    // Spans should include end_line/end_col.
    for d in diags {
        let span = &d["span"];
        assert!(span["line"].is_u64());
        assert!(span["col"].is_u64());
        assert!(span["end_line"].is_u64());
        assert!(span["end_col"].is_u64());
        assert_eq!(span["file"].as_str().unwrap(), path.display().to_string());
    }
    let _ = std::fs::remove_file(&path);
}

// ---- (3) undefined identifier surfaces as MT2021 ----

#[test]
fn json_undefined_identifier_surfaces_mt2021() {
    let path = write_tempfile("undef", "fn demo() {\n    log(undefined_thing);\n}\n");
    let (code, out, err) = run_check_json(&path);
    assert_eq!(code, 1, "undefined identifier should exit 1. stderr: {err}");
    let doc: serde_json::Value =
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON: {e}; stdout: {out}"));
    assert_eq!(doc["ok"], false);
    let diags = doc["diagnostics"].as_array().expect("diagnostics array");
    let mt2021 = diags.iter().find(|d| d["code"] == "MT2021");
    assert!(
        mt2021.is_some(),
        "expected MT2021 in diagnostics; got: {out}"
    );
    let dg = mt2021.unwrap();
    assert_eq!(dg["severity"], "error");
    assert!(
        dg["message"]
            .as_str()
            .unwrap_or("")
            .contains("undefined_thing"),
        "expected the offending name in the message, got: {dg}"
    );
    let _ = std::fs::remove_file(&path);
}

// ---- (4) parse glitch surfaces ----

#[test]
fn json_parse_error_surfaces() {
    // `let = 42;` — no binding pattern. T6 widened `mty check` to
    // surface this as MT0001; the --json path inherits the same
    // diagnostic stream.
    let path = write_tempfile("parse", "fn demo() {\n    let = 42;\n}\n");
    let (code, out, err) = run_check_json(&path);
    assert_eq!(code, 1, "parse error should exit 1. stderr: {err}");
    let doc: serde_json::Value =
        serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("not JSON: {e}; stdout: {out}"));
    assert_eq!(doc["ok"], false);
    let diags = doc["diagnostics"].as_array().expect("diagnostics array");
    let parse_diag = diags
        .iter()
        .find(|d| d["code"].as_str().unwrap_or("").starts_with("MT00"));
    assert!(
        parse_diag.is_some(),
        "expected a parse-phase MT00xx diagnostic; got: {out}"
    );
    let _ = std::fs::remove_file(&path);
}

// ---- (5) back-compat: pretty path unchanged when `--json` NOT set ----

#[test]
fn pretty_path_unchanged_without_json_flag() {
    // Clean file → `ok:` on stdout, exit 0. Matches T6's
    // `clean_file_still_reports_ok`.
    let path = write_tempfile("compat_ok", "fn demo() {\n    log(\"hi\");\n}\n");
    let (code, out, err) = run_check_pretty(&path);
    assert_eq!(code, 0, "clean file pretty: stderr: {err}");
    assert!(
        out.contains("ok:"),
        "expected `ok:` on stdout under pretty (no --json), got: {out}"
    );
    // The pretty path doesn't print the structured-result doc.
    assert!(!out.trim_start().starts_with('{'));
    let _ = std::fs::remove_file(&path);

    // Errored file → exit 1, ariadne text on stderr (MT2001 referenced).
    let path = write_tempfile(
        "compat_err",
        "fn demo() {\n    let x: I32 = \"hello\";\n}\n",
    );
    let (code, _out, err) = run_check_pretty(&path);
    assert_eq!(code, 1, "type error pretty: stderr: {err}");
    assert!(
        err.contains("MT2"),
        "expected MT2xxx in stderr under pretty path, got: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

// ---- (6) span carries both start and end ----

#[test]
fn json_span_carries_start_and_end() {
    let path = write_tempfile("spans", "fn demo() {\n    let x: I32 = \"hello\";\n}\n");
    let (_code, out, _err) = run_check_json(&path);
    let doc: serde_json::Value = serde_json::from_str(out.trim()).expect("JSON");
    let diags = doc["diagnostics"].as_array().expect("diags");
    let first = diags
        .iter()
        .find(|d| d["severity"] == "error")
        .expect("at least one error");
    let span = &first["span"];
    let line = span["line"].as_u64().unwrap();
    let col = span["col"].as_u64().unwrap();
    let end_line = span["end_line"].as_u64().unwrap();
    let end_col = span["end_col"].as_u64().unwrap();
    // end >= start in both dimensions.
    assert!(end_line >= line);
    if end_line == line {
        assert!(
            end_col > col,
            "end_col must be > col when on the same line; got {end_col} <= {col}"
        );
    }
    let _ = std::fs::remove_file(&path);
}
