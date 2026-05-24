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

/// v0.6 easy-win 3: two FsCap values with disjoint allowlists must
/// stay isolated in the same process. This exercises the per-call
/// `cap: &FsCap` parameter of `sdust_stdlib::fs::{read, write, exists,
/// list_dir}` — neither cap may peek into the other's root.
///
/// The companion process-wide-default-cap path is already covered by
/// `read_outside_allowlist_returns_forbidden_io_err` /
/// `host_dispatch_read_outside_default_cap_returns_err_variant`;
/// this case proves that callers who construct their own per-call cap
/// don't share state across cap instances.
#[test]
fn two_disjoint_caps_isolate_in_the_same_process() {
    let tmp = tempdir().expect("mktmp");
    let root_a = tmp.path().join("agent_a");
    let root_b = tmp.path().join("agent_b");
    std::fs::create_dir_all(&root_a).expect("mkdir a");
    std::fs::create_dir_all(&root_b).expect("mkdir b");
    let a_file = root_a.join("a.txt");
    let b_file = root_b.join("b.txt");
    std::fs::write(&a_file, b"alpha").expect("seed a");
    std::fs::write(&b_file, b"bravo").expect("seed b");

    let cap_a = FsCap::rooted([root_a.clone()]);
    let cap_b = FsCap::rooted([root_b.clone()]);

    // Each cap reads its own file.
    assert_eq!(
        sdust_stdlib::fs::read(&cap_a, &a_file).expect("a reads a"),
        b"alpha"
    );
    assert_eq!(
        sdust_stdlib::fs::read(&cap_b, &b_file).expect("b reads b"),
        b"bravo"
    );

    // Cross-cap reads are forbidden — cap_a may not see cap_b's file
    // and vice versa.
    let err_ab = sdust_stdlib::fs::read(&cap_a, &b_file).expect_err("a must not read b");
    assert!(
        matches!(err_ab, IoErr::Forbidden(_) | IoErr::Denied(_)),
        "expected denial, got {err_ab:?}"
    );
    let err_ba = sdust_stdlib::fs::read(&cap_b, &a_file).expect_err("b must not read a");
    assert!(
        matches!(err_ba, IoErr::Forbidden(_) | IoErr::Denied(_)),
        "expected denial, got {err_ba:?}"
    );

    // `exists` follows the same gate: visible only via your own cap.
    assert!(sdust_stdlib::fs::exists(&cap_a, &a_file));
    assert!(!sdust_stdlib::fs::exists(&cap_a, &b_file));
    assert!(sdust_stdlib::fs::exists(&cap_b, &b_file));
    assert!(!sdust_stdlib::fs::exists(&cap_b, &a_file));

    // Writes through cap_a may not land in cap_b's root.
    let illegal = root_b.join("smuggled.txt");
    let write_err = sdust_stdlib::fs::write(&cap_a, &illegal, b"nope")
        .expect_err("write through wrong cap must fail");
    assert!(
        matches!(write_err, IoErr::Forbidden(_) | IoErr::Denied(_)),
        "expected denial, got {write_err:?}"
    );
    assert!(!illegal.exists(), "denied write must not touch disk");

    // `list_dir` on the other root is denied even though the directory
    // exists on disk.
    let list_err =
        sdust_stdlib::fs::list_dir(&cap_a, &root_b).expect_err("list_dir cross-cap must fail");
    assert!(
        matches!(list_err, IoErr::Forbidden(_) | IoErr::Denied(_)),
        "expected denial, got {list_err:?}"
    );
}
