# Package signing

> Status: **v0.18 — sigstore-real wired**. Default builds keep the
> v0.9 deterministic stub envelope (cross-platform, hermetic, no
> extra deps). Real keyless signing via Fulcio + Rekor is **opt-in**
> via the `mty-pkg/sigstore-real` cargo feature and now produces
> standard Sigstore Bundle JSON that external tooling (cosign,
> rekor-cli) can verify directly. See
> `dev/history/notes/SIGSTORE_V0_18_NOTES.md` for the v0.18
> cut-over rationale and `CLEANUP_V0_10_NOTES.md` for the original
> v0.10 mode-dispatch shape.

## Goals

1. Every `mty pkg publish` produces a signed bundle so consumers can
   verify that the bytes they download match what the publisher
   pushed.
2. The on-disk artifact shape is forward-compatible with sigstore so
   we can swap the stub identity for a Fulcio cert later without
   breaking downstream verifiers.
3. Verification is integrated into `mty pkg fetch`, so users never
   have to invoke the signing layer directly.

## Modes

The `[registry.signing]` table in `mighty.toml` selects the mode:

```toml
[registry.signing]
mode = "keyless"                                  # or "stub" (default), or "off"
oidc_issuer = "https://oauth2.sigstore.dev/auth"  # keyless only; optional
```

| Mode | Behaviour | Requires |
|------|-----------|----------|
| `stub` *(default)* | Deterministic SHA-256 envelope. Detects tampering; no identity guarantee. | nothing extra |
| `keyless` | Real Fulcio cert + Rekor transparency-log entry. | `sigstore-real` feature **and** ambient OIDC identity |
| `off` | Skip signing. No `.sig` / `.bundle` sidecars written. | nothing |

When `mode = "keyless"` but the binary was built without
`sigstore-real`, the publish command **degrades to stub** and prints
a one-line note. This keeps `mty pkg publish` working on hosts
where the sigstore dep graph won't compile (Windows + NASM is the
historical headache, see below).

## Artifacts

For a bundle `<pkg>-<version>.tar.gz`, signing produces two
sidecars (in both `stub` and `keyless` modes):

| File | Format | Purpose |
| --- | --- | --- |
| `<pkg>-<version>.tar.gz.sig` | text, one key:value per line | Compact human-readable signature record. |
| `<pkg>-<version>.tar.gz.bundle` | JSON | Sigstore-shaped envelope (mediaType, messageSignature, verificationMaterial). |

In `off` mode neither sidecar is written.

### `.sig` format

```text
mty-sig/1
bundle-sha256:<64-hex>
identity:<hex-or-cert-thumbprint>
signed-at:<unix-secs-or-0>
sig:<hex>
```

- `mty-sig/1` is the format-version header. The verifier rejects
  files without it.
- `bundle-sha256` is the SHA-256 of the bundle bytes.
- `identity` (stub) is `SHA-256("mty-stub-id:<pkg>:<version>")`.
- `identity` (keyless) is the SHA-256 of the Fulcio cert's
  serialised contents (kept hashed so the `.sig` text stays a
  fixed-width line).
- `signed-at` is `0` in stub mode for byte-for-byte determinism. In
  keyless mode it's the system clock at signing time (approximating
  the OIDC token's `iat` claim).
- `sig` (stub) is `SHA-256("mty-stub-sig:" || bundle_sha256_hex ||
  ":" || identity_hex)`.
- `sig` (keyless) is the raw ECDSA-P256 signature returned by
  Fulcio + Rekor, hex-encoded.

### `.bundle` format (JSON)

```json
{
  "mediaType": "application/vnd.mty.bundle.v0.10+json",
  "messageSignature": {
    "messageDigest": { "algorithm": "SHA2_256", "digest": "<hex>" },
    "signature": "<hex>"
  },
  "verificationMaterial": {
    "identity": "<hex>",
    "mode": "stub" | "keyless"
  }
}
```

The shape mirrors the sigstore "bundle" media type. The
`verificationMaterial.mode` field tells future tooling whether the
cryptographic material is binding (`keyless`) or advisory (`stub`).

Media type bumped from `…v0.9+json` to `…v0.10+json` so verifiers
can distinguish the new mode-aware envelopes from the v0.9 ones at
a glance.

## API

```rust
use mty_pkg::publish::bundle;
use mty_pkg::signing::{sign_bundle, sign_bundle_with_mode, verify_bundle, SigningMode};

let outcome = bundle(repo_root)?;

// Legacy entry point — always stub. Kept for back-compat.
let signed = sign_bundle(&outcome)?;

// Mode-aware entry point — the new v0.10 API.
let signed = sign_bundle_with_mode(&outcome, SigningMode::Keyless)?;
verify_bundle(&outcome.bundle_path)?;
```

- `sign_bundle(outcome)` is the v0.9 surface preserved for
  back-compat. Always uses [`SigningMode::Stub`].
- `sign_bundle_with_mode(outcome, mode)` honours the requested
  mode. `Keyless` degrades to `Stub` when the `sigstore-real`
  feature isn't compiled in; `Off` writes no sidecars.
- `verify_bundle(path)` reads the `.sig` envelope, re-hashes the
  bundle, recomputes the stub signature (for stub mode), and
  confirms the JSON envelope agrees. Returns `Ok(())` on success.

## Wire-up in `mty pkg publish`

`mty_pkg::commands::publish` calls `signing::sign_bundle_with_mode`
with the mode taken from `[registry.signing]`. The signed artefacts
are included in the "auth required" message so users with no token
still see all four file paths to upload manually. The reported
mode in the message reflects what was *actually* used (e.g.
"signature path.sig (mode: Stub)" when keyless was requested but
the feature wasn't enabled).

## Determinism

The stub signature is a pure function of `(pkg_name, pkg_version,
bundle_bytes)`. Two identical bundles produce identical
`.sig` + `.bundle` sidecars byte-for-byte — relied on by
`signing::tests::signing_is_deterministic_for_same_input`.

The keyless signature is **not** deterministic (`signed-at` uses
the system clock; the Fulcio cert is freshly issued per invocation
with a fresh ECDSA keypair). Reproducible-build workflows that
need byte-identical artefacts should pin `mode = "stub"` or
`mode = "off"`.

## Verification on fetch (v0.11)

`mty_pkg::fetch` will call `verify_bundle` after downloading a
registry tarball. v0.10 ships the verify primitive but does not yet
gate `fetch` on it — that's tracked under the v0.11 plan in
`KNOWN_ISSUES.md`.

## Why `sigstore-real` is a feature flag

The `sigstore` Rust crate pulls in:

- `tonic` + a Fulcio gRPC client
- a Rekor OpenAPI client
- `aws-lc-rs` (required by `rustls-webpki/aws-lc-rs` for the
  `cert` feature) — **needs NASM at build time on Windows**
- on some hosts, an `openssl-sys` transitive

…which would noticeably slow the v0.10 build + complicate CI on
Windows. Fresh-clone confirmation (2026-05-23):

```
cargo build -p mty-pkg --features sigstore-real
# fails on Windows without NASM:
# panicked at nasm_builder.rs:138: NASM command not found! Build cannot continue.
```

The v0.10 policy is: ship the **shape + mode dispatch + degraded
fallback**, gate real signing behind a feature flag so default
builds stay fast + cross-platform. Linux CI runners get NASM
trivially (`apt install nasm`); Windows users either install NASM
or stick with stub mode.

## v0.10 cut-over status

| Sub-task | Status |
|----------|--------|
| Mode dispatch (`SigningMode::{Stub, Keyless, Off}`) | shipped |
| `[registry.signing]` config plumbed through `publish` | shipped |
| Real Fulcio cert exchange (feature-gated) | shipped (v0.18) |
| GitHub Actions OIDC token fetch (feature-gated) | shipped (v0.18) |
| Rekor transparency-log upload | shipped (v0.18) |
| Standard Sigstore Bundle embedded in `.bundle` | shipped (v0.18) |
| Structural verify of keyless envelopes (digest cross-check) | shipped (v0.18) |
| Verify against Rekor entry on `fetch` (full crypto verify) | **v0.19 follow-up** |
| Device-flow OAuth for local signing | **v0.19 follow-up** |
| SLSA provenance attestations | **v0.19 follow-up** |
| MSRV-compatible sigstore version pin | tracked — sigstore 0.14 + workspace MSRV 1.85 |

See `dev/history/notes/SIGSTORE_V0_18_NOTES.md` for the v0.18
implementation choices and v0.19 follow-ups.

## v0.18: real keyless flow (OIDC + Fulcio + Rekor)

When `[registry.signing] mode = "keyless"` is set **and** the
binary was built with `--features mty-pkg/sigstore-real`, the
publish path executes the full sigstore keyless flow:

```text
   ┌─────────────────────┐
   │ mty pkg publish     │
   └──────────┬──────────┘
              │ bundle bytes + bundle SHA-256
              ▼
   ┌─────────────────────────────────────────┐
   │ 1. OIDC token fetch                     │
   │    GET $ACTIONS_ID_TOKEN_REQUEST_URL    │
   │       ?audience=sigstore                │
   │    Authorization: Bearer $..._TOKEN     │
   │    → JWT bound to workflow identity     │
   └──────────┬──────────────────────────────┘
              │ JWT
              ▼
   ┌─────────────────────────────────────────┐
   │ 2. SigningContext::async_production()   │
   │    Loads the public-good sigstore TUF   │
   │    trust root (CTFE keys + Fulcio root  │
   │    + Rekor public key).                 │
   └──────────┬──────────────────────────────┘
              │ ctx
              ▼
   ┌─────────────────────────────────────────┐
   │ 3. ctx.signer(IdentityToken)            │
   │    Generates fresh ECDSA-P256 keypair   │
   │    locally; POSTs CSR + pubkey to       │
   │    https://fulcio.sigstore.dev/api/v1/  │
   │       signingCert                       │
   │    → short-lived (~10 min) x509 cert    │
   │      bound to the OIDC identity (sub,   │
   │      email) and a SCT proving CT log    │
   │      inclusion.                         │
   └──────────┬──────────────────────────────┘
              │ session (privkey + cert)
              ▼
   ┌─────────────────────────────────────────┐
   │ 4. session.sign(bundle_reader)          │
   │    a. SHA-256(bundle bytes)             │
   │    b. ECDSA-P256 sign(digest)           │
   │    c. POST {sig, cert PEM, digest hex}  │
   │       to https://rekor.sigstore.dev/    │
   │       api/v1/log/entries (hashedrekord) │
   │    d. Rekor returns TransparencyLog-    │
   │       Entry with log index, integrated  │
   │       time, signed inclusion-proof.     │
   └──────────┬──────────────────────────────┘
              │ SigningArtifact
              ▼
   ┌─────────────────────────────────────────┐
   │ 5. artifact.to_bundle()                 │
   │    Assembles the standard Sigstore      │
   │    Bundle (sigstore-bundle.v0.2 JSON)   │
   │    with cert chain, DER signature,      │
   │    message digest, Rekor entry.         │
   └──────────┬──────────────────────────────┘
              │ Bundle JSON
              ▼
   ┌─────────────────────────────────────────┐
   │ 6. Write <pkg>.tar.gz.sig + .bundle     │
   │    .bundle embeds the full Sigstore     │
   │    Bundle under                         │
   │      verificationMaterial.sigstoreBundle│
   │    so cosign / rekor-cli can verify     │
   │    the artefact directly.               │
   └─────────────────────────────────────────┘
```

### Example `.bundle` envelope (keyless mode)

```json
{
  "mediaType": "application/vnd.mty.bundle.v0.10+json",
  "messageSignature": {
    "messageDigest": { "algorithm": "SHA2_256", "digest": "<bundle-hex>" },
    "signature": "<ecdsa-sig-hex>"
  },
  "verificationMaterial": {
    "identity": "<fulcio-cert-thumbprint-hex>",
    "mode": "keyless",
    "sigstoreBundle": {
      "mediaType": "application/vnd.dev.sigstore.bundle+json;version=0.2",
      "verificationMaterial": {
        "x509CertificateChain": {
          "certificates": [ { "rawBytes": "<leaf-cert-DER-base64>" } ]
        },
        "tlogEntries": [
          {
            "logIndex": "<rekor-index>",
            "logId": { "keyId": "<rekor-pubkey-id>" },
            "kindVersion": { "kind": "hashedrekord", "version": "0.0.1" },
            "integratedTime": "<unix-secs>",
            "canonicalizedBody": "<base64>",
            "inclusionProof": { ... },
            "inclusionPromise": { ... }
          }
        ]
      },
      "messageSignature": {
        "messageDigest": { "algorithm": "SHA2_256", "digest": "<base64>" },
        "signature": "<DER-base64>"
      }
    }
  }
}
```

### Verify path (v0.18)

`verify_bundle(<bundle>.tar.gz)` runs the following for keyless envelopes:

1. Recompute the SHA-256 of the bundle bytes off disk.
2. Parse the `.sig` text + `.bundle` JSON; confirm the top-level
   digest + signature appear in the JSON envelope (back-compat
   cross-check).
3. Decode the embedded
   `verificationMaterial.sigstoreBundle.messageSignature.messageDigest.digest`
   from base64 and confirm it matches the recomputed SHA-256.
4. When built with `--features sigstore-real`, additionally check
   that `tlogEntries[]` is non-empty and that an x509 cert chain
   is present in the embedded bundle (structural — full Rekor
   inclusion-proof crypto verify lands in v0.19).

### CI integration

For GitHub Actions to expose the OIDC endpoint to `mty pkg
publish`, the workflow needs the `id-token: write` permission:

```yaml
permissions:
  contents: write   # to upload the GitHub Release
  id-token: write   # to mint an OIDC JWT for sigstore

jobs:
  publish:
    runs-on: ubuntu-latest      # Linux: NASM available out of the box
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p mty-cli --features mty-pkg/sigstore-real
      - run: ./target/release/mty pkg publish
```

With the permission granted, `$ACTIONS_ID_TOKEN_REQUEST_URL` and
`$ACTIONS_ID_TOKEN_REQUEST_TOKEN` are populated automatically. Outside
of CI (or when those env vars are absent), `sign_keyless` degrades
to stub mode and reports `SigningMode::Stub` in the returned
`SignedBundle`. Device-flow OAuth for local signing is a v0.19
follow-up.

## Testing

| Test file | What it covers |
|-----------|----------------|
| `crates/mty-pkg/src/signing.rs::tests::*` | Round-trip stub sign+verify, tamper detection, deterministic re-sign, mode parser, off-mode no-sidecar, keyless-degrades-to-stub. Hermetic — runs on default features. |
| `crates/mty-pkg/tests/signing_real.rs::*` (default) | Public-API contract: keyless request must not error on default builds; off mode skips sidecars. |
| `crates/mty-pkg/tests/signing_real.rs::keyless_round_trip_via_fulcio_and_rekor` | `#[ignore]`d + `#[cfg(feature = "sigstore-real")]`. Real network round-trip against the public Sigstore deployment. Run with `cargo test -p mty-pkg --features sigstore-real -- --ignored` on a Linux CI runner with ambient OIDC. |
