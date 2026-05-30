//! UUID v7 — time-ordered (RFC 9562 § 5.7).
//!
//! Layout (most significant first):
//!
//! ```text
//!   0                   1                   2                   3
//!   0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//!  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//!  |                          unix_ms_hi                           |
//!  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//!  |       unix_ms_lo              |  ver  |       rand_a          |
//!  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//!  |var|                        rand_b                             |
//!  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//!  |                            rand_b                             |
//!  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! - 48 bits = Unix epoch milliseconds (big-endian).
//! - 4 bits = version (`0111`).
//! - 12 bits = sub-millisecond entropy (`rand_a`).
//! - 2 bits = RFC 4122 variant (`10`).
//! - 62 bits = entropy (`rand_b`).
//!
//! Sorting v7 UUIDs lexicographically (or as big-endian byte arrays)
//! orders them by creation time — the property that makes v7 the
//! preferred database PK over v4.

use super::{Uuid, UuidErr};
use crate::crypto::rand::random_bytes;
use std::time::{SystemTime, UNIX_EPOCH};

/// Generate a v7 UUID. Requires `crypto.rand`.
pub fn generate() -> Result<Uuid, UuidErr> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    generate_with_timestamp(now_ms)
}

/// Generate a v7 UUID with an explicit timestamp. Public for tests + for
/// callers that want deterministic time-stamping (e.g. tracing with
/// recorded clocks).
pub fn generate_with_timestamp(unix_ms: u64) -> Result<Uuid, UuidErr> {
    let rand_bytes = random_bytes(10)?;
    let mut b = [0u8; 16];

    // 48-bit timestamp into bytes 0..6 (big-endian).
    let ts = unix_ms & 0xffff_ffff_ffff; // mask to 48 bits
    b[0] = (ts >> 40) as u8;
    b[1] = (ts >> 32) as u8;
    b[2] = (ts >> 24) as u8;
    b[3] = (ts >> 16) as u8;
    b[4] = (ts >> 8) as u8;
    b[5] = ts as u8;

    // bytes 6..16 = entropy, with version + variant rewritten.
    b[6] = rand_bytes[0];
    b[7] = rand_bytes[1];
    b[8] = rand_bytes[2];
    b[9] = rand_bytes[3];
    b[10] = rand_bytes[4];
    b[11] = rand_bytes[5];
    b[12] = rand_bytes[6];
    b[13] = rand_bytes[7];
    b[14] = rand_bytes[8];
    b[15] = rand_bytes[9];

    // Version 7 — high nibble of byte 6.
    b[6] = (b[6] & 0x0f) | 0x70;
    // RFC 4122 variant — top two bits of byte 8 must be `10`.
    b[8] = (b[8] & 0x3f) | 0x80;

    Ok(Uuid { bytes: b })
}

/// Extract the 48-bit Unix-ms timestamp from a v7 UUID.
#[must_use]
pub fn timestamp_ms(u: &Uuid) -> u64 {
    let b = &u.bytes;
    ((b[0] as u64) << 40)
        | ((b[1] as u64) << 32)
        | ((b[2] as u64) << 24)
        | ((b[3] as u64) << 16)
        | ((b[4] as u64) << 8)
        | (b[5] as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn v7_sets_version_nibble() {
        let u = generate().unwrap();
        assert_eq!(u.version(), 7, "{}", u);
    }

    #[test]
    fn v7_sets_variant_bits() {
        let u = generate().unwrap();
        let top = u.bytes[8] & 0xc0;
        assert_eq!(top, 0x80);
    }

    #[test]
    fn v7_two_draws_differ() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn v7_timestamp_round_trip() {
        // Pick a fixed UNIX ms — 2024-01-01T00:00:00Z = 1_704_067_200_000
        let ts = 1_704_067_200_000u64;
        let u = generate_with_timestamp(ts).unwrap();
        assert_eq!(timestamp_ms(&u), ts);
    }

    #[test]
    fn v7_sorts_by_creation_order() {
        // Draw a UUID, sleep 2 ms, draw another. The earlier one must
        // compare-lexicographically less than the later one (byte order).
        let a = generate().unwrap();
        thread::sleep(Duration::from_millis(5));
        let b = generate().unwrap();
        assert!(
            a.bytes < b.bytes,
            "a = {} ({:?})\nb = {} ({:?})",
            a,
            a.bytes,
            b,
            b.bytes
        );
    }

    #[test]
    fn v7_string_shape() {
        let s = generate().unwrap().to_string();
        assert_eq!(s.len(), 36);
        assert_eq!(s.as_bytes()[14], b'7');
    }

    #[test]
    fn v7_round_trips_via_string() {
        let u = generate().unwrap();
        let parsed = Uuid::parse(&u.to_string()).unwrap();
        assert_eq!(u, parsed);
    }

    #[test]
    fn v7_with_zero_timestamp() {
        let u = generate_with_timestamp(0).unwrap();
        assert_eq!(timestamp_ms(&u), 0);
        assert_eq!(u.version(), 7);
    }

    #[test]
    fn v7_max_48bit_timestamp() {
        // 2^48 - 1 is the largest representable timestamp.
        let ts = (1u64 << 48) - 1;
        let u = generate_with_timestamp(ts).unwrap();
        assert_eq!(timestamp_ms(&u), ts);
    }

    #[test]
    fn v7_truncates_timestamps_above_48_bits() {
        // Bits above 48 are dropped per RFC 9562 § 5.7.
        let ts = (1u64 << 48) | 0x12_3456;
        let u = generate_with_timestamp(ts).unwrap();
        assert_eq!(timestamp_ms(&u), 0x12_3456);
    }
}
