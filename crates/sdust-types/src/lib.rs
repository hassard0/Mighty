//! sdust-types: resolved-type representation, Hindley-Milner inference,
//! and bidirectional type checking for Stardust.
//!
//! This crate is invoked by `sdust-driver` after HIR lowering. The single
//! entry point is [`check_package`], which returns the list of
//! diagnostics produced by the type checker (errors and warnings).

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
use sdust_hir::Package;

/// Full type-check entry point.
pub fn check_package(pkg: &Package) -> Vec<Diagnostic> {
    items::check(pkg)
}
