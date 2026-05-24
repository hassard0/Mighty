//! v0.3 (A65): when a handler implements a **local** protocol, the
//! handler body's inferred param type must unify with the protocol's
//! declared param type. Mismatch fires SD4031.
//!
//! For **external** protocols (e.g. `http.Handler` in another module),
//! the check is skipped and SD2026 is emitted as before.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "protocol_param_strict.sd".into());
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

fn diag_codes_all(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "protocol_param_strict.sd".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn local_protocol_param_type_mismatch_fires_sd4031() {
    // Counter declares Add(n: Str); handler infers n as I32 via the
    // `let v: I32 = n` annotation. v0.3 unifies declared vs inferred
    // and raises SD4031 on mismatch.
    let src = "
        protocol Counter { Add(n: Str) -> I32 }
        agent Adder: Counter {
          total = 0
          on Add(n) -> {
            let v: I32 = n
            total + v
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD4031".to_string()),
        "expected SD4031 for local-protocol param-type mismatch, got {:?}",
        codes
    );
}

#[test]
fn local_protocol_param_type_matching_is_clean() {
    // Counter declares Add(n: I32); handler uses n as I32 — clean.
    let src = "
        protocol Counter { Add(n: I32) -> I32 }
        agent Adder: Counter {
          total = 0
          on Add(n) -> {
            total + n
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD4031".to_string()),
        "matching param types should not raise SD4031, got {:?}",
        codes
    );
}

#[test]
fn external_protocol_keeps_sd2026_warning_path() {
    // `http.Handler` is an external protocol (no local declaration).
    // The handler walks via the Unknown lookup → SD2026 warning, NOT
    // SD4031. This preserves v0.2 example 19's behavior.
    let src = r#"
        use std.http
        agent Api(searcher): http.Handler {
          on Request(req) -> http.ok("body")
        }
    "#;
    let codes_all = diag_codes_all(src);
    // SD2026 warning should be present; SD4031 must NOT be present.
    assert!(
        codes_all.contains(&"SD2026".to_string()),
        "external protocol should warn SD2026, got {:?}",
        codes_all
    );
    let errors_only = diag_codes(src);
    assert!(
        !errors_only.contains(&"SD4031".to_string()),
        "external protocol must NOT fire SD4031, got {:?}",
        errors_only
    );
}
