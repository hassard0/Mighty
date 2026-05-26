# SIGSTORE_V0_18_NOTES — wiring real keyless signing

Closes `KNOWN_ISSUES.md#2` ("Package signing is a stub (no real OIDC
/ Rekor)"). Item #2 was carried from v0.10's mode-aware refactor —
the dispatch was in place but the keyless arm produced stub bytes
because the sigstore-crate plumbing didn't compile against v0.14's
final API.

## What ships in v0.18

- `mty-pkg/sigstore-real` feature now compiles + type-checks
  cleanly. The cargo feature enables sigstore 0.14's `sign`,
  `sigstore-trust-root`, and `rustls-tls` features, plus
  `sigstore_protobuf_specs` for direct Bundle field access. reqwest
  is pulled in unconditionally under the feature so the OIDC token
  fetch path has an HTTP client.
- `sign_keyless` drives the real `SigningContext::async_production`
  surface: Fulcio cert exchange + ECDSA-P256 sign + Rekor hashedrekord
  upload. The full sigstore Bundle is serialised to JSON and embedded
  in the `.bundle` envelope under
  `verificationMaterial.sigstoreBundle` so cosign / rekor-cli can
  verify the artefact directly without any mty-specific
  understanding.
- `verify_bundle` now cross-checks the embedded sigstoreBundle's
  `messageDigest` against the recomputed bundle hash on disk. When
  built with `sigstore-real` it also validates the structural
  presence of the cert chain + Rekor tlog entries (full cryptographic
  cert-chain + inclusion-proof verify is a v0.19 follow-up).
- Three new gated tests under `tests/signing_real.rs`:
  - `verify_bundle_recognizes_real_signed_envelope` — forges a
    keyless envelope with the v0.18 shape and confirms verify
    accepts it.
  - `verify_rejects_modified_payload_real` — embeds a wrong digest
    in the sigstoreBundle and confirms verify catches the tamper.
  - `verify_falls_back_to_stub_when_real_not_present` — confirms
    stub-only envelopes (no embedded sigstoreBundle) still verify
    under the v0.18 verify path.
  These tests run unconditionally on default features (no network,
  no `sigstore-real` required) — they exercise the
  structural verify path.
- The existing `keyless_round_trip_via_fulcio_and_rekor` test stays
  `#[ignore]` + `#[cfg(feature = "sigstore-real")]` for the
  network-hitting real round-trip.

## Implementation choices

### Why not call `bundle.verification_material.content` directly?

The sigstore-crate's `Bundle` is a re-export of the prost-generated
`sigstore_protobuf_specs::dev::sigstore::bundle::v1::Bundle`. The
`content` field is an `Option<verification_material::Content>` enum
whose variants are versioned and break between sigstore-crate
minor versions. The v0.18 implementation **serialises the whole
Bundle to JSON** and embeds it as-is in our envelope, then drives
verification via JSON-path lookups. This:

- Decouples us from the exact protobuf shape across sigstore-crate
  patch bumps.
- Lets external tooling (cosign verify-blob, rekor-cli) consume the
  embedded `sigstoreBundle` block directly — it's the standard
  Sigstore Bundle JSON, unmodified.
- Keeps the verify path runnable on default builds (no
  sigstore-real needed for the digest cross-check).

### Why not use `reqwest::Response::json()` for the OIDC fetch?

The workspace pin is `reqwest = { default-features = false, features =
["rustls-tls", "blocking"] }` — no `json` feature. Adding it would
pull `serde_urlencoded` + extra hyper bits into every downstream
crate. The OIDC body is a single-field JSON object (`{"value": "<jwt>"}`)
so we just call `resp.text().await?` and parse it ourselves.

### Cert lifetime + offline verify

Fulcio-issued certs are valid for ~10 minutes. The Sigstore Bundle
embeds:

- The leaf x509 cert (DER-encoded, base64'd in the JSON)
- The Rekor transparency-log entry with a signed inclusion proof
  (`inclusionProof.signedEntryTimestamp`)

After the cert expires, verifiers rely on the Rekor entry's
integrated time being within the cert's validity window. This is
the standard sigstore "long-lived signature, short-lived cert"
pattern. Our `.bundle` envelope preserves the inclusion proof, so
offline verify (e.g. months later) is possible against the public
Rekor root + the trust bundle TUF metadata. v0.18 ships the
structural check; full crypto-verify of the inclusion proof is
v0.19.

### NASM on Windows (carryover from v0.10)

The `aws-lc-rs` transitive (pulled by sigstore's `cert` feature,
which `sign` requires) still needs NASM at build time on Windows.
The v0.18 wire keeps this constraint — documented in
`docs/internals/package-signing.md`. Linux CI runners (the
intended sign+publish target) have NASM by default.

We did *not* attempt to swap `aws-lc-rs` → pure-`ring` because
sigstore 0.14's `cert` feature hard-codes `rustls-webpki/aws-lc-rs`.
Replacing it would mean either:

- Forking sigstore to add a `ring` opt-in (yak shaving)
- Hand-rolling the Fulcio/Rekor HTTP exchange with `ecdsa`+`p256`
  crates (~600 LOC + maintenance cost)

Neither is worth it for v0.18 — the publish flow runs on Linux CI.

## v0.19 follow-ups

1. **Full crypto-verify of keyless envelopes** — drive sigstore
   0.14's `bundle::verify::VerificationContext` against the
   embedded Bundle JSON. Will need to deserialise the JSON back
   into the protobuf type + plumb a `VerificationPolicy` (today
   we only do structural checks: digest match, cert chain present,
   tlog entries non-empty).

2. **`mty pkg fetch` gates on `verify_bundle`** — the verify primitive
   exists; fetch needs to call it after download and surface
   verification errors to the user.

3. **Device-flow OAuth for local signing** — `sigstore::oauth::openidflow`
   ships a browser-based OIDC flow. Wiring it lets `mty pkg publish`
   produce keyless signatures from a developer laptop, not just CI.

4. **SLSA provenance attestations** — alongside the bundle signature,
   produce a SLSA v1.0 provenance predicate (build inputs, build
   command, source repo + commit) and sign it as a `dsse-envelope`
   Rekor entry. Lets downstream consumers verify *how* the bundle
   was built, not just *who* signed it.

5. **CI integration smoke** — add a `.github/workflows/sigstore-smoke.yml`
   that builds with `--features sigstore-real`, runs `mty pkg
   publish --dry-run` against a throwaway test registry, and
   verifies the produced `.bundle` against the public Sigstore
   trust root using `cosign verify-blob`. Catches upstream Sigstore
   API regressions before they hit a real publish.

6. **Switch sigstore-crate `cert` to a ring-backed variant** — when
   upstream sigstore-rs publishes a ring-only build path, drop
   the NASM-on-Windows requirement.

## Manual smoke instructions

Until the v0.19 CI smoke lands, validating a real keyless publish
end-to-end requires:

```bash
# 1. Linux host with NASM
sudo apt install nasm

# 2. Build with the feature
cargo build --release -p mty-cli --features mty-pkg/sigstore-real

# 3. In a GitHub Actions runner (or with a GH OIDC token mock):
#    export ACTIONS_ID_TOKEN_REQUEST_URL=...
#    export ACTIONS_ID_TOKEN_REQUEST_TOKEN=...

# 4. Run the ignored network test:
cargo test -p mty-pkg --features sigstore-real \
  --test signing_real -- --ignored keyless_round_trip

# 5. Inspect the produced bundle:
jq . /tmp/keyless-rt-0.1.0.tar.gz.bundle | less

# 6. Cross-verify with cosign:
jq .verificationMaterial.sigstoreBundle \
  /tmp/keyless-rt-0.1.0.tar.gz.bundle > /tmp/bundle.json
cosign verify-blob \
  --bundle /tmp/bundle.json \
  /tmp/keyless-rt-0.1.0.tar.gz
```

## Files touched

- `crates/mty-pkg/Cargo.toml` — enable sigstore features under
  `sigstore-real`; add `sigstore_protobuf_specs` opt-in; make
  `reqwest` part of the feature set.
- `crates/mty-pkg/src/signing.rs` — fix the keyless arm to use
  sigstore 0.14's actual API (`bundle::sign::SigningContext`,
  `async_production`, `BufReader<Cursor<Vec<u8>>>` for AsyncRead).
  Embed the full Bundle JSON in the `.bundle` envelope. Extend
  verify path to cross-check the embedded digest.
- `crates/mty-pkg/tests/signing_real.rs` — 3 new structural verify
  tests (run unconditionally) + retain the existing ignored
  round-trip.
- `docs/internals/package-signing.md` — document the v0.18 OIDC →
  Fulcio → Rekor flow, the example envelope shape, CI integration
  permissions, and verify-path behaviour.
- `KNOWN_ISSUES.md` — strike #2.
