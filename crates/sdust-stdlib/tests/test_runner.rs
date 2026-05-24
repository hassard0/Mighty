//! Verify the `std.test` discovery + run flow against a fixture
//! containing two passing tests and one failing test.

#![cfg(feature = "runner")]

use sdust_stdlib::test::{run_dir, TestOutcome};
use std::fs;

fn write_fixture(dir: &std::path::Path) {
    fs::write(
        dir.join("a_test.sd"),
        "\
fn test_pass_one() {
}
fn test_pass_two() {
}
",
    )
    .unwrap();
    fs::write(
        dir.join("b_test.sd"),
        "\
fn test_panics() {
  panic(\"boom\")
}
",
    )
    .unwrap();
}

#[test]
fn discovers_and_runs_fixture() {
    let tmp = tempfile::tempdir().unwrap();
    write_fixture(tmp.path());

    let summary = run_dir(tmp.path());
    // 3 tests total.
    assert_eq!(summary.reports.len(), 3, "summary: {}", summary.output);
    // 2 passed, 1 failed.
    let passed = summary
        .reports
        .iter()
        .filter(|r| matches!(r.outcome, TestOutcome::Pass))
        .count();
    let failed = summary.reports.len() - passed;
    assert_eq!(passed, 2, "output:\n{}", summary.output);
    assert_eq!(failed, 1, "output:\n{}", summary.output);
    assert_eq!(summary.exit_code(), 1);
    // Reporter mentions both pass and FAILED.
    assert!(summary.output.contains("ok"), "{}", summary.output);
    assert!(summary.output.contains("FAILED"), "{}", summary.output);
    assert!(
        summary.output.contains("2 passed; 1 failed"),
        "{}",
        summary.output
    );
}

#[test]
fn empty_dir_passes_with_zero_tests() {
    let tmp = tempfile::tempdir().unwrap();
    let summary = run_dir(tmp.path());
    assert_eq!(summary.reports.len(), 0);
    assert_eq!(summary.exit_code(), 0);
}

#[test]
fn missing_dir_passes_with_zero_tests() {
    let nonexistent = std::path::Path::new("/nonexistent/path/for/stdlib/test");
    let summary = run_dir(nonexistent);
    assert_eq!(summary.reports.len(), 0);
    assert_eq!(summary.exit_code(), 0);
}
