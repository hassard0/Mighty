//! v0.21 — cap-name resolver unit tests.
//!
//! These tests drive [`mty_types::cap_resolver::CapResolver`] directly,
//! exercising the six MT4060..MT4065 paths without any HIR plumbing.
//! Surface-syntax wiring lands in v0.22; the resolver API is the
//! load-bearing piece consumed by `mty-types::cap_check` and the
//! eventual `with cap(...)` lowering.

use mty_types::cap_resolver::{CapResolutionError, CapResolver, CapSpec};
use mty_types::ty::{CapConstraint, CapFamily};

#[test]
fn mt4060_unbound_name() {
    let resolver = CapResolver::new();
    match resolver.resolve("fs") {
        Err(CapResolutionError::Unbound { name }) => assert_eq!(name, "fs"),
        other => panic!("expected Unbound, got {:?}", other),
    }
}

#[test]
fn mt4060_unbound_name_after_unrelated_decls() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("net", CapSpec::top(CapFamily::Net))
        .expect("declare net");
    match resolver.resolve("fs") {
        Err(CapResolutionError::Unbound { name }) => assert_eq!(name, "fs"),
        other => panic!("expected Unbound, got {:?}", other),
    }
}

#[test]
fn declared_cap_is_resolvable() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("fs", CapSpec::top(CapFamily::Fs))
        .expect("declare");
    let spec = resolver.resolve("fs").expect("resolves");
    assert_eq!(spec.family, CapFamily::Fs);
    assert_eq!(spec.constraint, CapConstraint::Any);
}

#[test]
fn mt4061_family_mismatch_explicit() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("fs", CapSpec::top(CapFamily::Fs))
        .expect("declare");
    match resolver.resolve_as("fs", &CapFamily::Net) {
        Err(CapResolutionError::FamilyMismatch {
            name,
            declared,
            expected,
        }) => {
            assert_eq!(name, "fs");
            assert_eq!(declared, CapFamily::Fs);
            assert_eq!(expected, CapFamily::Net);
        }
        other => panic!("expected FamilyMismatch, got {:?}", other),
    }
}

#[test]
fn mt4062_scope_violation_after_pop() {
    let mut resolver = CapResolver::new();
    resolver.push_scope();
    resolver
        .bind_in_scope("tmp", CapSpec::top(CapFamily::Fs))
        .expect("bind");
    // Inside frame: resolves.
    assert!(resolver.resolve("tmp").is_ok());
    // Pop frame, then resolve again: scope violation.
    resolver.pop_scope();
    match resolver.resolve("tmp") {
        Err(CapResolutionError::ScopeViolation {
            name,
            popped_at_depth,
        }) => {
            assert_eq!(name, "tmp");
            assert_eq!(popped_at_depth, 1);
        }
        other => panic!("expected ScopeViolation, got {:?}", other),
    }
}

#[test]
fn mt4063_redeclaration_module_level() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("fs", CapSpec::top(CapFamily::Fs))
        .expect("first");
    match resolver.declare("fs", CapSpec::top(CapFamily::Fs)) {
        Err(CapResolutionError::Redeclaration { name, frame_depth }) => {
            assert_eq!(name, "fs");
            assert_eq!(frame_depth, 0);
        }
        other => panic!("expected Redeclaration, got {:?}", other),
    }
}

#[test]
fn mt4063_redeclaration_within_frame() {
    let mut resolver = CapResolver::new();
    resolver.push_scope();
    resolver
        .bind_in_scope("fs", CapSpec::top(CapFamily::Fs))
        .expect("first");
    match resolver.bind_in_scope("fs", CapSpec::top(CapFamily::Fs)) {
        Err(CapResolutionError::Redeclaration { name, frame_depth }) => {
            assert_eq!(name, "fs");
            assert_eq!(frame_depth, 1);
        }
        other => panic!("expected Redeclaration, got {:?}", other),
    }
}

#[test]
fn mt4064_unknown_method_on_fs() {
    let resolver = CapResolver::new();
    match resolver.check_method(&CapFamily::Fs, "explode") {
        Err(CapResolutionError::UnknownMethod {
            family,
            method,
            available,
        }) => {
            assert_eq!(family, CapFamily::Fs);
            assert_eq!(method, "explode");
            assert!(available.contains(&"ro".to_string()));
            assert!(available.contains(&"path".to_string()));
        }
        other => panic!("expected UnknownMethod, got {:?}", other),
    }
}

#[test]
fn mt4064_unknown_method_on_net() {
    let resolver = CapResolver::new();
    let err = resolver
        .check_method(&CapFamily::Net, "ro")
        .expect_err("Net.ro is not a method");
    assert!(matches!(err, CapResolutionError::UnknownMethod { .. }));
}

#[test]
fn mt4064_known_method_resolves() {
    let resolver = CapResolver::new();
    assert_eq!(resolver.check_method(&CapFamily::Fs, "ro").unwrap(), "ro");
    assert_eq!(
        resolver.check_method(&CapFamily::Net, "host").unwrap(),
        "host"
    );
}

#[test]
fn mt4065_invalid_constraint_readonly_on_net() {
    let resolver = CapResolver::new();
    match resolver.check_narrowing(&CapFamily::Net, "host", &CapConstraint::ReadOnly) {
        Err(CapResolutionError::InvalidConstraint { family, method, .. }) => {
            assert_eq!(family, CapFamily::Net);
            assert_eq!(method, "host");
        }
        other => panic!("expected InvalidConstraint, got {:?}", other),
    }
}

#[test]
fn mt4065_invalid_constraint_empty_host_on_net() {
    let resolver = CapResolver::new();
    match resolver.check_narrowing(&CapFamily::Net, "host", &CapConstraint::Host(vec![])) {
        Err(CapResolutionError::InvalidConstraint { .. }) => {}
        other => panic!("expected InvalidConstraint for empty Host, got {:?}", other),
    }
}

#[test]
fn mt4065_valid_constraint_readonly_on_fs() {
    let resolver = CapResolver::new();
    assert!(resolver
        .check_narrowing(&CapFamily::Fs, "ro", &CapConstraint::ReadOnly)
        .is_ok());
}

#[test]
fn mt4065_valid_constraint_path_on_fs() {
    let resolver = CapResolver::new();
    assert!(resolver
        .check_narrowing(&CapFamily::Fs, "path", &CapConstraint::Path("/data".into()))
        .is_ok());
}

#[test]
fn scope_stack_inside_out_resolution() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("fs", CapSpec::top(CapFamily::Fs))
        .expect("module fs");
    resolver.push_scope();
    resolver
        .bind_in_scope("fs", CapSpec::new(CapFamily::Fs, CapConstraint::ReadOnly))
        .expect("inner fs shadow");
    // Inside the frame: the inner ReadOnly binding wins.
    let inner = resolver.resolve("fs").expect("inner");
    assert_eq!(inner.constraint, CapConstraint::ReadOnly);
    resolver.pop_scope();
    // After pop: the outer module fs is back.
    let outer = resolver.resolve("fs").expect("outer");
    assert_eq!(outer.constraint, CapConstraint::Any);
}

#[test]
fn visible_names_dedup_across_frames() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("net", CapSpec::top(CapFamily::Net))
        .unwrap();
    resolver.push_scope();
    resolver
        .bind_in_scope("fs", CapSpec::top(CapFamily::Fs))
        .unwrap();
    resolver.push_scope();
    resolver
        .bind_in_scope("clock", CapSpec::top(CapFamily::Clock))
        .unwrap();
    let names = resolver.visible_names();
    assert!(names.contains(&"fs".to_string()));
    assert!(names.contains(&"net".to_string()));
    assert!(names.contains(&"clock".to_string()));
}

#[test]
fn is_known_walks_full_chain() {
    let mut resolver = CapResolver::new();
    resolver
        .declare("net", CapSpec::top(CapFamily::Net))
        .unwrap();
    resolver.push_scope();
    resolver
        .bind_in_scope("fs", CapSpec::top(CapFamily::Fs))
        .unwrap();
    assert!(resolver.is_known("net"));
    assert!(resolver.is_known("fs"));
    assert!(!resolver.is_known("clock"));
}

#[test]
fn diag_builders_produce_correct_codes() {
    // Each builder maps to its stable code.
    use mty_diagnostics::codes::*;
    use mty_hir::SourceSpan;
    let span = SourceSpan { start: 0, end: 0 };
    let d = mty_types::diag::cap_name_unbound("fs", &span);
    assert_eq!(d.code, CAP_NAME_UNBOUND);

    let d = mty_types::diag::cap_family_mismatch("fs", &CapFamily::Fs, &CapFamily::Net, &span);
    assert_eq!(d.code, CAP_FAMILY_MISMATCH);

    let d = mty_types::diag::cap_scope_violation("fs", 1, &span);
    assert_eq!(d.code, CAP_SCOPE_VIOLATION);

    let d = mty_types::diag::cap_redeclaration("fs", 0, &span);
    assert_eq!(d.code, CAP_REDECLARATION);

    let d = mty_types::diag::cap_method_unknown(&CapFamily::Fs, "explode", &["ro".into()], &span);
    assert_eq!(d.code, CAP_METHOD_UNKNOWN);

    let d =
        mty_types::diag::cap_constraint_invalid(&CapFamily::Net, "host", "empty host list", &span);
    assert_eq!(d.code, CAP_CONSTRAINT_INVALID);
}
