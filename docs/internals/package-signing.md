# Package signing

> Status: **v0.9 — stub**. Real OIDC + Rekor integration lands in
> v0.10 behind a `sigstore-real` feature flag. See
> `KNOWN_ISSUES.md#2` for the upgrade plan.

## Goals

1. Every `mty pkg publish` produces a signed bundle so consumers can
   verify that the bytes they download match what the publisher
   pushed.
2. The on-disk artifact shape is forward-compatible with sigstore so
   we can swap the stub identity for a Fulcio cert later without
   breaking downstream verifiers.
3. Verification is integrated into `mty pkg fetch`, so users never
   have to invoke the signing layer directly.

## Artifacts

For a bundle `<pkg>-<version>.tar.gz`, signing produces two
sidecars:

| File | Format | Purpose |
| --- | --- | --- |
| `<pkg>-<version>.tar.gz.sig` | text, one key:value per line | Compact human-readable signature record. |
| `<pkg>-<version>.tar.gz.bundle` | JSON | Sigstore-shaped envelope (mediaType, messageSignature, verificationMaterial). |

### `.sig` format

```text
mty-sig/1
bundle-sha256:<64-hex>
identity:<64-hex>
signed-at:0
sig:<64-hex>
```

- `mty-sig/1` is the format-version header. The verifier rejects
  files without it.
- `bundle-sha256` is the SHA-256 of the bundle bytes.
- `identity` is `SHA-256("mty-stub-id:<pkg>:<version>")` for v0.9.
  In v0.10 this becomes a Fulcio short-lived cert thumbprint.
- `signed-at` is pinned to `0` for deterministic builds. In v0.10
  this becomes the OIDC token's `iat` claim.
- `sig` is `SHA-256("mty-stub-sig:" || bundle_sha256_hex || ":" ||
  identity_hex)`. In v0.10 this becomes an ECDSA-P256 signature
  over the same input.

### `.bundle` format (JSON)

```json
{
  "mediaType": "application/vnd.mty.bundle.v0.9+json",
  "messageSignature": {
    "messageDigest": { "algorithm": "SHA2_256", "digest": "<hex>" },
    "signature": "<hex>"
  },
  "verificationMaterial": {
    "identity": "<hex>",
    "mode": "stub"
  }
}
```

The shape mirrors the sigstore "bundle" media type. The
`verificationMaterial.mode: stub` field tells future tooling that
the cryptographic material is non-binding.

## API

```rust
use mty_pkg::publish::bundle;
use mty_pkg::signing::{sign_bundle, verify_bundle};

let outcome = bundle(repo_root)?;
let signed = sign_bundle(&outcome)?;
verify_bundle(&outcome.bundle_path)?;
```

- `sign_bundle(&PublishOutcome) -> SignedBundle` writes the two
  sidecars next to the `.tar.gz` and returns their paths.
- `verify_bundle(&Path)` reads the `.sig` envelope, re-hashes the
  bundle, recomputes the stub signature, and confirms the JSON
  envelope agrees. Returns `Ok(())` on success.

## Wire-up in `mty pkg publish`

`mty_pkg::commands::publish` calls `sign_bundle` immediately after
`publish::bundle`. The signed artefacts are included in the "auth
required" message so users with no token still see all four file
paths to upload manually.

## Determinism

The stub signature is a pure function of `(pkg_name, pkg_version,
bundle_bytes)`. Two identical bundles produce identical
`.sig` + `.bundle` sidecars byte-for-byte — relied on by
`signing::tests::signing_is_deterministic_for_same_input`.

## Verification on fetch (v0.10)

`mty_pkg::fetch` will call `verify_bundle` after downloading a
registry tarball. v0.9 ships the verify primitive but does not yet
gate `fetch` on it — that's tracked under the v0.10 plan in
`KNOWN_ISSUES.md`.

## Why a stub for v0.9?

The `sigstore` Rust crate pulls in:

- `tonic` + a Fulcio gRPC client
- a Rekor OpenAPI client
- on some hosts, an `openssl-sys` transitive (Windows in
  particular)

…which would noticeably slow the v0.9 build + complicate CI. The
RC-prep policy is "ship the *shape* of the feature with a
deterministic test", which the stub does. The v0.10 cut-over plan
is:

1. Add `sigstore = { workspace = true, optional = true }` and a
   `sigstore-real` cargo feature.
2. Under the feature flag, replace `stub_signature` with an ECDSA
   signing call against a Fulcio cert sourced from the GitHub
   Actions OIDC endpoint.
3. Upload the signing payload to Rekor and record the log entry
   index in `.bundle`.
4. Bump `BUNDLE_MEDIA_TYPE` to `…v0.10+json` and add a `verifyAt`
   pointing at the Rekor entry.
5. Flip `mty pkg fetch` to call `verify_bundle` by default; gate
   tamper-rejection on a `--no-verify` flag.

See `KNOWN_ISSUES.md#2` for the open follow-up.
