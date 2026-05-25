//! `mty-pkg` — bundle signing.
//!
//! v0.9 RC-prep ships **stub** sigstore-style signing: every
//! `mty pkg publish` produces two sidecars alongside the `.tar.gz`:
//!
//! - `<bundle>.sig` — a deterministic envelope containing the
//!   SHA-256 of the bundle plus a self-signed identity hash. The
//!   verifier checks the envelope against the bundle bytes.
//! - `<bundle>.bundle` — a sigstore-compatible JSON document with the
//!   artifact digest, identity claim, and signing timestamp. Real
//!   OIDC + Rekor log uploads come in v0.10 (see KNOWN_ISSUES.md).
//!
//! The on-disk format is intentionally close to a real sigstore
//! bundle so we can later swap the stub identity for a Fulcio cert
//! without breaking downstream verifiers.
//!
//! ### Why a stub?
//!
//! The `sigstore` Rust crate pulls in a large dep graph (tonic,
//! openssl-sys on some hosts, OpenAPI clients for Fulcio/Rekor) that
//! would noticeably slow the v0.9 build + complicate CI on Windows.
//! For the RC we ship the *shape* — a determinate signing pipeline
//! integrated into `mty pkg publish`, with a passing round-trip test
//! — and gate real keyless signing behind a v0.10 `sigstore-real`
//! feature flag.
//!
//! ### Envelope shape
//!
//! `.sig` (text, one line per record, deterministic order):
//!
//! ```text
//! mty-sig/1
//! bundle-sha256:<hex>
//! identity:<hex>
//! signed-at:0
//! sig:<hex>
//! ```
//!
//! `.bundle` (JSON, pretty-printed, deterministic key order):
//!
//! ```json
//! {
//!   "mediaType": "application/vnd.mty.bundle.v0.9+json",
//!   "messageSignature": {
//!     "messageDigest": { "algorithm": "SHA2_256", "digest": "<hex>" },
//!     "signature": "<hex>"
//!   },
//!   "verificationMaterial": {
//!     "identity": "<hex>",
//!     "mode": "stub"
//!   }
//! }
//! ```

use crate::publish::PublishOutcome;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Magic header for the `.sig` text format.
pub const SIG_FORMAT_VERSION: &str = "mty-sig/1";

/// Media type recorded in the `.bundle` envelope. The v-suffix is
/// bumped when we switch to a real sigstore payload.
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.mty.bundle.v0.9+json";

/// Result of [`sign_bundle`] — the two new sidecars.
#[derive(Debug, Clone)]
pub struct SignedBundle {
    /// `<bundle>.tar.gz` — unchanged; included for convenience.
    pub bundle_path: PathBuf,
    /// `<bundle>.tar.gz.sig`
    pub sig_path: PathBuf,
    /// `<bundle>.tar.gz.bundle`
    pub envelope_path: PathBuf,
    /// SHA-256 of the bundle bytes, hex-encoded.
    pub bundle_sha256_hex: String,
    /// Deterministic identity hash derived from the package name +
    /// version. In v0.10 this becomes a real Fulcio cert thumbprint.
    pub identity_hex: String,
    /// The signature value (hex-encoded).
    pub signature_hex: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bundle missing at {0}")]
    BundleMissing(PathBuf),
    #[error("parse signature: {0}")]
    Parse(String),
    #[error("verification failed: {0}")]
    Verify(String),
}

/// Sign the bundle produced by [`crate::publish::bundle`].
///
/// Writes `.sig` + `.bundle` alongside the `.tar.gz`. Returns the
/// paths + computed digests for further use (e.g. uploading to a
/// transparency log in v0.10).
pub fn sign_bundle(outcome: &PublishOutcome) -> Result<SignedBundle, SigningError> {
    if !outcome.bundle_path.exists() {
        return Err(SigningError::BundleMissing(outcome.bundle_path.clone()));
    }
    let bundle_bytes = std::fs::read(&outcome.bundle_path)?;
    let bundle_sha256_hex = hex::encode(Sha256::digest(&bundle_bytes));

    // Stub identity = sha256("mty-stub-id:<pkg>:<version>"). Real
    // signing replaces this with a Fulcio short-lived cert sourced
    // from the GitHub Actions OIDC token (see v0.10 plan).
    let identity_hex = stub_identity(&outcome.package_name, &outcome.package_version);

    // Stub signature = sha256(bundle_sha || identity). A real
    // signature would be ECDSA over the same input.
    let signature_hex = stub_signature(&bundle_sha256_hex, &identity_hex);

    let sig_path = with_suffix(&outcome.bundle_path, ".sig");
    let envelope_path = with_suffix(&outcome.bundle_path, ".bundle");

    let sig_text = format_sig_text(&bundle_sha256_hex, &identity_hex, &signature_hex);
    std::fs::write(&sig_path, sig_text.as_bytes())?;

    let envelope_text = format_envelope_json(&bundle_sha256_hex, &identity_hex, &signature_hex);
    std::fs::write(&envelope_path, envelope_text.as_bytes())?;

    Ok(SignedBundle {
        bundle_path: outcome.bundle_path.clone(),
        sig_path,
        envelope_path,
        bundle_sha256_hex,
        identity_hex,
        signature_hex,
    })
}

/// Verify a signed bundle: re-hashes the bundle, parses the `.sig`
/// envelope, and confirms the recorded digest + recomputed signature
/// agree with the bytes on disk. Returns `Ok(())` on success.
pub fn verify_bundle(bundle_path: &Path) -> Result<(), SigningError> {
    if !bundle_path.exists() {
        return Err(SigningError::BundleMissing(bundle_path.to_path_buf()));
    }
    let sig_path = with_suffix(bundle_path, ".sig");
    let envelope_path = with_suffix(bundle_path, ".bundle");

    let bundle_bytes = std::fs::read(bundle_path)?;
    let actual_sha = hex::encode(Sha256::digest(&bundle_bytes));

    let sig_text = std::fs::read_to_string(&sig_path)?;
    let parsed = parse_sig_text(&sig_text)?;

    if parsed.bundle_sha256_hex != actual_sha {
        return Err(SigningError::Verify(format!(
            "bundle sha256 mismatch: sig says {}, actual {}",
            parsed.bundle_sha256_hex, actual_sha
        )));
    }
    let expected_sig = stub_signature(&parsed.bundle_sha256_hex, &parsed.identity_hex);
    if expected_sig != parsed.signature_hex {
        return Err(SigningError::Verify(format!(
            "signature mismatch for identity {}",
            parsed.identity_hex
        )));
    }

    // Cross-check the JSON envelope agrees with the sig.
    if envelope_path.exists() {
        let envelope_text = std::fs::read_to_string(&envelope_path)?;
        if !envelope_text.contains(&parsed.bundle_sha256_hex) {
            return Err(SigningError::Verify(
                "envelope JSON does not record the same digest".into(),
            ));
        }
        if !envelope_text.contains(&parsed.signature_hex) {
            return Err(SigningError::Verify(
                "envelope JSON does not record the same signature".into(),
            ));
        }
    }
    Ok(())
}

// ============================================================
// Internals
// ============================================================

#[derive(Debug)]
struct ParsedSig {
    bundle_sha256_hex: String,
    identity_hex: String,
    #[allow(dead_code)]
    signed_at: i64,
    signature_hex: String,
}

fn parse_sig_text(text: &str) -> Result<ParsedSig, SigningError> {
    let mut bundle_sha256_hex = None;
    let mut identity_hex = None;
    let mut signed_at: Option<i64> = None;
    let mut signature_hex = None;
    let mut saw_header = false;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == SIG_FORMAT_VERSION {
            saw_header = true;
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(SigningError::Parse(format!("malformed line: `{line}`")));
        };
        let v = v.trim();
        match k.trim() {
            "bundle-sha256" => bundle_sha256_hex = Some(v.to_string()),
            "identity" => identity_hex = Some(v.to_string()),
            "signed-at" => {
                signed_at = Some(
                    v.parse()
                        .map_err(|e| SigningError::Parse(format!("signed-at: {e}")))?,
                );
            }
            "sig" => signature_hex = Some(v.to_string()),
            other => return Err(SigningError::Parse(format!("unknown key `{other}`"))),
        }
    }
    if !saw_header {
        return Err(SigningError::Parse("missing format header".into()));
    }
    Ok(ParsedSig {
        bundle_sha256_hex: bundle_sha256_hex
            .ok_or_else(|| SigningError::Parse("missing bundle-sha256".into()))?,
        identity_hex: identity_hex.ok_or_else(|| SigningError::Parse("missing identity".into()))?,
        signed_at: signed_at.unwrap_or(0),
        signature_hex: signature_hex.ok_or_else(|| SigningError::Parse("missing sig".into()))?,
    })
}

fn format_sig_text(bundle_sha: &str, identity: &str, sig: &str) -> String {
    // signed-at is pinned to 0 for determinism in v0.9. Real signing
    // uses the OIDC token's `iat` claim.
    format!(
        "{SIG_FORMAT_VERSION}\nbundle-sha256:{bundle_sha}\nidentity:{identity}\nsigned-at:0\nsig:{sig}\n"
    )
}

fn format_envelope_json(bundle_sha: &str, identity: &str, sig: &str) -> String {
    // Hand-written JSON keeps key order deterministic (serde_json's
    // BTreeMap-of-Value collapses everything to default order, which
    // is what we want — but writing it manually documents the shape).
    format!(
        "{{\n  \"mediaType\": \"{BUNDLE_MEDIA_TYPE}\",\n  \"messageSignature\": {{\n    \"messageDigest\": {{ \"algorithm\": \"SHA2_256\", \"digest\": \"{bundle_sha}\" }},\n    \"signature\": \"{sig}\"\n  }},\n  \"verificationMaterial\": {{\n    \"identity\": \"{identity}\",\n    \"mode\": \"stub\"\n  }}\n}}\n"
    )
}

fn stub_identity(pkg_name: &str, pkg_version: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"mty-stub-id:");
    h.update(pkg_name.as_bytes());
    h.update(b":");
    h.update(pkg_version.as_bytes());
    hex::encode(h.finalize())
}

fn stub_signature(bundle_sha_hex: &str, identity_hex: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"mty-stub-sig:");
    h.update(bundle_sha_hex.as_bytes());
    h.update(b":");
    h.update(identity_hex.as_bytes());
    hex::encode(h.finalize())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publish::bundle;

    fn write_pkg(dir: &Path, name: &str, version: &str) {
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
    fn sign_then_verify_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        let signed = sign_bundle(&outcome).expect("sign");

        assert!(signed.sig_path.exists(), "missing .sig");
        assert!(signed.envelope_path.exists(), "missing .bundle");
        assert_eq!(signed.bundle_sha256_hex.len(), 64);

        verify_bundle(&outcome.bundle_path).expect("verify");
    }

    #[test]
    fn signing_is_deterministic_for_same_input() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        let s1 = sign_bundle(&outcome).expect("sign-1");
        let sig_text_1 = std::fs::read_to_string(&s1.sig_path).unwrap();
        let env_text_1 = std::fs::read_to_string(&s1.envelope_path).unwrap();

        // Re-sign and confirm bytes are identical.
        let s2 = sign_bundle(&outcome).expect("sign-2");
        let sig_text_2 = std::fs::read_to_string(&s2.sig_path).unwrap();
        let env_text_2 = std::fs::read_to_string(&s2.envelope_path).unwrap();

        assert_eq!(s1.bundle_sha256_hex, s2.bundle_sha256_hex);
        assert_eq!(s1.identity_hex, s2.identity_hex);
        assert_eq!(s1.signature_hex, s2.signature_hex);
        assert_eq!(sig_text_1, sig_text_2);
        assert_eq!(env_text_1, env_text_2);
    }

    #[test]
    fn tampered_bundle_fails_verify() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        sign_bundle(&outcome).expect("sign");

        // Flip a byte in the middle of the bundle.
        let mut bytes = std::fs::read(&outcome.bundle_path).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        std::fs::write(&outcome.bundle_path, &bytes).unwrap();

        let err = verify_bundle(&outcome.bundle_path).expect_err("verify should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("sha256 mismatch") || msg.contains("signature mismatch"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn tampered_signature_fails_verify() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        let signed = sign_bundle(&outcome).expect("sign");

        // Corrupt the signature line.
        let sig_text = std::fs::read_to_string(&signed.sig_path).unwrap();
        let corrupt = sig_text.replace(
            &signed.signature_hex,
            &"0".repeat(signed.signature_hex.len()),
        );
        std::fs::write(&signed.sig_path, corrupt).unwrap();

        let err = verify_bundle(&outcome.bundle_path).expect_err("verify should fail");
        assert!(format!("{err}").contains("signature mismatch"));
    }

    #[test]
    fn parse_round_trips_text_format() {
        let text = format_sig_text("aa", "bb", "cc");
        let parsed = parse_sig_text(&text).expect("parse");
        assert_eq!(parsed.bundle_sha256_hex, "aa");
        assert_eq!(parsed.identity_hex, "bb");
        assert_eq!(parsed.signature_hex, "cc");
    }

    #[test]
    fn envelope_json_contains_canonical_fields() {
        let json = format_envelope_json("aaa", "bbb", "ccc");
        assert!(json.contains("\"mediaType\""));
        assert!(json.contains(BUNDLE_MEDIA_TYPE));
        assert!(json.contains("\"SHA2_256\""));
        assert!(json.contains("\"aaa\""));
        assert!(json.contains("\"bbb\""));
        assert!(json.contains("\"ccc\""));
    }
}
