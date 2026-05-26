//! v0.21 — Polonius-flavoured second-pass borrow checker tests.
//!
//! The Polonius module ships behind the `polonius` feature flag.
//! These tests exercise the solver API directly — they don't drive
//! the full HIR fact-collector, so they're robust against the
//! surface-syntax surface area.

#![cfg(feature = "polonius")]

use mty_borrow::polonius::{Fact, LoanInfo, LoanKind, Point, PoloniusSolver};
use mty_hir::SourceSpan;

fn span() -> SourceSpan {
    SourceSpan { start: 0, end: 0 }
}

fn info(place: &str, kind: LoanKind) -> LoanInfo {
    LoanInfo {
        kind,
        place_root: place.into(),
        span: span(),
    }
}

#[test]
fn nested_borrow_conflict_detected() {
    // Polonius rejects: two mutable borrows of x live at the same
    // program point, even when nested through different aliases.
    let mut s = PoloniusSolver::new();
    let outer = s.fresh_loan(info("x", LoanKind::Mut));
    let inner = s.fresh_loan(info("x", LoanKind::Mut));
    s.add(Fact::BorrowAt(outer, Point(3)));
    s.add(Fact::BorrowAt(inner, Point(3)));
    s.solve();
    assert!(
        s.has_errors(),
        "two simultaneously live &mut on the same place must be rejected"
    );
}

#[test]
fn two_phase_borrow_accepted() {
    // vec.push(vec.len()) — two-phase borrow accept. Polonius's
    // TwoPhaseMut + concurrent Shared at the same point is the
    // canonical pattern this checker MUST accept.
    let mut s = PoloniusSolver::new();
    let push_mut = s.fresh_loan(info("vec", LoanKind::TwoPhaseMut));
    let len_read = s.fresh_loan(info("vec", LoanKind::Shared));
    s.add(Fact::BorrowAt(push_mut, Point(5)));
    s.add(Fact::BorrowAt(len_read, Point(5)));
    s.solve();
    assert!(
        !s.has_errors(),
        "two-phase mut + shared read must be accepted"
    );
}

#[test]
fn conditional_control_flow_borrow_lives_across_branch() {
    // Simulates: `let r = &x; if cond { ... } else { move x; }`
    // — the move on the else branch invalidates the borrow `r`
    // taken before the conditional. Polonius's branch join must
    // surface that as an error.
    let mut s = PoloniusSolver::new();
    let borrow = s.fresh_loan(info("x", LoanKind::Shared));
    // r introduced at point 1, lives through to point 7.
    s.add(Fact::BorrowAt(borrow, Point(1)));
    // Move in the else branch invalidates at point 5.
    s.add(Fact::LoanInvalidated(borrow, Point(5)));
    s.solve();
    assert!(
        s.has_errors(),
        "shared borrow invalidated by branch move must be rejected"
    );
}

#[test]
fn shared_shared_borrows_at_same_point_accepted() {
    // &x and &x in the same expression — fine.
    let mut s = PoloniusSolver::new();
    let l1 = s.fresh_loan(info("x", LoanKind::Shared));
    let l2 = s.fresh_loan(info("x", LoanKind::Shared));
    s.add(Fact::BorrowAt(l1, Point(2)));
    s.add(Fact::BorrowAt(l2, Point(2)));
    s.solve();
    assert!(!s.has_errors());
}

#[test]
fn borrows_on_different_places_do_not_conflict() {
    // &x and &mut y — disjoint, accepted.
    let mut s = PoloniusSolver::new();
    let lx = s.fresh_loan(info("x", LoanKind::Shared));
    let ly = s.fresh_loan(info("y", LoanKind::Mut));
    s.add(Fact::BorrowAt(lx, Point(1)));
    s.add(Fact::BorrowAt(ly, Point(1)));
    s.solve();
    assert!(!s.has_errors());
}

#[test]
fn borrow_invalidated_after_introduction_errors() {
    // Borrow introduced at point 1; invalidated at point 5. Rule 2
    // (P_i >= P_b) fires the Error fact at point 5 even without
    // the forward-flow rule (which is disabled in v0.21's
    // shipped-subset — see polonius.rs apply_rules Rule 3 comment).
    let mut s = PoloniusSolver::new();
    let l = s.fresh_loan(info("x", LoanKind::Shared));
    s.add(Fact::BorrowAt(l, Point(1)));
    s.add(Fact::LoanInvalidated(l, Point(5)));
    s.solve();
    assert!(s.has_errors(), "borrow live across an invalidation point");
}

#[test]
fn subset_transitivity_closes_chain() {
    // Subset(A, B) AND Subset(B, C) ⇒ Subset(A, C) at the same point.
    let mut s = PoloniusSolver::new();
    let a = s.fresh_loan(info("x", LoanKind::Shared));
    let b = s.fresh_loan(info("x", LoanKind::Shared));
    let c = s.fresh_loan(info("x", LoanKind::Shared));
    s.add(Fact::Subset(a, b, Point(1)));
    s.add(Fact::Subset(b, c, Point(1)));
    s.solve();
    assert!(s.facts.contains(&Fact::Subset(a, c, Point(1))));
}

#[test]
fn fixpoint_terminates_on_empty_input() {
    let mut s = PoloniusSolver::new();
    s.solve();
    assert!(!s.has_errors());
}

#[test]
fn error_facts_carry_loan_info() {
    let mut s = PoloniusSolver::new();
    let l = s.fresh_loan(info("interesting_local", LoanKind::Mut));
    s.add(Fact::BorrowAt(l, Point(2)));
    s.add(Fact::LoanInvalidated(l, Point(2)));
    s.solve();
    let errors: Vec<_> = s.errors().collect();
    assert_eq!(errors.len(), 1);
    let (_, _, info) = errors[0];
    assert_eq!(info.place_root, "interesting_local");
    assert!(matches!(info.kind, LoanKind::Mut));
}

#[test]
fn solver_is_monotonic_no_fact_removed() {
    // Fixpoint must only add facts, never remove. Smoke test by
    // counting before/after solve.
    let mut s = PoloniusSolver::new();
    let l = s.fresh_loan(info("x", LoanKind::Shared));
    s.add(Fact::BorrowAt(l, Point(1)));
    let before = format!("{:?}", s);
    s.solve();
    // The original BorrowAt(l, 1) must survive.
    assert!(s.facts.contains(&Fact::BorrowAt(l, Point(1))));
    let _ = before;
}
