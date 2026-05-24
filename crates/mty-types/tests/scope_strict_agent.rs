//! v0.3 (A65): strict agent body rejects unresolved value names with
//! SD2021 (was slice-3 A21 permissive fresh-var fallback).

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "scope_strict_agent.sd".into());
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
fn agent_handler_unknown_name_is_sd2021() {
    // `nonexistent_helper` is not in the agent's tolerance set, not in
    // the prelude, and not a local — strict handler scope must error.
    let src = "
        protocol Hi { Greet(name: Str) -> Str }
        agent Greeter: Hi {
          on Greet(name) -> {
            nonexistent_helper(name)
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD2021".to_string()),
        "expected SD2021 in strict handler scope, got {:?}",
        codes
    );
}

#[test]
fn agent_state_init_unknown_name_is_sd2021() {
    // State initializer runs in AgentBody scope (strict). `random_seed`
    // isn't bound anywhere.
    let src = "
        protocol Tick { Pulse() -> I32 }
        agent Heart: Tick {
          n = random_seed()
          on Pulse() -> n
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD2021".to_string()),
        "expected SD2021 in strict agent state-init scope, got {:?}",
        codes
    );
}

#[test]
fn agent_method_unknown_name_is_sd2021() {
    // Agent methods (vs handlers) also run in strict AgentBody scope per
    // v0.3 A65. The body of `compute` references an unbound name.
    let src = "
        protocol Tick { Pulse() -> I32 }
        agent Heart: Tick {
          n = 0
          on Pulse() -> n
          fn compute() -> I32 {
            unknown_helper() + 1
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"SD2021".to_string()),
        "expected SD2021 in strict agent-method scope, got {:?}",
        codes
    );
}

#[test]
fn agent_handler_known_state_name_is_clean() {
    // `n` is in the agent's tolerance set (state), so the strict scope
    // accepts it.
    let src = "
        protocol Tick { Pulse() -> I32 }
        agent Heart: Tick {
          n = 0
          on Pulse() -> n
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"SD2021".to_string()),
        "state name should be in tolerance, got {:?}",
        codes
    );
}
