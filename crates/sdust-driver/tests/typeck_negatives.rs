//! Negative-test corpus: each `.sd` file under `tests/typeck_neg/`
//! should emit at least one of the expected diagnostic codes.

use sdust_diagnostics::{codes::DiagCode, Severity};
use sdust_driver::{lower, parse_source, type_check};

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn run_negative(name: &str) -> Vec<sdust_diagnostics::Diagnostic> {
    let path = workspace_root().join("tests/typeck_neg").join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read {}: {}", path.display(), e);
    });
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(type_check(&pkg));
    diags
}

fn assert_emits(name: &str, expected: &[DiagCode]) {
    let diags = run_negative(name);
    let codes: Vec<DiagCode> = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code)
        .collect();
    assert!(
        expected.iter().any(|e| codes.contains(e)),
        "test {}: expected one of {:?} in {:?}",
        name,
        expected.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
        codes.iter().map(|c| c.as_str()).collect::<Vec<_>>()
    );
}

use sdust_diagnostics::codes::*;

#[test]
fn neg_mismatch_let() {
    assert_emits("mismatch_let.sd", &[TYPE_MISMATCH]);
}

#[test]
fn neg_mismatch_call() {
    assert_emits("mismatch_call.sd", &[TYPE_MISMATCH]);
}

#[test]
fn neg_unresolved_type() {
    assert_emits("unresolved_type.sd", &[UNRESOLVED_TYPE]);
}

#[test]
fn neg_wrong_arity() {
    // Some(1, 2) — fn expects 1 arg, got 2 → WRONG_ARG_COUNT.
    assert_emits("wrong_arity.sd", &[WRONG_ARG_COUNT, WRONG_VARIANT_ARITY]);
}

#[test]
fn neg_unknown_field() {
    assert_emits("unknown_field.sd", &[UNKNOWN_FIELD]);
}

#[test]
fn neg_pub_no_type() {
    assert_emits("pub_no_type.sd", &[PUB_PARAM_NEEDS_TYPE]);
}

#[test]
fn neg_q_outside_result() {
    assert_emits("q_outside_result.sd", &[QUESTION_OUTSIDE_RESULT]);
}

#[test]
fn neg_q_err_mismatch() {
    assert_emits(
        "q_err_mismatch.sd",
        &[QUESTION_ERROR_MISMATCH, TYPE_MISMATCH],
    );
}

#[test]
fn neg_binop_mismatch() {
    assert_emits("binop_mismatch.sd", &[BINOP_TYPE_MISMATCH, TYPE_MISMATCH]);
}

#[test]
fn neg_wrong_generic_arity() {
    assert_emits("wrong_generic_arity.sd", &[WRONG_GENERIC_ARITY]);
}

#[test]
fn neg_return_mismatch() {
    assert_emits("return_mismatch.sd", &[RETURN_TYPE_MISMATCH, TYPE_MISMATCH]);
}

#[test]
fn neg_if_branch_mismatch() {
    assert_emits(
        "if_branch_mismatch.sd",
        &[IF_BRANCH_MISMATCH, TYPE_MISMATCH],
    );
}
