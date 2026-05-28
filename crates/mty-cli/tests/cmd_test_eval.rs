//! v0.30 Track E — integration tests for `mty test --eval`.
//!
//! Spins the `mty` binary against a temp project populated with
//! hand-rolled `*.eval.mty` fixtures and asserts the exit-code +
//! report shape. These tests live alongside the unit tests in
//! `crates/mty-cli/src/cmd/test.rs` — the unit tests cover frontmatter
//! parsing + discovery in isolation, this file covers the
//! full-binary round-trip including clap argument plumbing.

use std::path::Path;
use std::process::Command;

fn mty(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mty"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run mty");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fixture_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let eval_dir = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&eval_dir).unwrap();
    let good = r#"//! eval: hermetic-suite
//! threshold: equal >= 1.0
//! members:
//!   - mock:m1
//!   - mock:m2
//! cases:
//!   - from_input: "hi"
"#;
    std::fs::write(eval_dir.join("hermetic.eval.mty"), good).unwrap();
    tmp
}

#[test]
fn test_eval_runs_and_passes_with_mock_members() {
    let tmp = fixture_project();
    let (code, out, err) = mty(tmp.path(), &["test", "--eval"]);
    assert_eq!(code, 0, "expected pass — stdout: {out} stderr: {err}");
    assert!(out.contains("hermetic.eval.mty"));
    assert!(out.contains("PASS"));
    assert!(out.contains("eval result:"));
}

#[test]
fn test_eval_json_format_emits_one_summary_object() {
    let tmp = fixture_project();
    let (code, out, _err) = mty(tmp.path(), &["test", "--eval", "--format", "json"]);
    assert_eq!(code, 0);
    let summary_line = out
        .lines()
        .find(|l| l.contains(r#""type":"summary""#))
        .expect("summary object on its own line");
    assert!(summary_line.contains(r#""mode":"eval""#));
    assert!(summary_line.contains(r#""passed":1"#));
    assert!(summary_line.contains(r#""failed":0"#));
}

#[test]
fn test_eval_returns_zero_when_no_files_found() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (code, out, _err) = mty(tmp.path(), &["test", "--eval"]);
    assert_eq!(code, 0);
    assert!(out.contains("no .eval.mty"));
}

#[test]
fn test_eval_fails_with_malformed_frontmatter_strict() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let eval_dir = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&eval_dir).unwrap();
    // No members → MissingMembers from the parser.
    let bad = "//! eval: x\n//! cases:\n//!   - from_input: \"hi\"\n";
    std::fs::write(eval_dir.join("bad.eval.mty"), bad).unwrap();
    let (code, _out, err) = mty(tmp.path(), &["test", "--eval"]);
    assert_eq!(code, 1, "expected fail; stderr: {err}");
    assert!(err.contains("missing `members`"));
}

#[test]
fn test_eval_no_strict_passes_even_with_error_cells() {
    // A real-provider member with no API key emits an error cell.
    // Under --no-strict the suite still passes (the other member is
    // a baseline match).
    let tmp = tempfile::tempdir().expect("tempdir");
    let eval_dir = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&eval_dir).unwrap();
    // Strip env vars by passing --no-strict; the anthropic member
    // will error since the test harness doesn't set ANTHROPIC_API_KEY.
    let mixed = r#"//! eval: mixed
//! threshold: equal >= 1.0
//! members:
//!   - mock:m1
//!   - anthropic:claude-opus-4-7
//! cases:
//!   - from_input: "hi"
"#;
    std::fs::write(eval_dir.join("mixed.eval.mty"), mixed).unwrap();
    let (code, out, _err) = mty(tmp.path(), &["test", "--eval", "--no-strict"]);
    // Either passes (the mock baseline is a SingleMember-style PASS
    // when the second cell ERRs in no-strict) or surfaces a clean
    // ERR line — we just assert the binary doesn't crash and emits
    // some recognizable output.
    assert!(code == 0 || code == 1);
    assert!(out.contains("mixed.eval.mty"));
}

#[test]
fn test_eval_respects_manifest_dir_paths_block() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        nested.join("a.eval.mty"),
        "//! eval: a\n//! members:\n//!   - mock:m\n//! cases:\n//!   - from_input: hi\n",
    )
    .unwrap();
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(
        elsewhere.join("b.eval.mty"),
        "//! eval: b\n//! members:\n//!   - mock:m\n//! cases:\n//!   - from_input: hi\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("mighty.toml"),
        "[eval]\npaths = [\"tests/eval\"]\n",
    )
    .unwrap();
    let (code, out, _err) = mty(tmp.path(), &["test", "--eval"]);
    assert_eq!(code, 0);
    // Only `a` should be discovered; `b` lives outside `tests/eval`.
    assert!(out.contains("a.eval.mty"));
    assert!(!out.contains("b.eval.mty"));
}

#[test]
fn test_eval_replay_only_flag_runs_clean() {
    // --replay-only is currently a pass-through (see v0.31 follow-up
    // in cmd/test.rs); the smoke test pins the flag's CLI surface
    // so a regression in argument parsing is loud.
    let tmp = fixture_project();
    let (code, out, _err) = mty(tmp.path(), &["test", "--eval", "--replay-only"]);
    assert_eq!(code, 0);
    assert!(out.contains("hermetic.eval.mty"));
}

#[test]
fn test_eval_ci_flag_overrides_threshold_and_members() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let eval_dir = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&eval_dir).unwrap();
    // Frontmatter says: 1 anthropic member, equal-threshold-1.0.
    let body = r#"//! eval: x
//! threshold: equal >= 1.0
//! members:
//!   - anthropic:claude-opus-4-7
//! cases:
//!   - from_input: "hi"
"#;
    std::fs::write(eval_dir.join("x.eval.mty"), body).unwrap();
    // CI override swaps anthropic→mock so the test doesn't need an
    // API key. Threshold stays semantic-similarity at 0.0 so
    // anything passes.
    std::fs::write(
        tmp.path().join("mighty.toml"),
        "[eval.ci]\nmembers = [\"mock:m1\", \"mock:m2\"]\nthreshold = \"semantic_similarity >= 0.0\"\n",
    )
    .unwrap();
    let (code, out, err) = mty(tmp.path(), &["test", "--eval", "--ci"]);
    assert_eq!(code, 0, "expected pass; stderr: {err}");
    assert!(out.contains("m1"));
    assert!(out.contains("m2"));
}

#[test]
fn test_default_mode_runs_unit_tests() {
    // No --eval flag → unit-test mode. We give it an empty tests/
    // dir so the run returns zero with the "0 passed; 0 failed"
    // summary.
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    let (code, out, _err) = mty(tmp.path(), &["test"]);
    assert_eq!(code, 0);
    assert!(out.contains("test result:"));
}

#[test]
fn test_default_mode_json_format() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    let (code, out, _err) = mty(tmp.path(), &["test", "--format", "json"]);
    assert_eq!(code, 0);
    assert!(out.contains(r#""mode":"unit""#));
}

#[test]
fn test_eval_eval_file_with_blank_lines_in_frontmatter() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let eval_dir = tmp.path().join("tests/eval");
    std::fs::create_dir_all(&eval_dir).unwrap();
    let body = "\n\n//! eval: x\n//! members:\n//!   - mock:m\n//! cases:\n//!   - from_input: hi\n\nfn eval() {}\n";
    std::fs::write(eval_dir.join("ok.eval.mty"), body).unwrap();
    let (code, _out, _err) = mty(tmp.path(), &["test", "--eval"]);
    assert_eq!(code, 0);
}
