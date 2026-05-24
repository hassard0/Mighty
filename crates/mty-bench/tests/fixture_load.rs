//! The synthetic 10 KLOC fixture must parse cleanly — otherwise the
//! parse_throughput benchmark would measure error-recovery instead of
//! the happy path.

use mty_bench::fixtures::{echo_sir_program, stardust_10kloc};
use mty_diagnostics::Severity;

#[test]
fn ten_kloc_parses_without_errors() {
    let src = stardust_10kloc();
    let parsed = mty_driver::parse_source(src, "synth.mty".into());
    let errors: Vec<_> = parsed
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "10 KLOC fixture had parse errors: {errors:#?}"
    );
}

#[test]
fn echo_program_lowers_to_sir() {
    let prog = echo_sir_program();
    // Has at least one agent + the Ping handler fn.
    assert!(!prog.agents.is_empty(), "echo program should have an agent");
}
