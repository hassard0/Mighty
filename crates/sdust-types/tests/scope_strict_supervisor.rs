//! v0.3 (A65): supervisor body scope is marked SupervisorBody. Today
//! supervisor child expressions still run with tolerance_open=true (until
//! slice-7 wires per-supervisor cap names), so SD2021 doesn't fire yet
//! — these tests are negative assertions documenting the current
//! behavior and the planned tightening.

use sdust_diagnostics::Severity;
use sdust_driver::{lower, parse_source};
use sdust_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "scope_strict_supervisor.sd".into());
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
fn supervisor_body_is_currently_tolerance_open() {
    // The supervisor body references `Echoer` which is declared as a
    // sibling agent in the same file; that's fine. It also uses bare
    // capability names that come from the supervisor's enclosing scope
    // — tolerance_open keeps these silent.
    let src = "
        protocol Echo { Ping(msg: Str) -> Str }
        agent Echoer: Echo {
          on Ping(msg) -> msg
        }
        supervisor Top one_for_one {
          worker = spawn Echoer()
        }
    ";
    let codes = diag_codes(src);
    // No SD2021 expected: tolerance_open is on.
    assert!(
        !codes.contains(&"SD2021".to_string()),
        "supervisor child expr should not trigger SD2021 yet, got {:?}",
        codes
    );
}
