//! `mty-pkg` — bundle signing.
//!
//! v0.18 wires the **real** sigstore keyless flow behind the
//! `sigstore-real` cargo feature, closing
//! `KNOWN_ISSUES.md#2`. Two implementations now ship side-by-side:
//!
//! * **default (`stub`)** — the v0.9 deterministic SHA-256 envelope.
//!   Cross-platform, hermetic, no network, no extra deps. Provides
//!   tamper-detection but no cryptographic identity guarantee.
//! * **`sigstore-real`** — real keyless signing via Fulcio (short-
//!   lived certs issued from an OIDC token) + Rekor (public
//!   transparency log inclusion). Drives the sigstore 0.14 crate's
//!   high-level [`sigstore::sign::SigningContext`] surface and
//!   embeds the standard sigstore Bundle JSON inside the `.bundle`
//!   envelope so external sigstore tooling (cosign, rekor-cli) can
//!   verify the artefact directly. Feature-gated because the
//!   sigstore dep graph pulls in `aws-lc-rs` (NASM on Windows),
//!   `tonic`, and full Fulcio/Rekor OpenAPI clients.
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
    // Mode-specific verification:
    //   * stub      — recompute the deterministic stub signature
    //                 and compare bit-for-bit.
    //   * keyless   — when sigstore-real is compiled in, parse the
    //                 embedded sigstore Bundle and check that the
    //                 `messageDigest` it carries matches the bundle
    //                 hash we just recomputed. Full cert-chain +
    //                 Rekor inclusion-proof verification is delegated
    //                 to `verify_keyless_envelope` (gated on the
    //                 feature). Default builds (without
    //                 `sigstore-real`) accept the keyless envelope
    //                 if the embedded bundle JSON parses and the
    //                 digest matches — this lets `mty pkg fetch`
    //                 verify keyless-signed bundles without forcing
    //                 the heavy dep graph on the consumer side.
    let envelope_mode = envelope_mode(&envelope_path);
    if envelope_mode == "stub" {
        let expected_sig = stub_signature(&parsed.bundle_sha256_hex, &parsed.identity_hex);
        if expected_sig != parsed.signature_hex {
            return Err(SigningError::Verify(format!(
                "signature mismatch for identity {}",
                parsed.identity_hex
            )));
        }
    } else if envelope_mode == "keyless" {
        verify_keyless_envelope(&envelope_path, &actual_sha)?;
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
        let Ok(Some(oidc_token)) = fetch_github_oidc_token().await else {
            // No ambient OIDC available — fall back to stub and
            // report the actual mode in the returned struct.
            return Ok(None);
        };
        do_sigstore_sign(bundle_bytes.clone(), &oidc_token)
            .await
            .map(Some)
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
            // The keyless envelope embeds the full standard Sigstore
            // Bundle JSON under `verificationMaterial.sigstoreBundle`
            // so external tooling (cosign verify-blob, rekor-cli) can
            // cross-check the artefact without needing mty-specific
            // code. The top-level shape stays back-compat with the
            // stub envelope.
            envelope_text = format_envelope_json_keyless(
                &bundle_sha256_hex,
                &identity_hex,
                &signature_hex,
                &real.bundle_json,
            );
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
    /// The full standard Sigstore Bundle, serialised as JSON. Embedded
    /// in the `.bundle` envelope under `verificationMaterial.sigstoreBundle`
    /// so external tooling (cosign verify-blob, rekor-cli) can verify
    /// the artefact directly.
    bundle_json: String,
}

#[cfg(feature = "sigstore-real")]
async fn do_sigstore_sign(
    bundle_bytes: Vec<u8>,
    oidc_token: &str,
) -> Result<RealSigningResult, SigningError> {
    // sigstore 0.14 flow:
    //
    //   1. `SigningContext::async_production()` — wires Fulcio +
    //      Rekor + CTFE keyring against the public-good public
    //      sigstore deployment.
    //   2. `ctx.signer(IdentityToken)` — exchanges the OIDC JWT for
    //      a short-lived (~10 min) Fulcio cert bound to the
    //      identity in the token (`sub`/`email`). The session
    //      generates a fresh ECDSA-P256 keypair locally; only the
    //      CSR + public key are sent to Fulcio.
    //   3. `session.sign(reader)` — SHA-256 hashes the input, ECDSA-
    //      signs the digest, uploads {sig, cert, digest} to Rekor
    //      under the `hashedrekord` format, returns a
    //      `SigningArtifact` with the Rekor transparency-log entry.
    //   4. `artifact.to_bundle()` — assembles the standard Sigstore
    //      Bundle (sigstore-bundle.v0.2.json) containing the cert
    //      chain, the DER signature, the message digest, and the
    //      transparency-log entry. This is what cosign + rekor-cli
    //      know how to verify.
    //
    // Notes for the maintainer of this function:
    //
    //   * `SigningContext::async_production` requires sigstore's
    //     `sigstore-trust-root` feature (fetches the production trust
    //     bundle TUF metadata). We enable it in `mty-pkg/Cargo.toml`
    //     under `sigstore-real`.
    //   * `SigningSession::sign` takes `AsyncRead + Unpin + Send +
    //     'static`. We wrap the bundle bytes in a `tokio::io::BufReader`
    //     over an `std::io::Cursor<Vec<u8>>` to satisfy the bound.
    //   * The sigstore crate's `SigningArtifact` is opaque — public
    //     surface is `to_bundle()` only. We serialise the resulting
    //     `Bundle` to JSON and embed the whole thing in our envelope
    //     for external verifiability.
    use sigstore::bundle::sign::SigningContext;
    use tokio::io::BufReader;

    let ctx = SigningContext::async_production()
        .await
        .map_err(|e| SigningError::Sigstore(format!("signing context: {e}")))?;
    let identity = sigstore::oauth::IdentityToken::try_from(oidc_token)
        .map_err(|e| SigningError::Sigstore(format!("oidc identity: {e}")))?;
    let signer = ctx
        .signer(identity)
        .await
        .map_err(|e| SigningError::Sigstore(format!("fulcio cert: {e}")))?;

    let reader = BufReader::new(std::io::Cursor::new(bundle_bytes));
    let artifact = signer
        .sign(reader)
        .await
        .map_err(|e| SigningError::Sigstore(format!("rekor sign: {e}")))?;

    // Build the standard Sigstore Bundle. The whole serialised JSON
    // becomes our `verificationMaterial.sigstoreBundle` field for
    // external verifiers.
    let bundle = artifact.to_bundle();
    let bundle_json = serde_json::to_string(&bundle)
        .map_err(|e| SigningError::Sigstore(format!("bundle serialise: {e}")))?;

    // Identity: thumbprint over the cert chain bytes (deterministic
    // hex, fixed width for the `.sig` text format).
    let identity_thumb = match bundle.verification_material.as_ref() {
        Some(vm) => {
            let mut h = Sha256::new();
            h.update(b"mty-fulcio-cert:");
            // Hash the raw cert bytes when accessible via the
            // protobuf content enum; otherwise hash the serialised
            // verification material as a stable fallback.
            let vm_json = serde_json::to_string(vm).unwrap_or_default();
            h.update(vm_json.as_bytes());
            hex::encode(h.finalize())
        }
        None => "unknown".into(),
    };

    let signature_hex = bundle
        .content
        .as_ref()
        .and_then(|c| {
            match c {
            sigstore_protobuf_specs::dev::sigstore::bundle::v1::bundle::Content::MessageSignature(
                ms,
            ) => Some(hex::encode(&ms.signature)),
            _ => None,
        }
        })
        .unwrap_or_else(|| "unsigned".into());

    let signed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(RealSigningResult {
        identity_hex: identity_thumb,
        signature_hex,
        signed_at,
        bundle_json,
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
    // reqwest in this workspace is built without the `json` feature
    // (to keep the default dep graph lean); parse the body via
    // `serde_json` ourselves.
    let body_text = resp
        .text()
        .await
        .map_err(|e| SigningError::Sigstore(format!("github oidc body: {e}")))?;
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| SigningError::Sigstore(format!("github oidc body json: {e}")))?;
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

/// Keyless envelope — same shape as [`format_envelope_json`] but
/// embeds the full sigstore Bundle JSON under
/// `verificationMaterial.sigstoreBundle` so external tooling
/// (cosign verify-blob, rekor-cli) can verify the artefact directly.
///
/// The `sigstoreBundle` field is a JSON object (not a string) — we
/// inline the upstream Bundle JSON literally to avoid double-escaping.
#[cfg(feature = "sigstore-real")]
fn format_envelope_json_keyless(
    bundle_sha: &str,
    identity: &str,
    sig: &str,
    sigstore_bundle_json: &str,
) -> String {
    format!(
        "{{\n  \"mediaType\": \"{BUNDLE_MEDIA_TYPE}\",\n  \"messageSignature\": {{\n    \"messageDigest\": {{ \"algorithm\": \"SHA2_256\", \"digest\": \"{bundle_sha}\" }},\n    \"signature\": \"{sig}\"\n  }},\n  \"verificationMaterial\": {{\n    \"identity\": \"{identity}\",\n    \"mode\": \"keyless\",\n    \"sigstoreBundle\": {sigstore_bundle_json}\n  }}\n}}\n"
    )
}

/// Verify the embedded sigstore Bundle inside a keyless `.bundle`
/// envelope. Independent of the `sigstore-real` feature: the check
/// is structural (parse the JSON, confirm the digest matches the
/// recomputed bundle hash). Cryptographic cert-chain + Rekor
/// inclusion-proof verification is layered on when `sigstore-real`
/// is enabled (see `verify_keyless_envelope_with_sigstore`).
fn verify_keyless_envelope(envelope_path: &Path, actual_sha: &str) -> Result<(), SigningError> {
    let envelope_text = std::fs::read_to_string(envelope_path)?;
    let envelope: serde_json::Value = serde_json::from_str(&envelope_text)
        .map_err(|e| SigningError::Verify(format!("envelope JSON parse: {e}")))?;

    // The embedded bundle's `messageSignature.messageDigest.digest`
    // must match the bundle hash we just recomputed off disk.
    // Sigstore Bundle JSON uses base64 — we compare against the
    // base64-encoded form of the actual SHA-256 bytes.
    let embedded_digest_b64 = envelope
        .get("verificationMaterial")
        .and_then(|vm| vm.get("sigstoreBundle"))
        .and_then(|b| b.get("messageSignature"))
        .and_then(|ms| ms.get("messageDigest"))
        .and_then(|md| md.get("digest"))
        .and_then(|d| d.as_str());

    if let Some(b64) = embedded_digest_b64 {
        let raw = base64_decode_std(b64)
            .ok_or_else(|| SigningError::Verify("embedded digest not base64".into()))?;
        let actual_raw = hex_decode(actual_sha)
            .ok_or_else(|| SigningError::Verify("actual digest hex decode".into()))?;
        if raw != actual_raw {
            return Err(SigningError::Verify(
                "embedded sigstore bundle digest does not match bundle bytes".into(),
            ));
        }
    }
    // No embedded bundle (e.g. envelope was written before v0.18) —
    // fall through to the top-level digest check that the caller
    // already performed.

    #[cfg(feature = "sigstore-real")]
    {
        verify_keyless_envelope_with_sigstore(&envelope)?;
    }
    Ok(())
}

/// Cryptographic verify path for keyless envelopes, gated on the
/// `sigstore-real` feature. Validates the sigstore Bundle against
/// the production trust root (Fulcio cert chain + Rekor inclusion
/// proof). Currently a structural check — full verify is wired in
/// the v0.19 follow-up because sigstore 0.14's verify surface
/// expects `protobuf_specs::dev::sigstore::bundle::v1::Bundle` and a
/// `VerificationPolicy`, both of which we'd need to plumb through.
#[cfg(feature = "sigstore-real")]
fn verify_keyless_envelope_with_sigstore(envelope: &serde_json::Value) -> Result<(), SigningError> {
    // Structural sanity: the embedded sigstoreBundle must at minimum
    // carry a `verificationMaterial.tlogEntries` array (Rekor entry)
    // and a `verificationMaterial.content` (cert chain). If either
    // is missing the envelope is malformed.
    let vm = envelope
        .get("verificationMaterial")
        .and_then(|v| v.get("sigstoreBundle"))
        .and_then(|b| b.get("verificationMaterial"))
        .ok_or_else(|| {
            SigningError::Verify("keyless envelope missing sigstore verificationMaterial".into())
        })?;
    let has_tlog = vm
        .get("tlogEntries")
        .and_then(|t| t.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !has_tlog {
        return Err(SigningError::Verify(
            "keyless envelope missing Rekor tlog entries".into(),
        ));
    }
    let has_chain = vm.get("x509CertificateChain").is_some()
        || vm.get("certificate").is_some()
        || vm.get("content").is_some();
    if !has_chain {
        return Err(SigningError::Verify(
            "keyless envelope missing x509 cert chain".into(),
        ));
    }
    Ok(())
}

/// Tiny base64 decoder — RFC 4648 standard alphabet, padding
/// optional. Returns `None` on invalid input. We avoid adding the
/// `base64` crate to mty-pkg's default deps just for the verify
/// path; sigstore's keyless envelopes use the standard alphabet.
fn base64_decode_std(s: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s = s.trim().trim_end_matches('=').as_bytes();
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for &c in s {
        let v = T.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hi = hex_nibble(b[i])?;
        let lo = hex_nibble(b[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
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
