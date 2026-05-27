//! v0.27 Track B — strict handler scope accepts std.* opaque ADT
//! constructors. The v0.3 (A65) rule still applies to USER-defined
//! opaque ADTs (back-compat).
//!
//! v0.26 demo 07 (Track E) had to construct `VectorStore.local(...)`,
//! `Episodic.in_memory(...)`, `Working.new()`, `AnthropicClient.from_env()`
//! in `main()` (TopLevelFn — permissive) and pass them as ctor args to
//! the Researcher agent. Constructing them INSIDE an `on Ask()` handler
//! tripped MT2021 because the names weren't in the prelude.
//!
//! v0.27 Track B registers the four ctor types (plus their siblings) as
//! prelude opaque ADTs and marks each `AdtId` in
//! `DefMap::handler_safe_adts`. `check::synth_path` consults that set
//! before firing MT2021 in strict scopes.
//!
//! These tests pin:
//!
//! 1. Each named std.* opaque ADT is constructible inside a handler
//!    body without firing MT2021.
//! 2. User-defined opaque-ish names without a prelude registration
//!    continue to trip the original A65 rule (back-compat).
//! 3. The MT2021 path still fires for non-`std.*` unresolved names
//!    side-by-side with handler-safe constructions.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "opaque_adt_handler_scope.mty".into());
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
fn std_memory_working_constructible_in_handler() {
    // `Working.new()` is now a recognised prelude opaque ADT name, so
    // the strict handler scope accepts it (no MT2021).
    let src = "
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let working = Working.new()
            question
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "Working.new() should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_memory_vector_constructible_in_handler() {
    // VectorStore.local(...) — the v0.26 demo 07 ctor that was the
    // forcing function for this whole carve-out.
    let src = r#"
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let vector = VectorStore.local("./vector.json")
            question
          }
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "VectorStore.local(...) should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_memory_episodic_constructible_in_handler() {
    let src = "
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let episodic = Episodic.in_memory(100)
            question
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "Episodic.in_memory(100) should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_llm_anthropic_constructible_in_handler() {
    // AnthropicClient.from_env() — Track A v0.26's typed provider.
    let src = "
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let client = AnthropicClient.from_env()
            question
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "AnthropicClient.from_env() should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn all_four_demo07_ctors_constructible_in_one_handler() {
    // The actual v0.26 demo 07 workaround pattern, lifted INTO the
    // handler. This is the headline shape — all four ctors that used
    // to live in `main()` and be passed as ctor args now construct
    // cleanly inside `on Ask()`.
    let src = r#"
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let client = AnthropicClient.from_env()
            let vector = VectorStore.local("./vector.json")
            let episodic = Episodic.in_memory(100)
            let working = Working.new()
            question
          }
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "all four std.* ctors should be handler-safe in one body, got {:?}",
        codes
    );
}

// ---------------------------------------------------------------------
// v0.29 Track B — std.swarm ADTs added to the handler-safe carve-out.
//
// Demo 08 (v0.27 Track F) lifted `Member.anthropic(...)`,
// `DollarBudget.from_dollars(...)`, and the `ConsensusStrategy.Majority`
// reference out of the handler body (into `main()`) because the four
// swarm ADTs weren't on the v0.27 allowlist. v0.29 Track B adds them.
// Each test below pins that the ctor lands cleanly in a handler scope
// without firing MT2021.
// ---------------------------------------------------------------------

#[test]
fn std_swarm_member_anthropic_constructible_in_handler() {
    // `Member.anthropic("...")` — the demo 08 ctor that headlined the
    // v0.27 Track F note's "v0.28 follow-up §A".
    let src = r#"
        protocol Reviewer { Review(snippet: Str) -> Str }
        agent R: Reviewer {
          on Review(snippet) -> {
            let m = Member.anthropic("claude-opus-4-7")
            snippet
          }
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "Member.anthropic(...) should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_swarm_dollar_budget_constructible_in_handler() {
    // `DollarBudget.from_dollars(0.50)` — the demo 08 budget ctor.
    let src = "
        protocol Reviewer { Review(snippet: Str) -> Str }
        agent R: Reviewer {
          on Review(snippet) -> {
            let cap = DollarBudget.from_dollars(0.50)
            snippet
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "DollarBudget.from_dollars(...) should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_swarm_consensus_strategy_majority_in_handler() {
    // `ConsensusStrategy.Majority` — the strategy variant reference
    // demo 08 had to lift out of the handler.
    let src = "
        protocol Reviewer { Review(snippet: Str) -> Str }
        agent R: Reviewer {
          on Review(snippet) -> {
            let strategy = ConsensusStrategy.Majority
            snippet
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "ConsensusStrategy.Majority should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn std_swarm_consensus_type_usable_in_handler() {
    // `Consensus` referenced as a value-position name (e.g. via a
    // synthetic helper). The opaque-ADT carve-out applies symmetrically
    // to the `Consensus` result shape, not just the inputs.
    let src = "
        protocol Reviewer { Review(snippet: Str) -> Str }
        agent R: Reviewer {
          on Review(snippet) -> {
            let _typename = Consensus
            snippet
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "Consensus should be handler-safe, got {:?}",
        codes
    );
}

#[test]
fn all_four_swarm_ctors_constructible_in_one_handler() {
    // The actual demo 08 panel-build pattern, lifted INTO the handler.
    // Every swarm ctor that the demo had to keep in `main()` now
    // constructs cleanly inside `on Review()`.
    let src = r#"
        protocol Reviewer { Review(snippet: Str) -> Str }
        agent R: Reviewer {
          on Review(snippet) -> {
            let panel = Vec.new()
            panel.push(Member.anthropic("claude-opus-4-7"))
            panel.push(Member.openai("gpt-5"))
            panel.push(Member.gemini("gemini-2.5-pro"))
            let cap = DollarBudget.from_dollars(0.50)
            let strategy = ConsensusStrategy.Majority
            snippet
          }
        }
    "#;
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2021".to_string()),
        "all four std.swarm ctors should be handler-safe in one body, got {:?}",
        codes
    );
}

#[test]
fn user_defined_adt_without_effects_still_blocked_in_handler() {
    // Back-compat: a name that is NOT a registered std.* opaque ADT
    // (`MyCustomThing`) still trips MT2021 in strict handler scope.
    // This pins the carve-out as scoped strictly to the prelude
    // registrations — user-side opaque-ish names continue to need
    // ctor-in-main + ctor-arg threading.
    let src = "
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let thing = MyCustomThing.new()
            question
          }
        }
    ";
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2021".to_string()),
        "user-defined opaque name should still trip MT2021, got {:?}",
        codes
    );
}

#[test]
fn mixed_handler_safe_and_unknown_keeps_only_unknown_error() {
    // Mixed body: one handler-safe ctor + one user-side unknown name.
    // Only the user-side unknown should error, the handler-safe ctor
    // should be silently accepted. Pins that the carve-out doesn't
    // accidentally suppress unrelated MT2021 emissions.
    let src = "
        protocol Researcher { Ask(question: Str) -> Str }
        agent R: Researcher {
          on Ask(question) -> {
            let working = Working.new()
            let bad = unknown_helper_fn()
            question
          }
        }
    ";
    let codes = diag_codes(src);
    let mt2021_count = codes.iter().filter(|c| *c == "MT2021").count();
    assert!(
        mt2021_count >= 1,
        "expected at least one MT2021 for unknown_helper_fn, got {:?}",
        codes
    );
}
