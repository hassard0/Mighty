//! Cryptographic random — `crypto.rand` capability.
//!
//! This is the only `std.crypto` surface that requires a capability:
//! anything that reads OS entropy gets gated behind `crypto.rand` so a
//! sandbox can refuse to give non-deterministic output to untrusted
//! Mighty code. The underlying source is [`getrandom`] which routes to
//! the platform CSPRNG (Linux `getrandom(2)`, macOS `getentropy(2)`,
//! Windows `BCryptGenRandom`).
//!
//! For uniform integer sampling we use rejection sampling on the OS
//! entropy stream — this avoids the bias that comes from `rand % range`.

#[derive(Debug, thiserror::Error)]
pub enum RandErr {
    #[error("os rng: {0}")]
    Os(String),
    #[error("empty range: low >= high")]
    EmptyRange,
}

/// Pull `n` cryptographically-secure random bytes from the OS CSPRNG.
///
/// Requires `crypto.rand`. Calling this from inside a sandbox profile
/// that does NOT grant `crypto.rand` will be a future host-side denial
/// — the function itself just performs the syscall.
pub fn random_bytes(n: usize) -> Result<Vec<u8>, RandErr> {
    let mut v = vec![0u8; n];
    getrandom::getrandom(&mut v).map_err(|e| RandErr::Os(e.to_string()))?;
    Ok(v)
}

/// Uniform-random `i64` in `[low, high)`. Rejects out-of-range draws to
/// avoid modulo bias.
///
/// Returns `RandErr::EmptyRange` if `low >= high`.
pub fn uniform_int(low: i64, high: i64) -> Result<i64, RandErr> {
    if low >= high {
        return Err(RandErr::EmptyRange);
    }
    let range = (high as i128 - low as i128) as u128;
    // Smallest unsigned type that holds the range.
    // We use u128 throughout and rejection-sample against the largest
    // multiple of `range` that fits.
    let limit = u128::MAX - (u128::MAX % range);
    loop {
        let mut buf = [0u8; 16];
        getrandom::getrandom(&mut buf).map_err(|e| RandErr::Os(e.to_string()))?;
        let draw = u128::from_le_bytes(buf);
        if draw < limit {
            let offset = (draw % range) as i128;
            return Ok((low as i128 + offset) as i64);
        }
    }
}

/// Uniform-random `f64` in `[0.0, 1.0)`.
///
/// Implementation: draw 53 random bits, interpret as the mantissa of an
/// `f64` in `[1.0, 2.0)`, then subtract 1.0. This gives every
/// representable `f64` in `[0.0, 1.0)` whose mantissa is a 53-bit
/// integer equal probability.
pub fn uniform_f64() -> Result<f64, RandErr> {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).map_err(|e| RandErr::Os(e.to_string()))?;
    let bits = u64::from_le_bytes(buf);
    // Take top 53 bits.
    let mantissa = bits >> 11;
    Ok(mantissa as f64 * (1.0_f64 / ((1u64 << 53) as f64)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_bytes_len_matches_request() {
        let v = random_bytes(64).unwrap();
        assert_eq!(v.len(), 64);
    }

    #[test]
    fn random_bytes_zero_returns_empty() {
        assert!(random_bytes(0).unwrap().is_empty());
    }

    #[test]
    fn random_bytes_two_draws_differ() {
        // 32 bytes collision probability ~2^-256 — never in this universe.
        let a = random_bytes(32).unwrap();
        let b = random_bytes(32).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn random_bytes_large_buffer() {
        // 4 KiB exercises the > buffer-size code path on platforms that
        // chunk syscalls (getrandom on Linux limits to 256 bytes per call
        // for old kernels — the crate handles the loop).
        let v = random_bytes(4096).unwrap();
        assert_eq!(v.len(), 4096);
        // Sanity: not all-zero. With true entropy P(all zero) is 2^-32768.
        assert!(v.iter().any(|b| *b != 0));
    }

    #[test]
    fn uniform_int_stays_in_range() {
        for _ in 0..1000 {
            let n = uniform_int(0, 10).unwrap();
            assert!((0..10).contains(&n), "out of range: {}", n);
        }
    }

    #[test]
    fn uniform_int_negative_range() {
        for _ in 0..1000 {
            let n = uniform_int(-50, 50).unwrap();
            assert!((-50..50).contains(&n), "out of range: {}", n);
        }
    }

    #[test]
    fn uniform_int_single_value_range() {
        let n = uniform_int(7, 8).unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn uniform_int_rejects_empty_range() {
        assert!(matches!(uniform_int(5, 5), Err(RandErr::EmptyRange)));
        assert!(matches!(uniform_int(10, 0), Err(RandErr::EmptyRange)));
    }

    #[test]
    fn uniform_int_covers_distribution() {
        // 10 buckets × 1000 samples — every bucket should have at least
        // one hit with overwhelming probability.
        let mut hits = [false; 10];
        for _ in 0..1000 {
            let n = uniform_int(0, 10).unwrap();
            hits[n as usize] = true;
        }
        assert!(hits.iter().all(|h| *h), "distribution missed a bucket");
    }

    #[test]
    fn uniform_f64_stays_in_range() {
        for _ in 0..1000 {
            let f = uniform_f64().unwrap();
            assert!((0.0..1.0).contains(&f), "out of range: {}", f);
        }
    }

    #[test]
    fn uniform_f64_two_draws_differ() {
        let a = uniform_f64().unwrap();
        let b = uniform_f64().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn uniform_f64_mean_in_band() {
        // Average of 10k uniform draws should land within [0.45, 0.55].
        let sum: f64 = (0..10_000).map(|_| uniform_f64().unwrap()).sum();
        let mean = sum / 10_000.0;
        assert!((0.45..0.55).contains(&mean), "mean out of band: {}", mean);
    }
}
