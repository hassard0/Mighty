# `std.crypto` — cryptographic primitives (internals, v0.39 T1 → v0.40 T4)

**Module:** `mty_stdlib::crypto` (submodules `hash`, `hmac`, `rand`, `aes_gcm`, `chacha20_poly1305`)
**Mighty surface:** `use std.crypto.{sha256, hmac_sha256, random_bytes}` + `use std.crypto.aes_gcm.{encrypt, decrypt}`

This document describes the architecture and security model of the
`std.crypto` stack. v0.39 T1 shipped the foundational hash + HMAC +
CSPRNG surface; v0.40 T4 closes the AEAD gap with AES-256-GCM and
ChaCha20-Poly1305 (RFC 8439).

## Module shape

```
crates/mty-stdlib/src/crypto/
  ├── mod.rs                — re-exports + module docs
  ├── hash.rs               — SHA-256, SHA-512, BLAKE3 (one-shot + streaming)
  ├── hmac.rs               — HMAC-SHA-256, HMAC-SHA-512, constant-time `subtle_eq`
  ├── rand.rs               — `crypto.rand` CSPRNG (OS entropy via getrandom)
  ├── aes_gcm.rs            — AES-256-GCM AEAD (NIST CAVP-tested)
  └── chacha20_poly1305.rs  — ChaCha20-Poly1305 AEAD (RFC 8439-tested)
```

All five modules are thin wrappers around audited RustCrypto crates
(`sha2`, `hmac`, `aes-gcm`, `chacha20poly1305`) with a Mighty-shaped
surface on top. The wrapper layer:

- normalises argument shapes (`&[u8; 32]` keys, `&[u8; 12]` nonces)
- returns simple `Vec<u8>` / `[u8; N]` rather than the crates' typed
  newtype wrappers (which don't read cleanly from Mighty source)
- collapses error variants into a small enum per module

## Capability model

| Surface | Capability | Why |
|---|---|---|
| `hash::*` | none | Pure function of input |
| `hmac::*` | none | Pure function of (key, message) |
| `rand::*` | `crypto.rand` | Reads OS entropy — sandbox-relevant |
| `aes_gcm::*` | none | Pure function of (key, nonce, aad, payload) |
| `chacha20_poly1305::*` | none | Pure function of (key, nonce, aad, payload) |

Note that AEAD encrypt is **pure** even though typical callers pair
it with a fresh random nonce — that nonce comes from `rand::random_bytes`
which DOES require `crypto.rand`. Surfacing the entropy gate on the
nonce-generation side keeps the AEAD function itself deterministic.

## AEAD: AES-256-GCM and ChaCha20-Poly1305

Both AEAD surfaces share an identical shape on purpose — callers can
pick the cipher by changing one function name:

```rust
let ct: Vec<u8> = aes_gcm_256_encrypt(&key32, &nonce12, aad, plaintext)?;
let pt: Vec<u8> = aes_gcm_256_decrypt(&key32, &nonce12, aad, &ct)?;

let ct: Vec<u8> = chacha20_poly1305_encrypt(&key32, &nonce12, aad, plaintext)?;
let pt: Vec<u8> = chacha20_poly1305_decrypt(&key32, &nonce12, aad, &ct)?;
```

### Ciphertext layout

Both surfaces return `plaintext_len + 16` bytes:

```
+--------------------+--------------------+
| ciphertext (len N) |   auth tag (16 B)  |
+--------------------+--------------------+
```

The 128-bit auth tag is appended by `aes-gcm` / `chacha20poly1305`
crates. Decrypt verifies the tag in constant time before returning
plaintext; any tamper yields `AeadErr::Decrypt`.

### Security properties

| Property | Notes |
|---|---|
| Key length | 256 bits. 128-bit AES is acceptable but the new stdlib only ships AES-256-GCM. |
| Nonce length | 96 bits (12 bytes). MUST be unique per `(key, message)`. |
| Tag length | 128 bits. Fixed — not configurable. |
| AAD | Bound into the tag; the decryptor must supply the same AAD bytes or decrypt fails. |

### Nonce reuse — the #1 footgun

GCM and ChaCha20-Poly1305 both **catastrophically break** under nonce
reuse with the same key:

- Two messages encrypted under the same `(key, nonce)` reveal the XOR
  of their plaintexts.
- For GCM specifically, the attacker can additionally recover the
  GHASH authentication key, forging arbitrary ciphertexts.

Safe nonce strategies (in order of preference):

1. **Counter** — bump a 96-bit sequence number per message. Persist
   the counter in stable storage. Recommended for any single-writer
   workload.
2. **Random** — `crypto.rand` 12 random bytes. Collision probability
   `≈ 2^-32` after `2^32` messages (birthday bound) — safe for cookies/
   sessions where you'd rotate the key long before then.
3. **Deterministic** — derived from a hash of the plaintext (SIV
   construction). Not currently surfaced; v0.41 may add AES-GCM-SIV.

If you need a randomly safe nonce above `2^32` messages, use
XChaCha20-Poly1305 (192-bit nonce). Not surfaced in v0.40 — follow-up
slot.

### AAD usage

AAD is data that's NOT encrypted but IS bound into the auth tag. Use
it to commit the ciphertext to:

- a version number (`b"v=1"`) so a future key-rotation rollout can
  refuse old-version ciphertext
- a key id (`b"key-id=primary"`) so decrypting under the wrong key
  fails noisily
- a request id, session id, etc.

The decryptor must pass the same AAD bytes. Different AAD → decrypt
fails (intentionally opaque error — no distinguishing "wrong AAD"
from "wrong key" so we don't leak oracles).

### Key derivation

The AEAD surface accepts a 32-byte symmetric key directly — it does
not derive it. Common derivation patterns:

```rust
// HKDF-like: HMAC-SHA-256 over (info, master_key)
let session_key = hmac_sha256(&master_key, b"session/v1/aes-gcm");
// session_key is 32 bytes — feed straight into aes_gcm_256_encrypt.
```

For password-derived keys, v0.40 does NOT ship PBKDF2 / scrypt /
Argon2 — that's a v0.41 follow-up. Until then, callers can build
PBKDF2-HMAC-SHA-256 directly on top of `hmac_sha256`.

## Test coverage

Total: 75 tests across the crypto stack.

| Module | Tests | KAT source |
|---|---|---|
| `hash` | 16 | NIST FIPS 180-4 (SHA-256/512), BLAKE3 reference suite |
| `hmac` | 13 | RFC 4231 |
| `rand` | 11 | Statistical sanity (range, distribution, mean) |
| `aes_gcm` | 15 | NIST CAVP `gcmEncryptExtIV256.rsp` (Count=0 / PT0/AAD0, PT128/AAD0) + tamper coverage |
| `chacha20_poly1305` | 12 | RFC 8439 §2.8.2 + Appendix A.5 + tamper coverage |

The AEAD modules pin RFC / NIST vectors bit-for-bit:

- **AES-GCM**: NIST CAVP test vectors from `gcmEncryptExtIV256.rsp`
  (Keylen=256, IVlen=96, Taglen=128, two `[PTlen, AADlen]` shapes).
- **ChaCha20-Poly1305**: RFC 8439 §2.8.2 "Ladies and Gentlemen of the
  class of '99" KAT (the canonical vector every implementation gets
  validated against) plus the Appendix A.5 longer vector for round-trip
  coverage.

Tamper coverage exercises every failure mode for both surfaces:
ciphertext bit-flip, tag bit-flip, wrong AAD, wrong key, wrong nonce,
truncation, empty input.

## Deps added in v0.40 T4

| Crate | Version | Purpose |
|---|---|---|
| `aes-gcm` | 0.10 | RustCrypto AES-256-GCM |
| `chacha20poly1305` | 0.10 | RustCrypto ChaCha20-Poly1305 (RFC 8439) |

Both pull in `aead` 0.5 (the trait crate) and `cipher` 0.4 (the block
cipher trait). AES-NI is auto-detected at runtime via `cpufeatures`
(already in the lockfile). On ARM, both ciphers fall back to portable
constant-time implementations.

## v0.41 follow-ups

- **Ed25519 signing** — `std.crypto.ed25519.{sign, verify, keygen}`.
  RustCrypto ships `ed25519-dalek` (audited, constant-time).
- **ECDH key exchange** — `std.crypto.x25519.{keygen, dh}` on top of
  the same `curve25519-dalek` core. Enables Noise-style handshakes.
- **PBKDF2 / Argon2 password KDFs** — `std.crypto.kdf.pbkdf2_sha256`
  and `std.crypto.kdf.argon2id`. RustCrypto ships both.
- **XChaCha20-Poly1305** — 192-bit nonce for randomly-safe nonces
  above the GCM/ChaCha20 birthday bound.
- **AES-GCM-SIV** — deterministic AEAD (RFC 8452) for use cases that
  can't reliably ensure nonce uniqueness.
