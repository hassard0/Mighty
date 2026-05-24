//! sdust-types: resolved-type representation, Hindley-Milner inference,
//! and bidirectional type checking for Stardust.
//!
//! This crate is invoked by `sdust-driver` after HIR lowering. The single
//! entry point is [`check_package`], which returns the list of
//! diagnostics produced by the type checker (errors and warnings).
//!
//! For consumers that need the typed side tables (the borrow checker, the
//! language server), [`check_package_typed`] returns a [`TypedPackage`]
//! holding the DefMap, TyArena, and per-expression / per-local resolved
//! types alongside the diagnostic list.

pub mod check;
pub mod defs;
pub mod diag;
pub mod infer;
pub mod items;
pub mod prelude;
pub mod resolve;
pub mod ty;

pub use defs::*;
pub use ty::*;

use sdust_diagnostics::Diagnostic;
use sdust_hir::{ExprId, FnId, Package};
use std::collections::HashMap;

/// Resolved-type side tables attached to a [`Package`] after type checking.
///
/// `expr_ty` is keyed by [`ExprId`]. `fn_params` and `fn_ret` are keyed by
/// the HIR [`FnId`]. `local_ty` maps the binding **name** (within its
/// declaring fn body) to its resolved type — slice 4's borrow checker walks
/// HIR linearly and tracks state per-name, so a name keyed map suffices.
/// For shadowed names we currently keep the latest binding (this is
/// acceptable because the borrow walker re-binds as it traverses).
#[derive(Debug, Default)]
pub struct TypedPackage {
    pub def_map: DefMap,
    pub ty_arena: TyArena,
    pub expr_ty: HashMap<ExprId, TyId>,
    /// Per-fn parameter types in declaration order.
    pub fn_params: HashMap<FnId, Vec<(String, TyId)>>,
    /// Per-fn declared return type.
    pub fn_ret: HashMap<FnId, TyId>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Full type-check entry point. Returns diagnostics only.
pub fn check_package(pkg: &Package) -> Vec<Diagnostic> {
    items::check(pkg)
}

/// Full type-check entry point that also returns the typed side tables.
pub fn check_package_typed(pkg: &Package) -> TypedPackage {
    items::check_typed(pkg)
}
