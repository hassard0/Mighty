//! v0.3 (A65): Sendable trait at `!Msg(...)` / `?Msg(...)` call sites
//! accepts Copy types and owned heap values.

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source};
use sdust_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "sendable_copy_passes.sd".into());
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
fn send_str_literal_is_sendable() {
    let src = r#"
        protocol Hi { Greet(name: Str) -> Str }
        agent Greeter: Hi {
          on Greet(name) -> name
        }
        fn driver(g: Greeter) -> Unit {
          g!Greet("alice")
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD3011".to_string()),
        "Str literal is Sendable (owned + no internal refs), got {:?}",
        codes
    );
}

#[test]
fn send_int_literal_is_sendable() {
    let src = "
        protocol Counter { Add(n: I32) -> I32 }
        agent Adder: Counter {
          on Add(n) -> n
        }
        fn driver(a: Adder) -> Unit {
          a!Add(1)
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD3011".to_string()),
        "I32 literal is Copy / Sendable, got {:?}",
        codes
    );
}

#[test]
fn send_string_value_is_sendable() {
    // A String (heap, owned, no internal refs) crosses the boundary OK.
    let src = r#"
        protocol Hi { Greet(name: Str) -> Str }
        agent Greeter: Hi {
          on Greet(name) -> name
        }
        fn driver(g: Greeter, name: Str) -> Unit {
          g!Greet(name)
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD3011".to_string()),
        "Str param is Sendable, got {:?}",
        codes
    );
}
