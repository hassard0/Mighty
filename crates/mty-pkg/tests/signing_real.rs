//! v0.10 cleanup → v0.18 sigstore-real wiring — public-API tests
//! for the mode-aware signing API.
//!
//! These tests run with **default features** (no `sigstore-real`)
//! and verify the public-API contract that `mty pkg publish` and
//! `mty pkg fetch` rely on:
//!
//! * [`SigningMode`] parses common config-string spellings.
//! * `sign_bundle_with_mode(_, SigningMode::Keyless)` returns
//!   successfully and degrades to `Stub` on default builds (the
//!   feature flag is off).
//! * `sign_bundle_with_mode(_, SigningMode::Off)` writes no
//!   sidecars.
//! * The keyless envelope shape (with embedded sigstoreBundle JSON)
//!   round-trips through `verify_bundle`, even when the envelope
//!   was generated externally (mocked here as a synthetic envelope
//!   with the right shape).
//! * A keyless envelope with a tampered embedded digest fails
//!   `verify_bundle` cleanly — the v0.18 verify path cross-checks
//!   the sigstoreBundle's messageDigest against the recomputed
//!   bundle hash.
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

/// v0.18 — verify_bundle accepts a synthetic keyless envelope that
/// embeds the standard sigstoreBundle JSON, provided the embedded
/// digest matches the bundle bytes.
///
/// This exercises the structural verify path without requiring the
/// `sigstore-real` feature: the test forges a keyless envelope by
/// hand using the same shape the real Fulcio+Rekor flow would
/// produce. Future PRs that change the envelope layout will trip
/// this test.
#[test]
fn verify_bundle_recognizes_real_signed_envelope() {
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "fake-keyless", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");

    let bundle_bytes = std::fs::read(&outcome.bundle_path).unwrap();
    let sha = Sha256::digest(&bundle_bytes);
    let sha_hex = hex::encode(sha);
    // The sigstoreBundle's messageDigest.digest is base64-encoded
    // raw SHA-256 bytes — same convention as the upstream sigstore
    // Bundle JSON.
    let sha_b64 = base64_std(&sha);

    // Forge a keyless envelope that mirrors what the real Fulcio +
    // Rekor flow would emit. The top-level shape is back-compat
    // with stub envelopes; the `sigstoreBundle` block is the v0.18
    // addition.
    let fake_envelope = format!(
        r#"{{
  "mediaType": "application/vnd.mty.bundle.v0.10+json",
  "messageSignature": {{
    "messageDigest": {{ "algorithm": "SHA2_256", "digest": "{sha_hex}" }},
    "signature": "deadbeef"
  }},
  "verificationMaterial": {{
    "identity": "fake-thumbprint",
    "mode": "keyless",
    "sigstoreBundle": {{
      "mediaType": "application/vnd.dev.sigstore.bundle+json;version=0.2",
      "verificationMaterial": {{
        "x509CertificateChain": {{
          "certificates": [
            {{ "rawBytes": "Zm9v" }}
          ]
        }},
        "tlogEntries": [
          {{
            "logIndex": "1234567",
            "logId": {{ "keyId": "AAAA" }},
            "kindVersion": {{ "kind": "hashedrekord", "version": "0.0.1" }},
            "integratedTime": "1700000000",
            "canonicalizedBody": "Zm9v"
          }}
        ]
      }},
      "messageSignature": {{
        "messageDigest": {{ "algorithm": "SHA2_256", "digest": "{sha_b64}" }},
        "signature": "Zm9v"
      }}
    }}
  }}
}}
"#
    );
    let envelope_path = with_bundle_suffix(&outcome.bundle_path, ".bundle");
    std::fs::write(&envelope_path, &fake_envelope).unwrap();

    // Forge the matching `.sig` file. The v0.18 verify path
    // requires the top-level digest + signature to match the
    // envelope JSON; we satisfy that with `deadbeef`.
    let sig_path = with_bundle_suffix(&outcome.bundle_path, ".sig");
    let sig_text = format!(
        "mty-sig/1\nbundle-sha256:{sha_hex}\nidentity:fake-thumbprint\nsigned-at:1700000000\nsig:deadbeef\n"
    );
    std::fs::write(&sig_path, sig_text).unwrap();

    signing::verify_bundle(&outcome.bundle_path).expect("verify keyless envelope");
}

/// v0.18 — verify_bundle rejects a keyless envelope whose embedded
/// sigstoreBundle digest does NOT match the recomputed bundle hash.
/// Catches the case where an attacker swaps the bundle bytes but
/// leaves the envelope intact.
#[test]
fn verify_rejects_modified_payload_real() {
    use sha2::{Digest, Sha256};

    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "fake-keyless-tamper", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");

    let bundle_bytes = std::fs::read(&outcome.bundle_path).unwrap();
    let sha_hex = hex::encode(Sha256::digest(&bundle_bytes));

    // Embedded digest is a base64 of a *different* hash than the
    // top-level one. The top-level cross-check passes (digest +
    // signature appear in the JSON), but the embedded check catches
    // the mismatch.
    let wrong_digest = Sha256::digest(b"some-other-bytes");
    let wrong_b64 = base64_std(&wrong_digest);

    let fake_envelope = format!(
        r#"{{
  "mediaType": "application/vnd.mty.bundle.v0.10+json",
  "messageSignature": {{
    "messageDigest": {{ "algorithm": "SHA2_256", "digest": "{sha_hex}" }},
    "signature": "deadbeef"
  }},
  "verificationMaterial": {{
    "identity": "fake-thumbprint",
    "mode": "keyless",
    "sigstoreBundle": {{
      "messageSignature": {{
        "messageDigest": {{ "algorithm": "SHA2_256", "digest": "{wrong_b64}" }},
        "signature": "Zm9v"
      }},
      "verificationMaterial": {{
        "x509CertificateChain": {{ "certificates": [] }},
        "tlogEntries": []
      }}
    }}
  }}
}}
"#
    );
    let envelope_path = with_bundle_suffix(&outcome.bundle_path, ".bundle");
    std::fs::write(&envelope_path, &fake_envelope).unwrap();
    let sig_path = with_bundle_suffix(&outcome.bundle_path, ".sig");
    let sig_text = format!(
        "mty-sig/1\nbundle-sha256:{sha_hex}\nidentity:fake-thumbprint\nsigned-at:1700000000\nsig:deadbeef\n"
    );
    std::fs::write(&sig_path, sig_text).unwrap();

    let err = signing::verify_bundle(&outcome.bundle_path)
        .expect_err("verify should fail on tampered embedded digest");
    let msg = format!("{err}");
    assert!(
        msg.contains("embedded sigstore bundle digest does not match"),
        "unexpected error: {msg}"
    );
}

/// v0.18 — stub-mode bundles still verify under the v0.18 verify
/// path. Tests back-compat with envelopes produced before the
/// `sigstoreBundle` field was added (i.e. plain stub envelopes
/// that lack the embedded bundle JSON).
#[test]
fn verify_falls_back_to_stub_when_real_not_present() {
    let dir = tempfile::tempdir().unwrap();
    write_pkg(dir.path(), "stub-back-compat", "0.1.0");
    let outcome = bundle(dir.path()).expect("bundle");

    let signed = signing::sign_bundle_with_mode(&outcome, SigningMode::Stub).expect("sign stub");
    assert_eq!(signed.mode, SigningMode::Stub);
    // The verify path must accept a stub envelope (no embedded
    // sigstoreBundle) just like it did in v0.10–v0.17.
    signing::verify_bundle(&outcome.bundle_path).expect("verify stub-only envelope");
}

// ---- test helpers ----

fn with_bundle_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

fn base64_std(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8 | (bytes[i + 2] as u32);
        out.push(T[((n >> 18) & 0x3f) as usize] as char);
        out.push(T[((n >> 12) & 0x3f) as usize] as char);
        out.push(T[((n >> 6) & 0x3f) as usize] as char);
        out.push(T[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(T[((n >> 18) & 0x3f) as usize] as char);
        out.push(T[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8;
        out.push(T[((n >> 18) & 0x3f) as usize] as char);
        out.push(T[((n >> 12) & 0x3f) as usize] as char);
        out.push(T[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
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
