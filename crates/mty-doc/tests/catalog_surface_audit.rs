//! v0.41 T5 — catalog-surface integration test.
//!
//! Parses every `.docstub` file in `crates/mty-stdlib/docs/`, walks the
//! resulting catalog, and fails if any entry's symbol does not resolve
//! to a real stdlib surface item (prelude registration, interp ctor,
//! interp method dispatch, host dispatcher arm, or `# concept-doc` /
//! `# future` opt-in marker).
//!
//! See `crates/mty-doc/src/surface_audit.rs` for the resolution rules.

use mty_doc::{audit_catalog, build_extracted_catalog, render_audit_report};

#[test]
fn every_docstub_entry_resolves_to_real_surface() {
    let total = build_extracted_catalog().len();
    let unresolved = audit_catalog();
    // Allow `# concept-doc` / `# future` marked entries — they're
    // documentation-only or intentionally planned. Everything else
    // must resolve.
    let hard_fail: Vec<_> = unresolved
        .iter()
        .filter(|u| !u.flagged_as_concept && !u.flagged_as_future)
        .collect();
    assert!(
        hard_fail.is_empty(),
        "\n{}\n{} unresolved entries (concept/future opt-ins excluded)",
        render_audit_report(&unresolved, total),
        hard_fail.len()
    );
}
