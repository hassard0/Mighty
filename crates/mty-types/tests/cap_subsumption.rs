//! v0.3 (A65) — sanity test for `CapConstraint::is_narrower_or_eq`.
//! Exercises Path-prefix and Any subsumption used by the MT4010 check.

use mty_types::ty::CapConstraint;

#[test]
fn any_is_widest_only_narrower_than_any() {
    assert!(CapConstraint::Any.is_narrower_or_eq(&CapConstraint::Any));
    // Path narrower than Any: yes.
    assert!(CapConstraint::Path("/data".into()).is_narrower_or_eq(&CapConstraint::Any));
    // Any narrower than Path: no.
    assert!(!CapConstraint::Any.is_narrower_or_eq(&CapConstraint::Path("/data".into())));
}

#[test]
fn cap_subsumption_path_too_broad() {
    // /home is NOT a subpath of /data → not narrower → MT4010 should
    // fire when the arg /home is passed where param /data is required.
    let arg = CapConstraint::Path("/home".into());
    let param = CapConstraint::Path("/data".into());
    assert!(
        !arg.is_narrower_or_eq(&param),
        "/home is too broad for /data"
    );
}

#[test]
fn cap_subsumption_path_subpath_is_narrower() {
    // /data/cats is narrower than /data — accepted.
    let arg = CapConstraint::Path("/data/cats".into());
    let param = CapConstraint::Path("/data".into());
    assert!(arg.is_narrower_or_eq(&param));
}

#[test]
fn readonly_narrower_than_readonly() {
    let arg = CapConstraint::ReadOnly;
    let param = CapConstraint::ReadOnly;
    assert!(arg.is_narrower_or_eq(&param));
}
