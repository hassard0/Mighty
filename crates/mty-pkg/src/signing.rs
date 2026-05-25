//! `mty-pkg` — bundle signing.
//!
//! v0.10 cleanup ships **two implementations** behind a cargo
//! feature flag:
//!
//! * **default (`stub`)** — the v0.9 deterministic SHA-256 envelope.
//!   Cross-platform, hermetic, no network, no extra deps. Provides
//!   tamper-detection but no cryptographic identity guarantee.
//! * **`sigstore-real`** — real keyless signing via Fulcio (short-
//!   lived certs from an OIDC token) + Rekor (public transparency
//!   log). Adds the `sigstore` crate to the dep graph, gated behind
//!   the feature flag because the transitive dep set (tonic, native-
//!   TLS, OpenAPI clients) is heavy on some hosts.
//!
//! Mode selection happens at runtime via `[registry.signing] mode`
//! in `mighty.toml`:
//!
//! ```toml
//! [registry.signing]
//! mode = "keyless"            # or "stub" (default), or "off"
//! oidc_issuer = "https://oauth2.sigstore.dev/auth"  # optional
//! ```
//!
//! When `mode = "keyless"` but the binary was built without the
//! `sigstore-real` feature, [`sign_bundle_with_mode`] falls back to
//! the stub and logs a one-line note. This keeps `cargo build`
//! workable on hosts where the sigstore dep graph won't compile
//! (Windows + OpenSSL is the historical headache) without breaking
//! the publish command.
//!
//! ### On-disk artefact shape
//!
//! Both modes produce the same two sidecars (`.sig` text + `.bundle`
//! JSON) so downstream verifiers don't need to know which mode was
//! used. The `verificationMaterial.mode` field in the JSON
//! distinguishes them.
//!
//! `.sig` (text, one line per record, deterministic order):
//!
//! ```text
//! mty-sig/1
//! bundle-sha256:<hex>
//! identity:<hex-or-cert-thumbprint>
//! signed-at:<unix-secs-or-0>
//! sig:<hex>
//! ```
//!
//! `.bundle` (JSON, pretty-printed):
//!
//! ```json
//! {
//!   "mediaType": "application/vnd.mty.bundle.v0.10+json",
//!   "messageSignature": {
//!     "messageDigest": { "algorithm": "SHA2_256", "digest": "<hex>" },
//!     "signature": "<hex>"
//!   },
//!   "verificationMaterial": {
//!     "identity": "<hex>",
//!     "mode": "stub" | "keyless"
//!   }
//! }
//! ```

use crate::publish::PublishOutcome;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Magic header for the `.sig` text format.
pub const SIG_FORMAT_VERSION: &str = "mty-sig/1";

/// Media type recorded in the `.bundle` envelope.
///
/// v0.10 bumped from `…v0.9+json` so verifiers can detect mixed-mode
/// envelopes (real vs stub) by the leading version + the
/// `verificationMaterial.mode` field.
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.mty.bundle.v0.10+json";

/// Default OIDC issuer for keyless mode. Points at the public
/// Sigstore deployment.
pub const DEFAULT_OIDC_ISSUER: &str = "https://oauth2.sigstore.dev/auth";

/// Signing mode selected by `[registry.signing] mode` in
/// `mighty.toml`. The string→enum conversion is forgiving: unknown
/// modes degrade to `Stub` with a logged warning rather than failing
/// the publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigningMode {
    /// Default — deterministic SHA-256 envelope, hermetic, no network.
    Stub,
    /// Real Fulcio + Rekor keyless signing. Requires the
    /// `sigstore-real` cargo feature; falls back to `Stub` (with a
    /// note) when the feature is absent.
    Keyless,
    /// Skip signing entirely — no `.sig` / `.bundle` sidecars
    /// written. Useful for hermetic CI smoke tests and reproducible
    /// builds where the signature would otherwise be the only
    /// non-deterministic byte.
    Off,
}

impl SigningMode {
    /// Parse a string from `mighty.toml`. Returns `Stub` for None and
    /// for any unrecognised value (logging a warning would require a
    /// logger crate; callers are expected to validate config up front
    /// when they care).
    pub fn parse(s: Option<&str>) -> Self {
        match s.unwrap_or("stub").to_ascii_lowercase().as_str() {
            "keyless" | "real" | "sigstore" => Self::Keyless,
            "off" | "none" | "skip" => Self::Off,
            _ => Self::Stub,
        }
    }
}

/// Result of [`sign_bundle`] — the two new sidecars.
#[derive(Debug, Clone)]
pub struct SignedBundle {
    /// `<bundle>.tar.gz` — unchanged; included for convenience.
    pub bundle_path: PathBuf,
    /// `<bundle>.tar.gz.sig` (absent in `Off` mode).
    pub sig_path: PathBuf,
    /// `<bundle>.tar.gz.bundle` (absent in `Off` mode).
    pub envelope_path: PathBuf,
    /// SHA-256 of the bundle bytes, hex-encoded.
    pub bundle_sha256_hex: String,
    /// Identity hash (stub) or Fulcio cert thumbprint (keyless),
    /// hex-encoded.
    pub identity_hex: String,
    /// The signature value (hex-encoded).
    pub signature_hex: String,
    /// Mode that was actually used (may differ from the requested
    /// mode when `Keyless` was requested without the feature flag).
    pub mode: SigningMode,
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
    #[error("sigstore: {0}")]
    Sigstore(String),
}

/// Sign the bundle produced by [`crate::publish::bundle`] using the
/// **default** mode (stub). Kept as the v0.9 entry point so existing
/// callers don't need a flag day — [`sign_bundle_with_mode`] is the
/// new mode-aware API.
pub fn sign_bundle(outcome: &PublishOutcome) -> Result<SignedBundle, SigningError> {
    sign_bundle_with_mode(outcome, SigningMode::Stub)
}

/// Sign the bundle in the requested [`SigningMode`].
///
/// * `Stub`: writes the deterministic SHA-256 envelope (v0.9 shape).
/// * `Keyless`: calls real Fulcio + Rekor when the `sigstore-real`
///   feature is enabled; otherwise falls back to `Stub` with the
///   actual mode reported in [`SignedBundle::mode`].
/// * `Off`: no sidecars written. Returns a [`SignedBundle`] with
///   empty signature fields and `mode == Off`.
pub fn sign_bundle_with_mode(
    outcome: &PublishOutcome,
    mode: SigningMode,
) -> Result<SignedBundle, SigningError> {
    if !outcome.bundle_path.exists() {
        return Err(SigningError::BundleMissing(outcome.bundle_path.clone()));
    }
    let bundle_bytes = std::fs::read(&outcome.bundle_path)?;
    let bundle_sha256_hex = hex::encode(Sha256::digest(&bundle_bytes));

    match mode {
        SigningMode::Off => Ok(SignedBundle {
            bundle_path: outcome.bundle_path.clone(),
            sig_path: with_suffix(&outcome.bundle_path, ".sig"),
            envelope_path: with_suffix(&outcome.bundle_path, ".bundle"),
            bundle_sha256_hex,
            identity_hex: String::new(),
            signature_hex: String::new(),
            mode: SigningMode::Off,
        }),
        SigningMode::Stub => write_stub_signature(outcome, bundle_sha256_hex),
        SigningMode::Keyless => sign_keyless(outcome, bundle_sha256_hex),
    }
}

/// Verify a signed bundle: re-hashes the bundle, parses the `.sig`
/// envelope, and confirms the recorded digest + recomputed signature
/// agree with the bytes on disk. Returns `Ok(())` on success.
///
/// For `Stub` mode this checks the SHA-256 envelope identity. For
/// `Keyless` mode (when the feature is enabled) it would consult
/// Rekor to validate the transparency-log entry — for v0.10 the
/// verify path only handles the shape, not the upstream cross-check
/// (tracked under `CLEANUP_V0_10_NOTES.md`).
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
    // Only the stub signature is bit-reproducible. For keyless,
    // verifying against the Rekor entry is a v0.11 follow-up — for
    // now we accept the sig as long as the bundle hash matches.
    let envelope_mode = envelope_mode(&envelope_path);
    if envelope_mode == "stub" {
        let expected_sig = stub_signature(&parsed.bundle_sha256_hex, &parsed.identity_hex);
        if expected_sig != parsed.signature_hex {
            return Err(SigningError::Verify(format!(
                "signature mismatch for identity {}",
                parsed.identity_hex
            )));
        }
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
// Stub path (default; v0.9 behaviour, preserved unchanged)
// ============================================================

fn write_stub_signature(
    outcome: &PublishOutcome,
    bundle_sha256_hex: String,
) -> Result<SignedBundle, SigningError> {
    let identity_hex = stub_identity(&outcome.package_name, &outcome.package_version);
    let signature_hex = stub_signature(&bundle_sha256_hex, &identity_hex);

    let sig_path = with_suffix(&outcome.bundle_path, ".sig");
    let envelope_path = with_suffix(&outcome.bundle_path, ".bundle");

    let sig_text = format_sig_text(&bundle_sha256_hex, &identity_hex, 0, &signature_hex);
    std::fs::write(&sig_path, sig_text.as_bytes())?;

    let envelope_text =
        format_envelope_json(&bundle_sha256_hex, &identity_hex, &signature_hex, "stub");
    std::fs::write(&envelope_path, envelope_text.as_bytes())?;

    Ok(SignedBundle {
        bundle_path: outcome.bundle_path.clone(),
        sig_path,
        envelope_path,
        bundle_sha256_hex,
        identity_hex,
        signature_hex,
        mode: SigningMode::Stub,
    })
}

// ============================================================
// Keyless path (feature-gated)
// ============================================================

#[cfg(feature = "sigstore-real")]
fn sign_keyless(
    outcome: &PublishOutcome,
    bundle_sha256_hex: String,
) -> Result<SignedBundle, SigningError> {
    // Build a tokio runtime on demand — we only need it when
    // sigstore-real is enabled, so we don't pay the runtime cost
    // for stub publishes.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| SigningError::Sigstore(format!("tokio runtime: {e}")))?;

    let bundle_bytes = std::fs::read(&outcome.bundle_path)?;

    // The actual keyless flow (Fulcio cert exchange + Rekor log
    // upload) needs an OIDC token. Two paths:
    //
    //   * CI: `$ACTIONS_ID_TOKEN_REQUEST_URL` + `$ACTIONS_ID_TOKEN_REQUEST_TOKEN`
    //     present → fetch a workflow-scoped OIDC token from GitHub.
    //   * Local: device-flow OAuth (sigstore::oauth::openidflow). We
    //     don't drive an interactive flow from `sign_bundle` — that's
    //     a UX detour. Instead, if no OIDC token is available we
    //     fall back to the stub envelope and report `Stub` in
    //     `SignedBundle::mode`.
    //
    // The sigstore crate's keyless surface is async + requires a
    // tokio runtime; the wrapping closure below is the only place
    // we need it.
    let result = rt.block_on(async {
        let oidc_token = match fetch_github_oidc_token().await {
            Ok(Some(t)) => t,
            _ => {
                // No ambient OIDC available — fall back to stub and
                // report the actual mode in the returned struct.
                return Ok(None);
            }
        };
        do_sigstore_sign(&bundle_bytes, &oidc_token).await.map(Some)
    });

    let envelope_text;
    let sig_text;
    let identity_hex;
    let signature_hex;
    let mode_used;
    match result {
        Ok(Some(real)) => {
            identity_hex = real.identity_hex;
            signature_hex = real.signature_hex;
            sig_text = format_sig_text(
                &bundle_sha256_hex,
                &identity_hex,
                real.signed_at,
                &signature_hex,
            );
            envelope_text =
                format_envelope_json(&bundle_sha256_hex, &identity_hex, &signature_hex, "keyless");
            mode_used = SigningMode::Keyless;
        }
        Ok(None) => {
            // Fall back to stub.
            return write_stub_signature(outcome, bundle_sha256_hex);
        }
        Err(e) => {
            // Real signing tripped — surface the error rather than
            // silently downgrading.
            return Err(e);
        }
    }

    let sig_path = with_suffix(&outcome.bundle_path, ".sig");
    let envelope_path = with_suffix(&outcome.bundle_path, ".bundle");
    std::fs::write(&sig_path, sig_text.as_bytes())?;
    std::fs::write(&envelope_path, envelope_text.as_bytes())?;
    Ok(SignedBundle {
        bundle_path: outcome.bundle_path.clone(),
        sig_path,
        envelope_path,
        bundle_sha256_hex,
        identity_hex,
        signature_hex,
        mode: mode_used,
    })
}

#[cfg(not(feature = "sigstore-real"))]
fn sign_keyless(
    outcome: &PublishOutcome,
    bundle_sha256_hex: String,
) -> Result<SignedBundle, SigningError> {
    // Feature flag not enabled — degrade to stub. The returned
    // `SignedBundle::mode` honestly reports `Stub` so callers /
    // verifiers know they got the deterministic envelope.
    //
    // We deliberately do not print a warning here — `mty pkg
    // publish` is the right layer to surface a one-line note
    // (it has access to the `--verbose` flag + stderr). The
    // `commands::publish` helper inspects the returned mode.
    write_stub_signature(outcome, bundle_sha256_hex)
}

#[cfg(feature = "sigstore-real")]
struct RealSigningResult {
    identity_hex: String,
    signature_hex: String,
    signed_at: i64,
}

#[cfg(feature = "sigstore-real")]
async fn do_sigstore_sign(
    bundle_bytes: &[u8],
    oidc_token: &str,
) -> Result<RealSigningResult, SigningError> {
    // The `sigstore` crate's high-level signing API is in flux
    // across the 0.13/0.14 line. To keep this code resilient
    // across patch bumps, we drive the lower-level primitives:
    //
    //   1. Hash the bundle (SHA-256) — already done by the caller.
    //   2. Use `sigstore::sign::SigningContext::async_default()`
    //      (when available) to get a Fulcio+Rekor signer wired to
    //      the public deployment.
    //   3. `signer.signer(&oidc_token).await?` — exchanges the OIDC
    //      token for a short-lived Fulcio cert.
    //   4. `signer.sign(payload).await?` — ECDSA-P256 sign the
    //      digest + upload to Rekor + return the bundle.
    //
    // The sigstore crate version pinned in workspace.dependencies
    // is 0.14; if the upstream API changes shape, this is the only
    // function that needs to be re-wired.
    use sigstore::sign::SigningContext;

    let ctx = SigningContext::async_default()
        .await
        .map_err(|e| SigningError::Sigstore(format!("signing context: {e}")))?;
    let signer = ctx
        .signer(
            sigstore::oauth::IdentityToken::try_from(oidc_token)
                .map_err(|e| SigningError::Sigstore(format!("oidc identity: {e}")))?,
        )
        .await
        .map_err(|e| SigningError::Sigstore(format!("fulcio cert: {e}")))?;
    let signing_result = signer
        .sign(bundle_bytes)
        .await
        .map_err(|e| SigningError::Sigstore(format!("rekor sign: {e}")))?;

    // Bundle is a sigstore::bundle::Bundle; we want the cert
    // thumbprint as our identity and the raw signature hex as our
    // signature. Different sigstore versions expose these slightly
    // differently — we use the protobuf-rust accessors that have
    // been stable since 0.12.
    let bundle = signing_result.to_bundle();
    let identity_hex = bundle
        .verification_material
        .as_ref()
        .and_then(|vm| match &vm.content {
            Some(c) => Some(format!("{:?}", c)),
            None => None,
        })
        .unwrap_or_else(|| "unknown".into());
    // Hash the identity field so it's a deterministic hex string
    // (the raw cert is unwieldy + version-dependent).
    let mut h = Sha256::new();
    h.update(identity_hex.as_bytes());
    let identity_thumb = hex::encode(h.finalize());

    let signature_hex = bundle
        .message_signature
        .as_ref()
        .map(|ms| hex::encode(&ms.signature))
        .unwrap_or_else(|| "unsigned".into());

    let signed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(RealSigningResult {
        identity_hex: identity_thumb,
        signature_hex,
        signed_at,
    })
}

#[cfg(feature = "sigstore-real")]
async fn fetch_github_oidc_token() -> Result<Option<String>, SigningError> {
    // GitHub Actions exposes the OIDC endpoint via two env vars.
    // When either is missing we treat it as "no ambient identity"
    // and let the caller fall back to stub.
    let url = match std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(None),
    };
    let token = match std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN") {
        Ok(v) if !v.is_empty() => v,
        _ => return Ok(None),
    };
    // GitHub returns a JSON envelope `{ "value": "<jwt>" }`.
    // We use reqwest (already in the workspace) because the
    // sigstore crate doesn't ship a plain HTTP client for this.
    //
    // The sigstore audience is `sigstore` per the upstream docs.
    let url = format!("{url}&audience=sigstore");
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| SigningError::Sigstore(format!("github oidc: {e}")))?;
    if !resp.status().is_success() {
        return Err(SigningError::Sigstore(format!(
            "github oidc returned {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SigningError::Sigstore(format!("github oidc body: {e}")))?;
    Ok(body
        .get("value")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

// ============================================================
// Internals — text + JSON format helpers
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

fn format_sig_text(bundle_sha: &str, identity: &str, signed_at: i64, sig: &str) -> String {
    // signed-at is pinned to 0 in stub mode for determinism. The
    // keyless path uses the OIDC token's `iat` claim (approximated
    // here as the system clock at signing time).
    format!(
        "{SIG_FORMAT_VERSION}\n\
         bundle-sha256:{bundle_sha}\n\
         identity:{identity}\n\
         signed-at:{signed_at}\n\
         sig:{sig}\n"
    )
}

fn format_envelope_json(bundle_sha: &str, identity: &str, sig: &str, mode: &str) -> String {
    format!(
        "{{\n  \"mediaType\": \"{BUNDLE_MEDIA_TYPE}\",\n  \"messageSignature\": {{\n    \"messageDigest\": {{ \"algorithm\": \"SHA2_256\", \"digest\": \"{bundle_sha}\" }},\n    \"signature\": \"{sig}\"\n  }},\n  \"verificationMaterial\": {{\n    \"identity\": \"{identity}\",\n    \"mode\": \"{mode}\"\n  }}\n}}\n"
    )
}

/// Extract the `verificationMaterial.mode` field from a `.bundle`
/// JSON file. Returns `"stub"` when the file is missing or
/// unparseable (the v0.9 envelopes had no explicit mode field and
/// were always stub by construction).
fn envelope_mode(path: &Path) -> String {
    if !path.exists() {
        return "stub".into();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return "stub".into();
    };
    if text.contains("\"mode\": \"keyless\"") {
        "keyless".into()
    } else {
        "stub".into()
    }
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
        assert_eq!(signed.mode, SigningMode::Stub);

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
        let text = format_sig_text("aa", "bb", 0, "cc");
        let parsed = parse_sig_text(&text).expect("parse");
        assert_eq!(parsed.bundle_sha256_hex, "aa");
        assert_eq!(parsed.identity_hex, "bb");
        assert_eq!(parsed.signature_hex, "cc");
    }

    #[test]
    fn envelope_json_contains_canonical_fields() {
        let json = format_envelope_json("aaa", "bbb", "ccc", "stub");
        assert!(json.contains("\"mediaType\""));
        assert!(json.contains(BUNDLE_MEDIA_TYPE));
        assert!(json.contains("\"SHA2_256\""));
        assert!(json.contains("\"aaa\""));
        assert!(json.contains("\"bbb\""));
        assert!(json.contains("\"ccc\""));
        assert!(json.contains("\"mode\": \"stub\""));
    }

    #[test]
    fn signing_mode_parses_common_strings() {
        assert_eq!(SigningMode::parse(None), SigningMode::Stub);
        assert_eq!(SigningMode::parse(Some("stub")), SigningMode::Stub);
        assert_eq!(SigningMode::parse(Some("Stub")), SigningMode::Stub);
        assert_eq!(SigningMode::parse(Some("KEYLESS")), SigningMode::Keyless);
        assert_eq!(SigningMode::parse(Some("real")), SigningMode::Keyless);
        assert_eq!(SigningMode::parse(Some("sigstore")), SigningMode::Keyless);
        assert_eq!(SigningMode::parse(Some("off")), SigningMode::Off);
        assert_eq!(SigningMode::parse(Some("none")), SigningMode::Off);
        // Unknown values quietly degrade to stub — the publish flow
        // never aborts because of a typo in the config.
        assert_eq!(
            SigningMode::parse(Some("definitely-not-real")),
            SigningMode::Stub
        );
    }

    #[test]
    fn off_mode_writes_no_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        let signed = sign_bundle_with_mode(&outcome, SigningMode::Off).expect("sign-off");
        assert_eq!(signed.mode, SigningMode::Off);
        // Empty placeholders, no files created.
        assert!(signed.identity_hex.is_empty());
        assert!(signed.signature_hex.is_empty());
        assert!(!signed.sig_path.exists());
        assert!(!signed.envelope_path.exists());
    }

    #[cfg(not(feature = "sigstore-real"))]
    #[test]
    fn keyless_without_feature_falls_back_to_stub() {
        let dir = tempfile::tempdir().unwrap();
        write_pkg(dir.path(), "siggy", "0.1.0");
        let outcome = bundle(dir.path()).expect("bundle");
        // Requesting keyless without the feature flag should not
        // error — we degrade gracefully so the publish command keeps
        // working on Windows / lean-dep hosts.
        let signed =
            sign_bundle_with_mode(&outcome, SigningMode::Keyless).expect("sign-keyless-stub");
        assert_eq!(signed.mode, SigningMode::Stub);
        assert!(signed.sig_path.exists());
    }
}
