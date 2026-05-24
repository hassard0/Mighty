//! Negative-test corpus for slice-5 diagnostics (SD4xxx).

use mty_diagnostics::{codes::*, Diagnostic, Severity};
use mty_driver::{lower, parse_source, type_check};

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

fn run_negative(name: &str) -> Vec<Diagnostic> {
    let path = workspace_root().join("tests/slice5_neg").join(name);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("failed to read {}: {}", path.display(), e);
    });
    let parsed = parse_source(src, path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(type_check(&pkg));
    diags
}

fn assert_emits(name: &str, expected: &[mty_diagnostics::codes::DiagCode]) {
    let diags = run_negative(name);
    let codes: Vec<mty_diagnostics::codes::DiagCode> = diags
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

#[test]
fn neg_effect_undeclared() {
    assert_emits("effect_undeclared.mty", &[EFFECT_UNDECLARED]);
}

#[test]
fn neg_protocol_missing() {
    assert_emits("protocol_missing.mty", &[PROTOCOL_MISSING_HANDLER]);
}

#[test]
fn neg_protocol_arity() {
    assert_emits("protocol_arity.mty", &[PROTOCOL_ARITY_MISMATCH]);
}

#[test]
fn neg_protocol_extra() {
    assert_emits("protocol_extra.mty", &[PROTOCOL_EXTRA_HANDLER]);
}

#[test]
fn neg_derive_copy_bad() {
    assert_emits("derive_copy_bad.mty", &[DERIVE_COPY_FIELD_NOT_COPY]);
}

#[test]
fn neg_derive_unknown() {
    assert_emits("derive_unknown.mty", &[DERIVE_UNKNOWN]);
}

#[test]
fn neg_trait_coherence() {
    assert_emits("trait_coherence.mty", &[TRAIT_COHERENCE_VIOLATION]);
}

#[test]
fn neg_dyn_unsafe() {
    assert_emits("dyn_unsafe.mty", &[DYN_REQUIRES_OBJECT_SAFE]);
}
