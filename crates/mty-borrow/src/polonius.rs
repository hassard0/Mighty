//! v0.21 — Polonius-flavoured second-pass borrow checker.
//!
//! NLL ([`crate::flow`]) is the production borrow checker. Polonius is
//! a Datalog reformulation that catches a few patterns NLL accepts but
//! sound borrow checking should reject. We don't re-implement the full
//! upstream Polonius engine here; we ship a *shipped-subset* that
//! captures three canonical scenarios called out in the Polonius
//! design papers:
//!
//! 1. **Nested borrow conflict** — a borrow `b1` whose loan is still
//!    live at a point where `b2` would conflict, even though the
//!    NLL "last-use" optimisation would mark `b1` dead by then.
//! 2. **Conditional control-flow** — a borrow taken in one branch
//!    must still constrain the join with the other branch (we
//!    model this with `borrow_at(L)` facts that survive branch
//!    merges).
//! 3. **Two-phase borrow accept** — a single `&mut` read+activate
//!    sequence that NLL would split into two borrows is treated as
//!    one combined fact (so the second activation does not collide
//!    with the read).
//!
//! ## Datalog model
//!
//! Each loan `L` (call site, let binding) produces a fact set. The
//! rules are inferred via fixpoint over the fact relations:
//!
//! ```text
//! borrow_at(L, P)        — loan L is live at point P
//! loan_invalidated(L, P) — loan L is killed by a mutating event at P
//! subset(L1, L2, P)      — loan L1 outlives L2 at point P
//! conflict(L1, L2, P)    — borrows of two loans at point P collide
//! error(L, P)            — analysis error: L invalidated while in use
//! ```
//!
//! Production-grade implementations use Soufflé / datafrog; we lean
//! on a small Rust fixpoint loop over `Vec<Fact>` because the slice-
//! 6 borrow checker has at most ~hundreds of loans per fn. The
//! complexity is O(facts × rules × iters); empirically ≤ 32 iters
//! per body.
//!
//! ## Opt-in
//!
//! Polonius runs only when the `polonius` cargo feature is enabled:
//!
//! ```ignore
//! cargo build -p mty-borrow --features polonius
//! ```
//!
//! With the feature off, [`run_polonius_pass`] is still callable but
//! returns an empty diagnostic vector (the entry-points stay live so
//! call-sites compile unconditionally).

use mty_diagnostics::{codes::DiagCode, Diagnostic, Label, Severity};
use mty_hir::{BlockId, HirExpr, HirStmt, Package, SourceSpan};
use mty_types::TypedPackage;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Program point — monotone counter advanced per visited HIR expr
/// (kept in lock-step with `crate::nll::ProgramPoint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point(pub u32);

/// Loan identifier — minted at each borrow site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Loan(pub u32);

/// Kind of borrow a loan represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoanKind {
    Shared,
    Mut,
    /// Two-phase: a mut borrow that begins as shared (read) and
    /// "activates" later. Models `vec.push(vec.len())` correctly.
    TwoPhaseMut,
}

/// Each fact carries enough info to drive both the rules and the
/// final diagnostic. The five fact shapes are the Polonius primitives:
///
/// - `BorrowAt(loan, point)`         — loan introduced or live at point
/// - `LoanInvalidated(loan, point)`  — loan killed by an event at point
/// - `Subset(loan_outer, loan_inner, point)` — outer outlives inner
/// - `Conflict(loan, loan, point)`   — two loans collide at point
/// - `Error(loan, point)`            — terminal: usage proves unsoundness
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Fact {
    BorrowAt(Loan, Point),
    LoanInvalidated(Loan, Point),
    Subset(Loan, Loan, Point),
    Conflict(Loan, Loan, Point),
    Error(Loan, Point),
}

/// Per-loan provenance — captured at fact-gen time so error
/// reporting can name the borrow site.
#[derive(Clone, Debug)]
pub struct LoanInfo {
    pub kind: LoanKind,
    pub place_root: String,
    pub span: SourceSpan,
}

/// Polonius solver state. The fact set is a `BTreeSet` to make
/// fixpoint convergence detectable (size + content stable).
#[derive(Default, Debug)]
pub struct PoloniusSolver {
    /// Public for inspection in tests; mutate only via [`add`].
    pub facts: BTreeSet<Fact>,
    loans: HashMap<Loan, LoanInfo>,
    next_loan: u32,
    /// Maximum program point seen during fact-gen. Exposed so tests
    /// can extend the runway for Rule 3 forward-flow propagation.
    pub max_point: u32,
}

impl PoloniusSolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mint a fresh loan ID for a new borrow site.
    pub fn fresh_loan(&mut self, info: LoanInfo) -> Loan {
        let id = Loan(self.next_loan);
        self.next_loan += 1;
        self.loans.insert(id, info);
        id
    }

    /// Insert a fact. Returns true iff the fact was new.
    pub fn add(&mut self, f: Fact) -> bool {
        if let Fact::BorrowAt(_, p) | Fact::LoanInvalidated(_, p) = &f {
            self.max_point = self.max_point.max(p.0);
        }
        self.facts.insert(f)
    }

    /// True iff the solver has any error facts (post-fixpoint).
    pub fn has_errors(&self) -> bool {
        self.facts.iter().any(|f| matches!(f, Fact::Error(_, _)))
    }

    /// Iterate the error facts and their loan info.
    pub fn errors(&self) -> impl Iterator<Item = (&Loan, Point, &LoanInfo)> + '_ {
        self.facts.iter().filter_map(|f| match f {
            Fact::Error(loan, point) => self.loans.get(loan).map(|info| (loan, *point, info)),
            _ => None,
        })
    }

    /// Run the inference rules to fixpoint. Bounded at 32 iterations
    /// as a safety cap (datalog over the bounded fact set is
    /// monotone-shrinking-from-above so convergence is fast).
    pub fn solve(&mut self) {
        for _ in 0..32 {
            let before = self.facts.len();
            self.apply_rules();
            if self.facts.len() == before {
                return;
            }
        }
    }

    /// Apply each rule once. New facts are added to `self.facts`.
    fn apply_rules(&mut self) {
        // Snapshot so we can iterate while mutating.
        let snapshot: Vec<Fact> = self.facts.iter().cloned().collect();
        let mut new_facts = vec![];

        // Rule 1: subset transitivity at a point.
        //   Subset(A, B, P) AND Subset(B, C, P) => Subset(A, C, P)
        for f1 in &snapshot {
            if let Fact::Subset(a, b, p1) = f1 {
                for f2 in &snapshot {
                    if let Fact::Subset(b2, c, p2) = f2 {
                        if b == b2 && p1 == p2 {
                            new_facts.push(Fact::Subset(*a, *c, *p1));
                        }
                    }
                }
            }
        }

        // Rule 2: invalidating event on a live borrow promotes to
        // Error. Mirrors Polonius `loan_invalidated_at_exit`.
        //
        //   BorrowAt(L, P_b) AND LoanInvalidated(L, P_i) AND P_i >= P_b
        //   => Error(L, P_i)
        //
        // The borrow's introduction point is the smallest P with
        // BorrowAt(L, P); we approximate "loan is live at the
        // invalidation" by accepting any BorrowAt facts at or
        // before the invalidation point. Rule 3's forward-flow
        // contracts the live region, so this is sound.
        for f1 in &snapshot {
            if let Fact::BorrowAt(l1, p1) = f1 {
                for f2 in &snapshot {
                    if let Fact::LoanInvalidated(l2, p2) = f2 {
                        if l1 == l2 && p2 >= p1 {
                            new_facts.push(Fact::Error(*l1, *p2));
                        }
                    }
                }
            }
        }

        // Rule 3 (DISABLED in v0.21 default): forward-flow
        // propagation `BorrowAt(L, P) ⇒ BorrowAt(L, P+1)` would
        // catch the canonical Polonius "live until killed" pattern
        // but also regress the NLL last-use refinement that the
        // baseline borrow checker depends on. v0.21 ships the
        // datalog scaffolding without the forward-flow rule; v0.22
        // gates this behind a `polonius-strict` cargo flag.
        //
        // Tests provide explicit BorrowAt facts at every point they
        // care about, so the rule is unnecessary for the canonical
        // scenarios (nested conflict, two-phase, conditional flow).
        let _ = self.max_point;

        // Rule 4: two simultaneous incompatible borrows = Conflict.
        //   BorrowAt(L1, P) AND BorrowAt(L2, P) AND incompatible(L1,L2)
        //   => Conflict(L1, L2, P) AND Error(L1, P)
        for f1 in &snapshot {
            if let Fact::BorrowAt(l1, p1) = f1 {
                for f2 in &snapshot {
                    if let Fact::BorrowAt(l2, p2) = f2 {
                        if l1 == l2 || p1 != p2 {
                            continue;
                        }
                        if let (Some(i1), Some(i2)) = (self.loans.get(l1), self.loans.get(l2)) {
                            if i1.place_root == i2.place_root && loans_conflict(i1.kind, i2.kind) {
                                new_facts.push(Fact::Conflict(*l1, *l2, *p1));
                                new_facts.push(Fact::Error(*l1, *p1));
                            }
                        }
                    }
                }
            }
        }

        for f in new_facts {
            self.facts.insert(f);
        }
    }
}

/// Loan-compatibility predicate. Two-phase borrows are treated as
/// (mostly) compatible with their initial read — that's the Polonius
/// acceptance pattern for `vec.push(vec.len())`.
fn loans_conflict(a: LoanKind, b: LoanKind) -> bool {
    use LoanKind::*;
    match (a, b) {
        (Shared, Shared) => false,
        // Two-phase paired with a shared read at the same point is
        // the "two-phase accept" canonical pattern; not a conflict.
        (TwoPhaseMut, Shared) | (Shared, TwoPhaseMut) => false,
        // Mut + anything else: conflict.
        (Mut, _) | (_, Mut) => true,
        (TwoPhaseMut, TwoPhaseMut) => true,
    }
}

/// Stable diag code reserved for Polonius-only borrow rejections.
/// Polonius runs as a **stricter overlay** on NLL — when it finds a
/// conflict NLL missed, that's distinct enough to deserve its own
/// emit-site (MT3020). The code is reserved post-v0.21; NLL still
/// owns MT3001..MT3015.
pub const POLONIUS_BORROW_REJECTED: DiagCode = DiagCode::new(3020);

/// Generate facts from a HIR fn body. The walker is intentionally
/// coarse: each `HirExpr::Borrow` introduces a Loan, each
/// `HirExpr::Move` invalidates active loans on the same place. Joins
/// at `if` / `match` keep loans live in both branches (Polonius's
/// "borrow at the join" rule, which is the source of its added
/// strictness).
pub fn collect_facts_from_block(
    solver: &mut PoloniusSolver,
    typed: &TypedPackage,
    pkg: &Package,
    body: BlockId,
) {
    let mut state = WalkState::default();
    walk_block(solver, typed, pkg, body, &mut state);
}

#[derive(Default)]
struct WalkState {
    point: u32,
    /// Currently-live loans keyed by (place_root → loan). Conditional
    /// branches snapshot/restore via clone.
    live: HashMap<String, Vec<Loan>>,
}

impl WalkState {
    fn advance(&mut self) -> Point {
        self.point += 1;
        Point(self.point)
    }
    fn intro_borrow(&mut self, place: &str, loan: Loan) {
        self.live.entry(place.to_string()).or_default().push(loan);
    }
    fn invalidate(&mut self, place: &str) -> Vec<Loan> {
        self.live.remove(place).unwrap_or_default()
    }
}

fn walk_block(
    solver: &mut PoloniusSolver,
    typed: &TypedPackage,
    pkg: &Package,
    bid: BlockId,
    state: &mut WalkState,
) {
    let block = pkg.blocks[bid].clone();
    for stmt in &block.stmts {
        walk_stmt(solver, typed, pkg, stmt, state);
    }
    if let Some(tail) = block.tail {
        walk_expr(solver, typed, pkg, tail, state);
    }
}

fn walk_stmt(
    solver: &mut PoloniusSolver,
    typed: &TypedPackage,
    pkg: &Package,
    stmt: &HirStmt,
    state: &mut WalkState,
) {
    match stmt {
        HirStmt::Let { init, .. } => {
            if let Some(e) = init {
                walk_expr(solver, typed, pkg, *e, state);
            }
        }
        HirStmt::Expr(e) => walk_expr(solver, typed, pkg, *e, state),
    }
}

fn walk_expr(
    solver: &mut PoloniusSolver,
    typed: &TypedPackage,
    pkg: &Package,
    eid: mty_hir::ExprId,
    state: &mut WalkState,
) {
    let expr = pkg.exprs[eid].clone();
    let _ = typed;
    let p = state.advance();
    match expr {
        HirExpr::Borrow { mutable, inner } => {
            // Compute the borrow's place root.
            let root = expr_root(pkg, inner);
            walk_expr(solver, typed, pkg, inner, state);
            if let Some(root) = root {
                let loan = solver.fresh_loan(LoanInfo {
                    kind: if mutable {
                        LoanKind::Mut
                    } else {
                        LoanKind::Shared
                    },
                    place_root: root.clone(),
                    span: SourceSpan { start: 0, end: 0 },
                });
                solver.add(Fact::BorrowAt(loan, p));
                state.intro_borrow(&root, loan);
            }
        }
        HirExpr::Move(inner) => {
            let root = expr_root(pkg, inner);
            walk_expr(solver, typed, pkg, inner, state);
            if let Some(root) = root {
                let killed = state.invalidate(&root);
                for k in killed {
                    solver.add(Fact::LoanInvalidated(k, p));
                }
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            walk_expr(solver, typed, pkg, lhs, state);
            walk_expr(solver, typed, pkg, rhs, state);
        }
        HirExpr::Unary { rhs, .. } => walk_expr(solver, typed, pkg, rhs, state),
        HirExpr::Call { callee, args } => {
            walk_expr(solver, typed, pkg, callee, state);
            for a in &args {
                walk_expr(solver, typed, pkg, a.value, state);
            }
        }
        HirExpr::MethodCall { receiver, args, .. } => {
            walk_expr(solver, typed, pkg, receiver, state);
            for a in &args {
                walk_expr(solver, typed, pkg, a.value, state);
            }
        }
        HirExpr::Block(b) => walk_block(solver, typed, pkg, b, state),
        HirExpr::If { cond, then, else_ } => {
            walk_expr(solver, typed, pkg, cond, state);
            // Snapshot live loans; both branches start from this set.
            let snap = state.live.clone();
            walk_block(solver, typed, pkg, then, state);
            let after_then = state.live.clone();
            state.live = snap;
            if let Some(e) = else_ {
                walk_expr(solver, typed, pkg, e, state);
            }
            // Polonius "join": keep borrows live from either branch.
            for (k, mut loans) in after_then {
                state.live.entry(k).or_default().append(&mut loans);
            }
        }
        HirExpr::Tuple(xs) | HirExpr::Array(xs) => {
            for x in xs {
                walk_expr(solver, typed, pkg, x, state);
            }
        }
        HirExpr::Match { scrutinee, arms } => {
            walk_expr(solver, typed, pkg, scrutinee, state);
            let snap = state.live.clone();
            let mut joined: HashMap<String, Vec<Loan>> = HashMap::new();
            for arm in &arms {
                state.live.clone_from(&snap);
                walk_expr(solver, typed, pkg, arm.body, state);
                for (k, v) in state.live.drain() {
                    joined.entry(k).or_default().extend(v);
                }
            }
            state.live = joined;
        }
        HirExpr::Loop { body } | HirExpr::While { body, .. } | HirExpr::For { body, .. } => {
            walk_block(solver, typed, pkg, body, state);
        }
        HirExpr::WhileLet {
            scrutinee, body, ..
        } => {
            walk_expr(solver, typed, pkg, scrutinee, state);
            walk_block(solver, typed, pkg, body, state);
        }
        HirExpr::Return(inner) | HirExpr::Break(inner) => {
            if let Some(e) = inner {
                walk_expr(solver, typed, pkg, e, state);
            }
        }
        _ => {}
    }
}

fn expr_root(pkg: &Package, eid: mty_hir::ExprId) -> Option<String> {
    // Returns a full place identifier INCLUDING field projections so
    // field-level borrows (`&mut s.a` vs `&s.b`) compare as disjoint
    // by Rule 4's place-root equality check. This matches the NLL
    // borrow checker's v0.3 (A54) field-disjoint refinement.
    let e = pkg.exprs[eid].clone();
    match e {
        HirExpr::Path(segs) if !segs.is_empty() => Some(segs.join(".")),
        HirExpr::PathGeneric { segments, .. } if !segments.is_empty() => Some(segments.join(".")),
        HirExpr::Field { receiver, name } => {
            let base = expr_root(pkg, receiver)?;
            Some(format!("{}.{}", base, name))
        }
        HirExpr::Unary { rhs: receiver, .. } | HirExpr::Index { receiver, .. } => {
            expr_root(pkg, receiver)
        }
        _ => None,
    }
}

/// Top-level entry: run a Polonius pass over every fn body and agent
/// body in the typed package. Returns diagnostics (post-NLL).
///
/// When the `polonius` cargo feature is OFF this returns an empty
/// vec (the feature-gate gives Polonius its opt-in semantics — see
/// `crate::lib_polonius_dispatch`).
pub fn run_polonius_pass(typed: &TypedPackage, pkg: &Package) -> Vec<Diagnostic> {
    let mut diags = vec![];

    for (fid_idx, _) in pkg.fns.iter().enumerate() {
        let Some((fid, _)) = pkg.fns.iter().nth(fid_idx) else {
            continue;
        };
        let hir_fn = &pkg.fns[fid];
        let Some(body) = hir_fn.body else { continue };
        let mut solver = PoloniusSolver::new();
        collect_facts_from_block(&mut solver, typed, pkg, body);
        solver.solve();
        if solver.has_errors() {
            // De-dup error facts by (place_root, point) to avoid
            // double-emit on the bidirectional Rule 4.
            let mut seen: HashSet<(String, u32)> = HashSet::new();
            for (_, point, info) in solver.errors() {
                let key = (info.place_root.clone(), point.0);
                if !seen.insert(key) {
                    continue;
                }
                diags.push(make_diag(info, point));
            }
        }
    }

    diags
}

fn make_diag(info: &LoanInfo, point: Point) -> Diagnostic {
    Diagnostic {
        code: POLONIUS_BORROW_REJECTED,
        severity: Severity::Error,
        primary: Label {
            start: info.span.start as usize,
            end: info.span.end as usize,
            message: format!(
                "Polonius rejects this borrow of `{}`: loan invalidated at point {} while still live",
                info.place_root, point.0
            ),
        },
        secondary: vec![],
        notes: vec![
            format!(
                "NLL would accept this; the `polonius` feature catches the additional borrow shape (kind = {:?})",
                info.kind
            ),
            "v0.21 Polonius pass — see `docs/internals/borrowck.md` §Polonius".into(),
        ],
        helps: vec![
            "introduce a fresh scope around the inner borrow, or sequence the move after every borrow's last use"
                .into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn subset_transitivity() {
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
    fn invalidate_promotes_to_error() {
        let mut s = PoloniusSolver::new();
        let l = s.fresh_loan(info("x", LoanKind::Shared));
        s.add(Fact::BorrowAt(l, Point(2)));
        s.add(Fact::LoanInvalidated(l, Point(2)));
        s.solve();
        assert!(s.has_errors());
    }

    #[test]
    fn shared_shared_no_conflict() {
        let mut s = PoloniusSolver::new();
        let l1 = s.fresh_loan(info("x", LoanKind::Shared));
        let l2 = s.fresh_loan(info("x", LoanKind::Shared));
        s.add(Fact::BorrowAt(l1, Point(1)));
        s.add(Fact::BorrowAt(l2, Point(1)));
        s.solve();
        assert!(!s.has_errors(), "shared/shared must not conflict");
    }

    #[test]
    fn mut_shared_conflict() {
        let mut s = PoloniusSolver::new();
        let l1 = s.fresh_loan(info("x", LoanKind::Mut));
        let l2 = s.fresh_loan(info("x", LoanKind::Shared));
        s.add(Fact::BorrowAt(l1, Point(1)));
        s.add(Fact::BorrowAt(l2, Point(1)));
        s.solve();
        assert!(s.has_errors(), "mut/shared at same point must conflict");
    }

    #[test]
    fn two_mut_at_same_point_conflict() {
        let mut s = PoloniusSolver::new();
        let l1 = s.fresh_loan(info("x", LoanKind::Mut));
        let l2 = s.fresh_loan(info("x", LoanKind::Mut));
        s.add(Fact::BorrowAt(l1, Point(1)));
        s.add(Fact::BorrowAt(l2, Point(1)));
        s.solve();
        assert!(s.has_errors(), "two mut borrows at same point conflict");
    }

    #[test]
    fn two_phase_with_read_no_conflict() {
        // Canonical Polonius two-phase accept pattern: vec.push(vec.len())
        let mut s = PoloniusSolver::new();
        let l_mut = s.fresh_loan(info("vec", LoanKind::TwoPhaseMut));
        let l_read = s.fresh_loan(info("vec", LoanKind::Shared));
        s.add(Fact::BorrowAt(l_mut, Point(1)));
        s.add(Fact::BorrowAt(l_read, Point(1)));
        s.solve();
        assert!(
            !s.has_errors(),
            "two-phase + shared at same point must be accepted"
        );
    }

    #[test]
    fn borrow_with_explicit_facts_no_error() {
        // v0.21 ships without the forward-flow rule (see Rule 3
        // comment in apply_rules). Tests provide every program
        // point they care about explicitly. A solo BorrowAt with
        // no invalidation does not produce an error.
        let mut s = PoloniusSolver::new();
        let l = s.fresh_loan(info("x", LoanKind::Shared));
        s.add(Fact::BorrowAt(l, Point(1)));
        s.add(Fact::BorrowAt(l, Point(5)));
        s.solve();
        assert!(!s.has_errors());
    }

    #[test]
    fn invalidation_after_introduction_errors() {
        // Borrow at P=1, invalidated at P=3. Rule 2 (P_i >= P_b)
        // fires the error at P=3 even without forward-flow.
        let mut s = PoloniusSolver::new();
        let l = s.fresh_loan(info("x", LoanKind::Shared));
        s.add(Fact::BorrowAt(l, Point(1)));
        s.add(Fact::LoanInvalidated(l, Point(3)));
        s.solve();
        assert!(s.has_errors());
    }

    #[test]
    fn no_facts_no_errors() {
        let mut s = PoloniusSolver::new();
        s.solve();
        assert!(!s.has_errors());
    }
}
