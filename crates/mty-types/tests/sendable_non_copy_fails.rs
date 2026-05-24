//! v0.3 (A65): cross-agent send/ask with a non-Sendable arg (e.g. an
//! immutable reference, a mutable reference, or a capability handle)
//! must fire SD3011 with a reason note.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "sendable_non_copy_fails.sd".into());
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
fn send_immutable_ref_is_not_sendable() {
    let src = "
        protocol Counter { Add(n: &I32) -> I32 }
        agent Adder: Counter {
          on Add(n) -> 0
        }
        fn driver(a: Adder, x: I32) -> Unit {
          a!Add(&x)
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD3011".to_string()),
        "&I32 must NOT be Sendable across agent boundaries, got {:?}",
        codes
    );
}

#[test]
fn send_mut_ref_is_not_sendable() {
    let src = "
        protocol Counter { Add(n: &mut I32) -> I32 }
        agent Adder: Counter {
          on Add(n) -> 0
        }
        fn driver(a: Adder) -> Unit {
          let mut x = 1
          a!Add(&mut x)
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD3011".to_string()),
        "&mut I32 must NOT be Sendable across agent boundaries, got {:?}",
        codes
    );
}
