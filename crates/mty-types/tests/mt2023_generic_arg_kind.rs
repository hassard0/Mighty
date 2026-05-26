//! v0.14 (Gap B / MT2023 emit-site): a generic type argument resolves
//! to a value-kind def (function name, variant constructor) instead of
//! a type. Pre-v0.14 this funnelled through MT2002 ("unresolved type");
//! v0.14 emits MT2023 so the user sees the kind-mismatch explanation.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "mt2023.mty".into());
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
fn function_name_in_generic_arg_emits_mt2023() {
    let src = "
        fn helper() -> I32 { 0 }
        fn bad() -> Result[helper, I32] { Ok(1) }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2023".to_string()),
        "expected MT2023 for fn-name in type-arg position, got {:?}",
        codes
    );
}

#[test]
fn type_in_generic_arg_does_not_emit_mt2023() {
    let src = "fn ok() -> Result[I32, I32] { Ok(1) }";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2023".to_string()),
        "MT2023 must not fire when the arg is a proper type, got {:?}",
        codes
    );
}
