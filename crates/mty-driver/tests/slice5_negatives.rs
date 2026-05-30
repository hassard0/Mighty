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

// ----------------------------------------------------------------------
// v0.41 T6 (L14) — improved MT4001 diagnostic.
//
// The pre-T6 message read:
//     "public function `f` is missing declared effect(s): alloc"
// with one terse note "add `effect alloc` to the function signature".
// Authors hitting this for the first time (every `pub fn` that returns
// a `Vec` / `String`) didn't know *why* — was it a typecheck bug, an
// effect-bug? The IDE dogfooding lesson (L14) called for an effect-
// specific hint + a docs link.
//
// This test asserts both the legacy primary-label text (back-compat for
// MT4001 tests that grep on it) AND the new hint + docs link notes.

#[test]
fn neg_effect_undeclared_carries_alloc_hint_and_docs_link() {
    // The driver-side end-to-end source uses an `arena` body to make
    // alloc-inference fire (the same shape the effect_row_e2e tests
    // and the inline effect-inference unit tests use). The L14
    // ergonomics is independent of how the missing effect is inferred:
    // any MT4001 carrying `alloc` should get the hint + docs link.
    let src = r#"
        pub fn make_buf() -> I32 {
          arena tmp { 0 }
        }
    "#;
    let parsed = parse_source(src.into(), "test.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    diags.extend(type_check(&pkg));
    let d = diags
        .iter()
        .find(|d| matches!(d.severity, Severity::Error) && d.code == EFFECT_UNDECLARED)
        .expect("expected MT4001 effect_undeclared");
    // Primary label text (unchanged for back-compat).
    assert!(
        d.primary
            .message
            .contains("is missing declared effect(s): alloc"),
        "primary message changed: {}",
        d.primary.message
    );
    // T6 adds a `hint:` line explaining alloc.
    let notes_joined = d.notes.join("\n");
    assert!(
        notes_joined.contains("hint: `alloc` is required"),
        "expected an `alloc`-specific hint, got notes: {:?}",
        d.notes
    );
    // T6 adds a docs link.
    assert!(
        notes_joined.contains("docs/internals/effects.md"),
        "expected a docs/internals/effects.md link, got notes: {:?}",
        d.notes
    );
}
