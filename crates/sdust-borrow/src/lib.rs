//! sdust-borrow: Stardust's ownership / move / borrow / affine / arena
//! analysis. Runs after the type checker (`sdust-types::check_package_typed`)
//! and consumes the typed-HIR side tables.
//!
//! v0.1 slice 4 shipped a **lexical, linear** walker. v0.3 (A54/A55/A56)
//! hardens it with:
//!
//! - **Field-level Place tracking** (`place::Place`) so `&mut s.a` and
//!   `&s.b` coexist (A54).
//! - **NLL last-use** (`nll::LastUseMap`) so `let r = &x; use(r); let m
//!   = &mut x` is accepted (A55).
//! - **Precise SD3009** for `move *ref` of non-Copy ref'd values (A56).
//!
//! Public surface: [`check_package`] returns a `Vec<Diagnostic>` carrying
//! borrow-checker errors. The [`drop_plan::DropPlan`] is also produced
//! internally for future codegen consumption.

pub mod arena_region;
pub mod copy;
pub mod diag;
pub mod drop_plan;
pub mod flow;
pub mod nll;
pub mod place;
pub mod sendable;
pub mod state;

use sdust_diagnostics::Diagnostic;
use sdust_hir::Package;
use sdust_types::TypedPackage;

/// Run the slice-4 borrow checker over a typed package + its source HIR.
/// Returns a vector of diagnostics (errors only — borrow checking has no
/// warnings of its own; SD2026 from typeck is the lone warning code).
pub fn check_package(typed: &TypedPackage, pkg: &Package) -> Vec<Diagnostic> {
    flow::run(typed, pkg)
}

/// Convenience helper: type-check `pkg` then borrow-check it. Returns the
/// concatenated diagnostics. If type-check reported any *error*, the
/// borrow check is skipped (its results would be misleading on a
/// type-broken program).
pub fn type_and_borrow_check(pkg: &Package) -> Vec<Diagnostic> {
    let typed = sdust_types::check_package_typed(pkg);
    let mut diags = typed.diagnostics.clone();
    let has_type_error = diags
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error));
    if !has_type_error {
        diags.extend(check_package(&typed, pkg));
    }
    diags
}
