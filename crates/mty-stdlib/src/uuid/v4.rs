//! UUID v4 — random.
//!
//! RFC 9562 § 5.4: 16 random bytes with two fields rewritten.
//!
//! - Byte 6 high nibble = `0100` (version 4)
//! - Byte 8 high two bits = `10` (RFC 4122 variant)

use super::{Uuid, UuidErr};
use crate::crypto::rand::random_bytes;

/// Generate a v4 UUID. Requires `crypto.rand`.
pub fn generate() -> Result<Uuid, UuidErr> {
    let raw = random_bytes(16)?;
    let mut b = [0u8; 16];
    b.copy_from_slice(&raw);
    // Version 4 — high nibble of byte 6.
    b[6] = (b[6] & 0x0f) | 0x40;
    // RFC 4122 variant — top two bits of byte 8 must be `10`.
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(Uuid { bytes: b })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v4_sets_version_nibble() {
        let u = generate().unwrap();
        assert_eq!(u.version(), 4, "{}", u);
    }

    #[test]
    fn v4_sets_variant_bits() {
        let u = generate().unwrap();
        // High two bits of byte 8 must be 10xx_xxxx.
        let top = u.bytes[8] & 0xc0;
        assert_eq!(top, 0x80, "byte8 = {:02x}", u.bytes[8]);
    }

    #[test]
    fn v4_two_draws_differ() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn v4_round_trips_via_string() {
        let u = generate().unwrap();
        let s = u.to_string();
        let parsed = Uuid::parse(&s).unwrap();
        assert_eq!(u, parsed);
    }

    #[test]
    fn v4_string_shape() {
        let s = generate().unwrap().to_string();
        // 36 chars, dashes at 8/13/18/23.
        assert_eq!(s.len(), 36);
        assert_eq!(s.as_bytes()[8], b'-');
        assert_eq!(s.as_bytes()[13], b'-');
        assert_eq!(s.as_bytes()[18], b'-');
        assert_eq!(s.as_bytes()[23], b'-');
        // Version digit at index 14 must be '4'.
        assert_eq!(s.as_bytes()[14], b'4');
    }
}
