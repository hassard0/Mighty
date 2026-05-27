//! NLL (non-lexical lifetimes) last-use analysis (v0.3 / A55).
//!
//! Rust's NLL replaces lexical scope-end with **last-use** as the
//! deactivation point of a borrow. v0.3 ships a hand-rolled simplified
//! NLL that tracks last-use of each *local* in linear program order.
//!
//! Algorithm (per fn body):
//!
//! 1. **Pre-pass** walks the typed HIR in source order, assigning every
//!    expression a monotonically increasing `ProgramPoint`. For each
//!    occurrence of a `Path([name])` we record the (name, point) pair.
//! 2. The walker keeps `last_use: HashMap<name, ProgramPoint>` — the
//!    HIGHEST point where each name is used. This is the "last use".
//! 3. During the main borrow-check walk, when we reach a `Path` use, we
//!    consult `last_use[name]`: if the current point equals `last_use`,
//!    the borrower's lifetime ends *after* this use, and we can decay
//!    any borrows whose **borrower binding** was this local.
//!
//! This is NOT polonius — there's no fact-based reasoner, no CFG split
//! on branches, no two-phase borrows. But it gives us the canonical
//! `let r = &x; use(r); let m = &mut x` pattern.
//!
//! ### Branch handling
//!
//! Inside `if`/`match` arms, last-use is computed *within* the arm; the
//! join after the branch resets borrow state to the snapshot, so per-arm
//! decay is local.
//!
//! ### What this loses vs polonius
//!
//! - Two-phase borrows (`vec.push(vec.len())`).
//! - Conditional borrows that flow through a loop back-edge.
//! - Borrow that ends only on one branch of a diamond.
//!
//! Documented in `docs/internals/borrowck.md` §17 and amendment A55.

use mty_hir::*;
use std::collections::HashMap;

/// A monotone program-point counter (per-fn).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProgramPoint(pub u32);

/// Pre-pass result: the highest program-point at which each local name
/// is referenced as a `Path([name])` (or `PathGeneric` single-segment).
/// Names not in the map are never used after their introduction.
#[derive(Default, Debug)]
pub struct LastUseMap {
    map: HashMap<String, ProgramPoint>,
}

impl LastUseMap {
    pub fn get(&self, name: &str) -> Option<ProgramPoint> {
        self.map.get(name).copied()
    }

    pub fn record(&mut self, name: &str, point: ProgramPoint) {
        let e = self.map.entry(name.to_string()).or_insert(point);
        if point > *e {
            *e = point;
        }
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.map.keys()
    }
}

/// Pre-pass: walk a fn body and compute the last-use of every name.
/// `counter` is the SHARED point counter — the main walker uses the
/// same monotone progression so that `LastUseMap`'s points are
/// comparable to the walker's current point.
pub fn compute_last_use(pkg: &Package, body: BlockId) -> (LastUseMap, u32) {
    let mut ctx = Pre {
        pkg,
        map: LastUseMap::default(),
        point: 0,
    };
    ctx.visit_block(body);
    (ctx.map, ctx.point)
}

struct Pre<'a> {
    pkg: &'a Package,
    map: LastUseMap,
    point: u32,
}

impl<'a> Pre<'a> {
    fn advance(&mut self) -> ProgramPoint {
        let p = ProgramPoint(self.point);
        self.point += 1;
        p
    }

    fn visit_block(&mut self, bid: BlockId) {
        let block = self.pkg.blocks[bid].clone();
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
        }
        if let Some(tail) = block.tail {
            self.visit_expr(tail);
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStmt) {
        match stmt {
            HirStmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(*e);
                }
            }
            HirStmt::Expr(e) => self.visit_expr(*e),
        }
    }

    fn visit_expr(&mut self, eid: ExprId) {
        let expr = self.pkg.exprs[eid].clone();
        match expr {
            HirExpr::Path(segs) => {
                // Multi-segment paths (`s.a`) are folded by the lower
                // pass into a single Path node. We advance ONE point
                // per path expression (matching the main walker's
                // single advance per Path read) and record the root
                // segment as the "used" name.
                if !segs.is_empty() {
                    let p = self.advance();
                    self.map.record(&segs[0], p);
                }
            }
            HirExpr::PathGeneric { segments, .. } => {
                if !segments.is_empty() {
                    let p = self.advance();
                    self.map.record(&segments[0], p);
                }
            }
            HirExpr::Literal(_) | HirExpr::HtmlTemplate(_) | HirExpr::Error => {}
            HirExpr::Block(b) | HirExpr::Unsafe(b) => self.visit_block(b),
            HirExpr::Tuple(xs) | HirExpr::Array(xs) => xs.iter().for_each(|e| self.visit_expr(*e)),
            HirExpr::Binary { lhs, rhs, .. } => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            HirExpr::Unary { rhs, .. } => self.visit_expr(rhs),
            HirExpr::Borrow { inner, .. } | HirExpr::Move(inner) => self.visit_expr(inner),
            HirExpr::Call { callee, args } => {
                self.visit_expr(callee);
                for a in args {
                    self.visit_expr(a.value);
                }
            }
            HirExpr::MethodCall { receiver, args, .. } => {
                self.visit_expr(receiver);
                for a in args {
                    self.visit_expr(a.value);
                }
            }
            HirExpr::Field { receiver, .. } => self.visit_expr(receiver),
            HirExpr::Index { receiver, idx } => {
                self.visit_expr(receiver);
                self.visit_expr(idx);
            }
            HirExpr::If { cond, then, else_ } => {
                self.visit_expr(cond);
                self.visit_block(then);
                if let Some(e) = else_ {
                    self.visit_expr(e);
                }
            }
            HirExpr::IfLet {
                scrutinee,
                then,
                else_,
                ..
            } => {
                self.visit_expr(scrutinee);
                self.visit_block(then);
                if let Some(e) = else_ {
                    self.visit_expr(e);
                }
            }
            HirExpr::Match { scrutinee, arms } => {
                self.visit_expr(scrutinee);
                for a in arms {
                    if let Some(g) = a.guard {
                        self.visit_expr(g);
                    }
                    self.visit_expr(a.body);
                }
            }
            HirExpr::For { iter, body, .. } => {
                self.visit_expr(iter);
                self.visit_block(body);
            }
            HirExpr::While { cond, body } => {
                self.visit_expr(cond);
                self.visit_block(body);
            }
            HirExpr::WhileLet {
                scrutinee, body, ..
            } => {
                self.visit_expr(scrutinee);
                self.visit_block(body);
            }
            HirExpr::Loop { body } => self.visit_block(body),
            HirExpr::Return(e) => {
                if let Some(x) = e {
                    self.visit_expr(x);
                }
            }
            HirExpr::Break(e) => {
                if let Some(x) = e {
                    self.visit_expr(x);
                }
            }
            HirExpr::Continue => {}
            HirExpr::Struct { fields, .. } => {
                for (_, e) in fields {
                    self.visit_expr(e);
                }
            }
            HirExpr::Map(entries) => {
                for (k, v) in entries {
                    self.visit_expr(k);
                    self.visit_expr(v);
                }
            }
            HirExpr::Send { target, args, .. } | HirExpr::Ask { target, args, .. } => {
                self.visit_expr(target);
                for a in args {
                    self.visit_expr(a.value);
                }
            }
            HirExpr::Deadline { inner, dur } => {
                self.visit_expr(dur);
                self.visit_expr(inner);
            }
            HirExpr::Question(e) | HirExpr::Run(e) => self.visit_expr(e),
            HirExpr::Spawn { inner, .. } => self.visit_expr(inner),
            HirExpr::Detach(e) | HirExpr::Join(e) => self.visit_expr(e),
            HirExpr::Arena { body, .. } => self.visit_expr(body),
            HirExpr::TaskScope { body, deadline } => {
                if let Some(d) = deadline {
                    self.visit_expr(d);
                }
                self.visit_block(body);
            }
            HirExpr::Budget { entries, body } => {
                for (_, e) in entries {
                    self.visit_expr(e);
                }
                self.visit_expr(body);
            }
            HirExpr::Sandbox { entries, body, .. } => {
                for (_, e) in entries {
                    self.visit_expr(e);
                }
                self.visit_block(body);
            }
            HirExpr::Cast { lhs, .. } => self.visit_expr(lhs),
            HirExpr::Lambda { body, .. } => self.visit_block(body),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_use_advances_monotonically() {
        let mut m = LastUseMap::default();
        m.record("x", ProgramPoint(1));
        m.record("x", ProgramPoint(5));
        m.record("x", ProgramPoint(3)); // not the latest
        assert_eq!(m.get("x"), Some(ProgramPoint(5)));
    }
}
