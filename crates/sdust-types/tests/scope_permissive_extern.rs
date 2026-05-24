//! v0.3 (A65): extern blocks and top-level fns keep the slice-3 A21
//! permissive fresh-var fallback. Unknown names inside them are NOT
//! promoted to SD2021.

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source};
use sdust_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "scope_permissive_extern.sd".into());
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
fn top_level_fn_unknown_name_is_permissive() {
    // Slice-3 A21: top-level fn bodies tolerate unresolved names by
    // falling back to fresh inference variables. v0.3 keeps this.
    let src = "
        fn driver() -> Unit {
          some_helper()
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD2021".to_string()),
        "top-level fn should be permissive (A21), got {:?}",
        codes
    );
}

#[test]
fn unsafe_block_unknown_name_is_permissive() {
    // `unsafe { ... }` opens tolerance for raw-pointer / pointer-ABI
    // identifiers. v0.3 marks it as ScopeKind::Unsafe (permissive).
    let src = "
        fn poke() -> Unit {
          unsafe {
            raw_call_with_no_decl()
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD2021".to_string()),
        "unsafe block should be permissive, got {:?}",
        codes
    );
}
