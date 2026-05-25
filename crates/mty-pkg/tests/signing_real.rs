//! v0.10 cleanup — sanity tests for the mode-aware signing API.
//!
//! These tests run with **default features** (no `sigstore-real`)
//! and verify the public-API shape that `mty pkg publish` relies on:
//!
//! * [`SigningMode`] parses common config-string spellings.
//! * `sign_bundle_with_mode(_, SigningMode::Keyless)` returns
//!   successfully and degrades to `Stub` (the feature flag is off
//!   in default builds).
//! * `sign_bundle_with_mode(_, SigningMode::Off)` writes no
//!   sidecars.
//!
//! The real-network round-trip (Fulcio cert exchange + Rekor entry
//! upload + verify against the public log) is `#[ignore]`d — to
//! exercise it, build with `--features sigstore-real` on a Linux
//! runner (Windows requires NASM for `aws-lc-rs`) and run:
//!
//! ```bash
//! cargo test -p mty-pkg --features sigstore-real \
//!   --test signing_real -- --ignored keyless_round_trip
//! ```

use mty_pkg::publish::bundle;
use mty_pkg::signing::{self, SigningMode};

fn write_pkg(dir: &std::path::Path, name: &str, version: &str) {
    std::fs::write(
        dir.join("mighty.toml"),
        format!(
            r#"
[package]
name = "{name}"
version = "{version}"
edition = "2026"
"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("main.mty"), b"fn main() {}").unwrap();
}

#[test]
fn keyless_request_degrades_gracefully_without_feature_flag() {
    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "feature-flag-fallback", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");

    // Requesting keyless mode on a default build should succeed
    // (not error) and the returned `mode` should report the
    // actual mode used. Without the feature flag, that's Stub.
    let signed =
        signing::sign_bundle_with_mode(&outcome, SigningMode::Keyless).expect("sign-keyless");

    // The publish command relies on this contract — if it ever
    // erroed on a default build, every `mty pkg publish` on a
    // Windows host (where the sigstore dep graph requires NASM)
    // would break.
    if cfg!(feature = "sigstore-real") {
        // When the feature IS enabled, we may or may not get
        // real keyless depending on ambient OIDC. The contract
        // only guarantees no error.
    } else {
        assert_eq!(
            signed.mode,
            SigningMode::Stub,
            "default build must degrade keyless → stub"
        );
        assert!(signed.sig_path.exists());
        assert!(signed.envelope_path.exists());
    }
}

#[test]
fn off_mode_skips_sidecar_creation() {
    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "off-mode", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");
    let signed = signing::sign_bundle_with_mode(&outcome, SigningMode::Off).expect("sign-off");
    assert_eq!(signed.mode, SigningMode::Off);
    assert!(!signed.sig_path.exists(), "off mode must not write .sig");
    assert!(
        !signed.envelope_path.exists(),
        "off mode must not write .bundle"
    );
    // The bundle path is still returned for convenience.
    assert!(signed.bundle_path.exists(), "bundle itself is preserved");
}

#[test]
fn parse_recognises_known_aliases_and_defaults_to_stub() {
    for s in &["stub", "STUB", "default", "unknown-mode"] {
        assert_eq!(
            SigningMode::parse(Some(s)),
            SigningMode::Stub,
            "input `{s}` should parse as Stub"
        );
    }
    for s in &["keyless", "Keyless", "real", "sigstore"] {
        assert_eq!(
            SigningMode::parse(Some(s)),
            SigningMode::Keyless,
            "input `{s}` should parse as Keyless"
        );
    }
    for s in &["off", "OFF", "none", "skip"] {
        assert_eq!(
            SigningMode::parse(Some(s)),
            SigningMode::Off,
            "input `{s}` should parse as Off"
        );
    }
    assert_eq!(SigningMode::parse(None), SigningMode::Stub);
}

/// Network test — only runs with `--features sigstore-real
/// --ignored`. Exercises the full Fulcio cert exchange + Rekor
/// upload + verify-against-the-public-log loop using ambient OIDC.
///
/// Requires:
///   * Linux host (Windows lacks NASM out of the box).
///   * Ambient OIDC identity (GitHub Actions runner env vars, or a
///     local interactive flow if the sigstore crate is configured
///     for one).
///   * Outbound network to `fulcio.sigstore.dev` and
///     `rekor.sigstore.dev`.
#[test]
#[ignore]
#[cfg(feature = "sigstore-real")]
fn keyless_round_trip_via_fulcio_and_rekor() {
    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "keyless-rt", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");

    let signed = signing::sign_bundle_with_mode(&outcome, SigningMode::Keyless)
        .expect("sign keyless (requires ambient OIDC)");
    assert_eq!(
        signed.mode,
        SigningMode::Keyless,
        "real keyless should succeed when OIDC is available; got {:?}",
        signed.mode
    );
    assert!(signed.sig_path.exists());
    assert!(signed.envelope_path.exists());

    signing::verify_bundle(&outcome.bundle_path).expect("verify keyless bundle");
}
