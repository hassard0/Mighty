//! mty-borrow: Mighty's ownership / move / borrow / affine / arena
//! analysis. Runs after the type checker (`mty-types::check_package_typed`)
//! and consumes the typed-HIR side tables.
//!
//! v0.1 slice 4 shipped a **lexical, linear** walker. v0.3 (A54/A55/A56)
//! hardens it with:
//!
//! - **Field-level Place tracking** (`place::Place`) so `&mut s.a` and
//!   `&s.b` coexist (A54).
//! - **NLL last-use** (`nll::LastUseMap`) so `let r = &x; use(r); let m
//!   = &mut x` is accepted (A55).
//! - **Precise MT3009** for `move *ref` of non-Copy ref'd values (A56).
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
#[cfg(feature = "polonius")]
pub mod polonius;
pub mod sendable;
pub mod state;

use mty_diagnostics::Diagnostic;
use mty_hir::Package;
use mty_types::TypedPackage;

/// Run the slice-4 borrow checker over a typed package + its source HIR.
/// Returns a vector of diagnostics (errors only — borrow checking has no
/// warnings of its own; MT2026 from typeck is the lone warning code).
///
/// v0.21 dispatch: when the `polonius` cargo feature is enabled the
/// returned diagnostics include the union of NLL findings AND any
/// Polonius-only rejections (code MT3020) detected by the second-pass
/// datalog solver in [`polonius`]. When the feature is off only the
/// NLL pass runs (zero overhead — `cfg(feature = "polonius")` guards
/// both the module and the dispatch call below).
pub fn check_package(typed: &TypedPackage, pkg: &Package) -> Vec<Diagnostic> {
    #[allow(unused_mut)]
    let mut diags = flow::run(typed, pkg);
    #[cfg(feature = "polonius")]
    {
        diags.extend(polonius::run_polonius_pass(typed, pkg));
    }
    diags
}

/// Convenience helper: type-check `pkg` then borrow-check it. Returns the
/// concatenated diagnostics. If type-check reported any *error*, the
/// borrow check is skipped (its results would be misleading on a
/// type-broken program).
pub fn type_and_borrow_check(pkg: &Package) -> Vec<Diagnostic> {
    let typed = mty_types::check_package_typed(pkg);
    let mut diags = typed.diagnostics.clone();
    let has_type_error = diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error));
    if !has_type_error {
        diags.extend(check_package(&typed, pkg));
    }
    diags
}
