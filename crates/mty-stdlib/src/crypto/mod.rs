//! `std.crypto` — cryptographic primitives for real Mighty services.
//!
//! v0.39 T1 shipped three foundational submodules — `hash`, `hmac`,
//! `rand`. v0.40 T4 closes the AEAD gap with two more:
//!
//! - [`hash`] — SHA-256, SHA-512, BLAKE3 over byte slices and streams.
//! - [`hmac`] — HMAC-SHA-256 and HMAC-SHA-512.
//! - [`rand`] — CSPRNG bytes and uniform sampling, backed by the OS
//!   entropy source via [`getrandom`]. Capability-gated as `crypto.rand`.
//! - [`aes_gcm`] — AES-256-GCM authenticated encryption (NIST CAVP-tested).
//! - [`chacha20_poly1305`] — ChaCha20-Poly1305 AEAD (RFC 8439-tested).
//!
//! Hashing, HMAC, and the AEAD surfaces are pure functions of their
//! inputs — they need no capability. The PRNG surface is the one place
//! we *do* require an entropy capability (`crypto.rand`) so a future
//! sandbox profile can refuse to hand a non-deterministic stream to
//! untrusted Mighty code.
//!
//! Mighty surface:
//!
//! ```ignore
//! use std.crypto.{sha256, hmac_sha256, random_bytes};
//! use std.crypto.aes_gcm.{encrypt, decrypt};
//!
//! let h: [U8; 32] = sha256(b"hello");
//! let mac: [U8; 32] = hmac_sha256(key, message);
//! let nonce: [U8; 12] = random_bytes(12);
//! let ct: Vec<U8> = encrypt(&key32, &nonce, b"v=1", b"payload")?;
//! ```

pub mod aes_gcm;
pub mod chacha20_poly1305;
pub mod hash;
pub mod hmac;
pub mod rand;

// Convenience re-exports for the canonical surface used in Mighty source
// and the host dispatcher.
pub use aes_gcm::{aes_gcm_256_decrypt, aes_gcm_256_encrypt, AeadErr};
pub use chacha20_poly1305::{chacha20_poly1305_decrypt, chacha20_poly1305_encrypt};
pub use hash::{blake3, sha256, sha512, Sha256Hasher, Sha512Hasher};
pub use hmac::{hmac_sha256, hmac_sha512};
pub use rand::{random_bytes, uniform_f64, uniform_int, RandErr};
