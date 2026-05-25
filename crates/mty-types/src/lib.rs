//! mty-types: resolved-type representation, Hindley-Milner inference,
//! and bidirectional type checking for Mighty.
//!
//! This crate is invoked by `mty-driver` after HIR lowering. The single
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
pub mod effects;
pub mod infer;
pub mod items;
pub mod prelude;
pub mod resolve;
pub mod sendable;
pub mod ty;

pub use defs::*;
pub use ty::*;

/// Slice 5: a small Copy-predicate mirroring `mty-borrow::copy::is_copy`
/// for use inside the type-checker (the borrow crate depends on us, so we
/// can't depend on it). Kept in sync via a comment-reference. The two
/// implementations agree on the slice-5 ruleset.
pub fn is_field_copy(ty: TyId, arena: &TyArena, defs: &DefMap) -> bool {
    match arena.get(ty) {
        TyData::Bool
        | TyData::Char
        | TyData::Str
        | TyData::Unit
        | TyData::Never
        | TyData::Duration
        | TyData::Size
        | TyData::Error
        | TyData::Module(_) => true,
        TyData::Int(_) | TyData::Float(_) => true,
        TyData::String | TyData::Bytes => false,
        TyData::RawPtr(_) => true,
        TyData::Ref { mutable, .. } => !*mutable,
        TyData::Fn { .. } => true,
        TyData::Tuple(xs) => xs.iter().all(|t| is_field_copy(*t, arena, defs)),
        TyData::Array { elem, .. } => is_field_copy(*elem, arena, defs),
        TyData::Adt(id, _) => {
            if defs.user_copy.contains(id) {
                return true;
            }
            match defs.adt(*id).map(|a| a.kind) {
                Some(AdtKind::Opaque) => true,
                Some(AdtKind::Struct | AdtKind::Enum) => false,
                None => true,
            }
        }
        TyData::Var(_) | TyData::Param(_) => false,
        TyData::Cap { .. } | TyData::Dyn { .. } => false,
    }
}

use mty_diagnostics::Diagnostic;
use mty_hir::{ExprId, FnId, Package};
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
    /// Slice 5: per-fn inferred effect set (deterministic order).
    pub fn_effects: HashMap<FnId, Vec<EffectId>>,
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
