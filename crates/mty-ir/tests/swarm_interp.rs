//! v0.29 Track A — `BuiltinId::Swarm` interpreter arm tests.
//!
//! Each test drives the SIR tree-walking interpreter through a tiny
//! Mighty `fn main()` that builds a panel of `Member.mock(...)` /
//! `Member.mock_error(...)` values, calls `swarm(prompt, panel,
//! budget, strategy)`, and returns an `I32` exit code derived from
//! the resulting `Consensus` (number of dissents, budget-trip flag,
//! verdict-present flag, etc.).
//!
//! The arm is implemented in `crates/mty-ir/src/interp/run.rs`
//! (the `swarm_dispatch` module + the `BuiltinId::Swarm` match arm).
//! Test names mirror the strategies + edge cases listed in the
//! v0.29 Track A mandate.
//!
//! Notes:
//! - Array literals in the test source intentionally omit the
//!   trailing comma before `]` — the v0.27-era HIR parser trips on
//!   the trailing comma + newline combo and emits a truncated IR.
//!   Closes a v0.29 follow-up to harden the array-literal parser.

mod common;

use common::*;
use mty_ir::interp::RunResult;

fn assert_ok_exit(src: &str, expected: i32) {
    let (res, _h) = run_main(src);
    match res {
        RunResult::Ok { exit } => assert_eq!(exit, expected, "exit; src: {src}"),
        other => panic!("expected Ok, got {other:?}; src: {src}"),
    }
}

/// Majority strategy: three members, two agree on "yes", one says
/// "no". Verdict is `Some("yes")` and dissent count is 1.
#[test]
fn swarm_majority_picks_largest_cluster() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("alpha", "yes", 5),
                Member.mock("beta", "yes", 5),
                Member.mock("gamma", "no", 5)
            ]
            let spend_cap = DollarBudget.new(100)
            let c = swarm("prompt", panel, spend_cap, ConsensusStrategy.Majority)
            let verdict = c.majority.unwrap_or("")
            let mut score: I32 = 0
            if verdict == "yes" { score = score + 100 }
            score = score + (c.dissents.len() as I32)
            score
        }
    "#;
    // 100 (verdict==yes) + 1 (one dissent: "gamma") = 101.
    assert_ok_exit(src, 101);
}

/// Unanimous strategy: every member agrees → verdict = `Some(body)`,
/// zero dissents.
#[test]
fn swarm_unanimous_short_circuits_on_full_agreement() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 1),
                Member.mock("b", "yes", 1),
                Member.mock("c", "yes", 1)
            ]
            let spend_cap = DollarBudget.new(100)
            let c = swarm("p", panel, spend_cap, ConsensusStrategy.Unanimous)
            let verdict = c.majority.unwrap_or("")
            if verdict == "yes" {
                (c.dissents.len() as I32)
            } else {
                999
            }
        }
    "#;
    // Unanimous "yes" → 0 dissents → exit 0.
    assert_ok_exit(src, 0);
}

/// Unanimous on a split panel surfaces `majority = None` and every
/// reply lands in the dissent set.
#[test]
fn swarm_unanimous_no_consensus_on_split() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 1),
                Member.mock("b", "no", 1)
            ]
            let c = swarm("p", panel, DollarBudget.new(100), ConsensusStrategy.Unanimous)
            let v = c.majority.unwrap_or("none")
            if v == "none" {
                (c.dissents.len() as I32)
            } else {
                999
            }
        }
    "#;
    // Split unanimous → majority = None, all 2 replies are dissents.
    assert_ok_exit(src, 2);
}

/// Weighted-vote tie-break: two members say "yes" with weight 1 each
/// (total 2), one member says "no" with weight 5. Weighted vote picks
/// "no" because the weight sum is higher.
#[test]
fn swarm_weighted_vote_tie_break() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 1),
                Member.mock("b", "yes", 1),
                Member.mock("c", "no", 1)
            ]
            let weights = [1, 1, 5]
            let strat = ConsensusStrategy.WeightedVote(weights)
            let c = swarm("p", panel, DollarBudget.new(100), strat)
            let v = c.majority.unwrap_or("")
            if v == "no" { 1 } else { 0 }
        }
    "#;
    assert_ok_exit(src, 1);
}

/// Dollar-budget exhaustion: a 10-cent budget against two 8-cent
/// members. The first runs (consumed=8), the second is skipped
/// (consumed >= limit after first run + 8 = 16 > 10). The consensus
/// surfaces with `budget_exhausted: true` and the one reply that
/// ran is the verdict.
#[test]
fn swarm_dollar_budget_exhaustion() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 8),
                Member.mock("b", "no", 8)
            ]
            let spend_cap = DollarBudget.new(10)
            let c = swarm("p", panel, spend_cap, ConsensusStrategy.Majority)
            let v = c.majority.unwrap_or("")
            let mut score: I32 = 0
            if v == "yes" { score = score + 10 }
            if c.budget_exhausted { score = score + 1 }
            score
        }
    "#;
    // 10 (verdict=="yes") + 1 (budget tripped) = 11.
    assert_ok_exit(src, 11);
}

/// Mid-run member abort: panel of three where the middle member is
/// `Member.mock_error(...)` (forced error). The two healthy members
/// agree on "yes"; the broken member drops out (does not count as a
/// reply). Consensus = "yes", dissents = 0, all_replies.len() = 2.
#[test]
fn swarm_mid_run_member_abort() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 1),
                Member.mock_error("broken", "kaboom"),
                Member.mock("c", "yes", 1)
            ]
            let c = swarm("p", panel, DollarBudget.new(100), ConsensusStrategy.Majority)
            let v = c.majority.unwrap_or("")
            let mut score: I32 = 0
            if v == "yes" { score = score + 100 }
            score = score + (c.dissents.len() as I32)
            score = score + (c.all_replies.len() as I32) * 10
            score
        }
    "#;
    // 100 + 0 + 2*10 = 120.
    assert_ok_exit(src, 120);
}

/// Empty-panel error: the swarm surfaces with `majority = None`,
/// zero dissents, `budget_exhausted = false`. This is the flattened
/// equivalent of `mty_stdlib::swarm::SwarmError::EmptyPanel`.
#[test]
fn swarm_empty_panel_returns_no_consensus() {
    let src = r#"
        fn main() -> I32 {
            let panel = []
            let c = swarm("p", panel, DollarBudget.new(100), ConsensusStrategy.Majority)
            let v = c.majority.unwrap_or("empty")
            if v == "empty" {
                (c.dissents.len() as I32) + (c.all_replies.len() as I32)
            } else {
                999
            }
        }
    "#;
    // Empty panel → majority None (unwrap_or "empty"), no dissents, no replies.
    assert_ok_exit(src, 0);
}

/// FirstAgreed strategy: short-circuits once two replies cluster.
/// Three members all replying "yes" — after the second, the swarm
/// stops; we see exactly two replies in `all_replies`.
#[test]
fn swarm_first_agreed_short_circuits_on_pair() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.mock("a", "yes", 1),
                Member.mock("b", "yes", 1),
                Member.mock("c", "yes", 1)
            ]
            let c = swarm("p", panel, DollarBudget.new(100), ConsensusStrategy.FirstAgreed)
            let n = c.all_replies.len()
            n as I32
        }
    "#;
    // Stopped after the second member → 2 replies.
    assert_ok_exit(src, 2);
}

/// Real-provider members synthesise a deterministic reply from the
/// prompt. `eval(user_input)` triggers `"UNSAFE"`. Verifies the
/// non-mock path through `Member.anthropic / openai / gemini` resolves
/// without needing API keys.
#[test]
fn swarm_real_provider_members_synthesise_reply() {
    let src = r#"
        fn main() -> I32 {
            let panel = [
                Member.anthropic("claude-opus-4-7"),
                Member.openai("gpt-5"),
                Member.gemini("gemini-2.5-pro")
            ]
            let c = swarm(
                "Is this safe? eval(user_input)",
                panel,
                DollarBudget.from_dollars(0.50),
                ConsensusStrategy.Majority
            )
            let v = c.majority.unwrap_or("")
            let mut score: I32 = 0
            if v == "UNSAFE" { score = score + 100 }
            score = score + (c.dissents.len() as I32)
            score
        }
    "#;
    // All three synthesise "UNSAFE" → unanimous → verdict + 0 dissents = 100.
    assert_ok_exit(src, 100);
}
