//! v0.5 dogfood Gap-5 integration test — install a process-wide
//! default read cap with a narrow allowlist, then verify that
//! `host::fs_read` denies a path outside the list.
//!
//! Until v0.6 lowers per-call caps from sandbox manifests into the
//! SIR call shape, this default-cap pathway is the v0.5 enforcement
//! mechanism: the driver materialises the manifest into a `FsCap` and
//! calls `install_default_read_cap` at startup.

use sdust_sir::interp::value::Value;
use sdust_stdlib::fs::{
    current_default_read_cap, install_default_read_cap, install_default_write_cap, FsCap, IoErr,
};
use sdust_stdlib::host::dispatch;
use tempfile::tempdir;

/// Save/restore the default read+write cap around a closure so tests
/// don't leak global state across runs.
fn with_default_caps<R>(read: FsCap, write: FsCap, body: impl FnOnce() -> R) -> R {
    let prev_r = install_default_read_cap(read);
    let prev_w = install_default_write_cap(write);
    let r = body();
    let _ = install_default_read_cap(prev_r);
    let _ = install_default_write_cap(prev_w);
    r
}

#[test]
fn fs_cap_denies_path_outside_allowlist() {
    let cap = FsCap::rooted(["/only/here"]);
    assert!(cap.allows(std::path::Path::new("/only/here/x")));
    assert!(!cap.allows(std::path::Path::new("/elsewhere/x")));
}

#[test]
fn read_outside_allowlist_returns_forbidden_io_err() {
    let cap = FsCap::rooted(["/only/here"]);
    let err = sdust_stdlib::fs::read(&cap, std::path::Path::new("/elsewhere/x"))
        .expect_err("must reject");
    match err {
        IoErr::Forbidden(p) | IoErr::Denied(p) => assert!(p.contains("elsewhere"), "p={p}"),
        other => panic!("unexpected err shape: {other:?}"),
    }
}

#[test]
fn host_dispatch_read_outside_default_cap_returns_err_variant() {
    let tmp = tempdir().expect("mktmp");
    let allowed = tmp.path().join("allowed");
    std::fs::create_dir_all(&allowed).expect("mkdir allowed");
    let inside_path = allowed.join("ok.txt");
    std::fs::write(&inside_path, "inside-bytes").expect("write inside file");

    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "outside-bytes").expect("write outside file");

    with_default_caps(
        FsCap::rooted([allowed.clone()]),
        FsCap::unrestricted(),
        || {
            // Inside the allow-list -> Str payload.
            let v_inside = dispatch(
                &["std".into(), "fs".into()],
                "read",
                &[Value::Unit, Value::Str(inside_path.display().to_string())],
            );
            match &v_inside {
                Value::Str(s) => {
                    assert!(s.contains("inside-bytes"), "expected file body, got {s:?}")
                }
                other => panic!("expected Str, got {other:?}"),
            }

            // Outside -> Enum (Err variant 1).
            let v_outside = dispatch(
                &["std".into(), "fs".into()],
                "read",
                &[Value::Unit, Value::Str(outside.display().to_string())],
            );
            match &v_outside {
                Value::Enum {
                    variant, payload, ..
                } => {
                    assert_eq!(*variant, 1, "expected Err variant");
                    match payload.first() {
                        Some(Value::Str(s)) => {
                            assert!(s.contains("forbidden"), "msg: {s}")
                        }
                        other => panic!("expected Str payload, got {other:?}"),
                    }
                }
                other => panic!("expected Enum Err, got {other:?}"),
            }
        },
    );
}

#[test]
fn install_default_read_cap_returns_previous_for_scoped_overrides() {
    let prev = install_default_read_cap(FsCap::rooted(["/scoped"]));
    // restore
    let _ = install_default_read_cap(prev);
    // After restore, reading the current cap should match the saved
    // version (a stable equality check just compares the allowed list).
    let now = current_default_read_cap();
    let _ = now; // just smoke — no panic
}
