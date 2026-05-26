//! `std.random` — cryptographically-secure random bytes.
//!
//! Host-side implementation backed by the OS RNG (`getrandom` on
//! Linux, `RtlGenRandom` on Windows, `getentropy` on macOS) via the
//! [`getrandom`] crate. The Mighty runtime exposes this as
//! `std.random.bytes(n)` / `std.random.u64()`.
//!
//! ## Backend dispatch (v0.14 P2 lowering)
//!
//! When a program is compiled with `--wasi=p2`, the Mighty codegen
//! emits these calls as a **direct** P2 import of
//! `wasi:random/random@0.2.3#get-random-bytes` instead of routing
//! through the `wasi_snapshot_preview1` adapter. See
//! [`P2_DIRECT_IMPORT_RANDOM_BYTES`] / [`P2_DIRECT_IMPORT_RANDOM_U64`]
//! below for the canonical import names — these are the same strings
//! `mty_codegen_wasm::P2DirectImport::RandomBytes` produces when the
//! codegen layer asks for a direct lowering.
//!
//! The runtime path here is unchanged — it's only the *Wasm import*
//! shape that switches between P1-via-adapter and direct P2.

#[derive(Debug, thiserror::Error)]
pub enum RandomErr {
    #[error("os rng: {0}")]
    Os(String),
}

/// Canonical P2 import name for `bytes()` when the program is built
/// with `--wasi=p2` and the v0.14 direct-lowering path is active.
/// Surfaced from the stdlib so test harnesses + codegen tests can
/// pattern-match on a single source of truth.
pub const P2_DIRECT_IMPORT_RANDOM_BYTES: (&str, &str) =
    ("wasi:random/random@0.2.3", "get-random-bytes");

/// Canonical P2 import name for `u64()`. Same caveat as
/// [`P2_DIRECT_IMPORT_RANDOM_BYTES`].
pub const P2_DIRECT_IMPORT_RANDOM_U64: (&str, &str) =
    ("wasi:random/random@0.2.3", "get-random-u64");

/// `std.random.bytes(n)` — return `n` cryptographically-secure
/// random bytes. Wraps [`getrandom::getrandom`] which routes to the
/// host OS entropy source.
pub fn bytes(n: usize) -> Result<Vec<u8>, RandomErr> {
    let mut v = vec![0u8; n];
    getrandom::getrandom(&mut v).map_err(|e| RandomErr::Os(e.to_string()))?;
    Ok(v)
}

/// `std.random.u64()` — return one cryptographically-secure
/// random `u64`. Convenience wrapper around [`bytes`].
pub fn u64() -> Result<u64, RandomErr> {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).map_err(|e| RandomErr::Os(e.to_string()))?;
    Ok(u64::from_le_bytes(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_returns_requested_len() {
        let v = bytes(32).unwrap();
        assert_eq!(v.len(), 32);
    }

    #[test]
    fn bytes_zero_is_empty() {
        assert!(bytes(0).unwrap().is_empty());
    }

    #[test]
    fn two_draws_differ() {
        // 16 random bytes colliding is ~2^-128 — for all intents
        // and purposes never.
        let a = bytes(16).unwrap();
        let b = bytes(16).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn u64_returns_nonzero_eventually() {
        // A single u64 *could* be zero (~1/2^64 odds), but two in a
        // row both being zero is effectively impossible. Pull two
        // draws and assert at least one differs from zero — this
        // primarily catches a "function never wrote to the buffer"
        // regression.
        let a = u64().unwrap();
        let b = u64().unwrap();
        assert!(a != 0 || b != 0);
    }

    #[test]
    fn p2_direct_import_constants_are_canonical() {
        assert_eq!(P2_DIRECT_IMPORT_RANDOM_BYTES.0, "wasi:random/random@0.2.3");
        assert_eq!(P2_DIRECT_IMPORT_RANDOM_BYTES.1, "get-random-bytes");
        assert_eq!(P2_DIRECT_IMPORT_RANDOM_U64.1, "get-random-u64");
    }
}
