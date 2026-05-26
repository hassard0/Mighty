//! v0.14 (Gap B / MT2003 emit-site): a let-binding with no annotation
//! whose initializer leaves the element type as a free inference
//! variable. Empty array literal is the v0.14 detection shape.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "mt2003.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn empty_array_no_annotation_emits_mt2003() {
    let src = "fn main() -> I32 { let xs = []; 42 }";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2003".to_string()),
        "expected MT2003 for empty-array let without annotation, got {:?}",
        codes
    );
}

#[test]
fn empty_array_with_annotation_does_not_emit_mt2003() {
    // `let xs: [I32; 0] = []` constrains element to I32 — no MT2003.
    let src = "fn main() -> I32 { let xs: [I32; 0] = []; 42 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2003".to_string()),
        "MT2003 should not fire when an annotation is provided, got {:?}",
        codes
    );
}

#[test]
fn non_empty_array_does_not_emit_mt2003() {
    // `let xs = [1]` infers element from the first item — no MT2003.
    let src = "fn main() -> I32 { let xs = [1]; 42 }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2003".to_string()),
        "MT2003 should not fire when at least one element constrains the type, got {:?}",
        codes
    );
}
