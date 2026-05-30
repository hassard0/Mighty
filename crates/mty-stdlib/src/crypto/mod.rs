//! `std.crypto` — cryptographic primitives for real Mighty services.
//!
//! v0.39 T1 ships four submodules:
//!
//! - [`hash`] — SHA-256, SHA-512, BLAKE3 over byte slices and streams.
//! - [`hmac`] — HMAC-SHA-256 and HMAC-SHA-512.
//! - [`rand`] — CSPRNG bytes and uniform sampling, backed by the OS
//!   entropy source via [`getrandom`]. Capability-gated as `crypto.rand`.
//!
//! Hashing and HMAC are pure functions of their inputs — they need no
//! capability. The PRNG surface is the one place we *do* require an
//! entropy capability (`crypto.rand`) so a future sandbox profile can
//! refuse to hand a non-deterministic stream to untrusted Mighty code.
//!
//! Mighty surface:
//!
//! ```ignore
//! use std.crypto.{sha256, hmac_sha256, random_bytes};
//!
//! let h: [U8; 32] = sha256(b"hello");
//! let mac: [U8; 32] = hmac_sha256(key, message);
//! let nonce: [U8; 16] = random_bytes(16);
//! ```

pub mod hash;
pub mod hmac;
pub mod rand;

// Convenience re-exports for the canonical surface used in Mighty source
// and the host dispatcher.
pub use hash::{blake3, sha256, sha512, Sha256Hasher, Sha512Hasher};
pub use hmac::{hmac_sha256, hmac_sha512};
pub use rand::{random_bytes, uniform_f64, uniform_int, RandErr};
