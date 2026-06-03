//! HIR → SIR lowering.
//!
//! Per design D4–D14, this module emits basic-block SIR functions
//! from a typed + borrow-checked HIR package. The implementation is a
//! pragmatic best-effort lowerer: shapes the slice-6 interpreter
//! exercises are lowered precisely; shapes that aren't yet needed for
//! the 20 canonical examples lower to a deterministic Unit placeholder
//! so the lowerer is total (never panics).

mod ctx;
mod exprs;
mod items;
mod pats;
mod stmts;
mod ty;

pub use ctx::*;

use crate::ir::Program;
use mty_hir::Package;
use mty_types::TypedPackage;

/// Public lowering entry point. Consumes a borrow-checked typed package
/// and produces an executable SIR program. Lowering errors are recorded
/// on `Program::errors` (the lowerer is total).
pub fn lower_package(pkg: &Package, typed: &TypedPackage) -> Program {
    let mut ctx = LowerCtx::new(pkg, typed);
    items::lower_all_items(&mut ctx);
    let mut prog = ctx.finish();
    // v0.47 T4 — mirror DefMap's auto-Drop table onto the IR Program so
    // codegen + interp can resolve the per-ADT runtime drop symbol
    // without depending on mty-types. Also: drive the per-fn
    // `Stmt::Drop(local)` insertion pass that turns owned drop-typed
    // locals into auto-closed values at every fn-exit terminator.
    prog.adt_drop_fns.clone_from(&typed.def_map.mty_drop_fns);
    items::inject_auto_drop_stmts(&mut prog);
    prog
}
