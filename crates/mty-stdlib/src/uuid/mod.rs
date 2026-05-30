//! `std.uuid` — RFC 9562 UUID values.
//!
//! Two generators ship in v0.39 T1:
//!
//! - [`v4`] — fully random (122 bits of entropy + 4 bits version
//!   tag + 2 bits variant tag). The default choice for opaque IDs.
//! - [`v7`] — time-ordered (48-bit Unix millisecond timestamp + 12 bits
//!   sub-millisecond counter / random + 62 bits random). Sortable by
//!   creation order while remaining globally unique. Use for database
//!   primary keys where insertion order matters (BTree locality).
//!
//! Both generators draw entropy from [`crate::crypto::rand`] and so
//! require the `crypto.rand` capability when called from sandboxed
//! Mighty code. The struct itself is plain data — parse / format are
//! pure functions.
//!
//! ```ignore
//! use std.uuid.Uuid;
//!
//! let id: Uuid = Uuid.v4();
//! let s: Str = id.to_string();
//! let parsed: Uuid = Uuid.parse(s)?;
//! let sortable: Uuid = Uuid.v7();
//! ```

pub mod v4;
pub mod v7;

use crate::crypto::rand::RandErr;

/// A 128-bit UUID. Layout matches RFC 9562 § 4 — the byte order is
/// big-endian network order regardless of host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid {
    pub bytes: [u8; 16],
}

#[derive(Debug, thiserror::Error)]
pub enum UuidErr {
    #[error("invalid uuid string: {0}")]
    Parse(String),
    #[error("entropy: {0}")]
    Entropy(#[from] RandErr),
}

impl Uuid {
    /// Generate a random version-4 UUID. Requires `crypto.rand`.
    pub fn v4() -> Result<Self, UuidErr> {
        v4::generate()
    }

    /// Generate a time-ordered version-7 UUID. Requires `crypto.rand`.
    pub fn v7() -> Result<Self, UuidErr> {
        v7::generate()
    }

    /// Construct from raw bytes (does not enforce version/variant).
    /// Use [`Self::v4`] / [`Self::v7`] instead unless you're rebuilding
    /// from a wire format.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self { bytes }
    }

    /// Nil UUID — all 16 bytes zero. Useful as a sentinel.
    #[must_use]
    pub fn nil() -> Self {
        Self { bytes: [0; 16] }
    }

    /// True iff this is the nil UUID.
    #[must_use]
    pub fn is_nil(&self) -> bool {
        self.bytes.iter().all(|b| *b == 0)
    }

    /// UUID version digit per RFC 9562 (the high nibble of byte 6).
    #[must_use]
    pub fn version(&self) -> u8 {
        self.bytes[6] >> 4
    }

    /// Parse the standard 8-4-4-4-12 dash-separated form.
    pub fn parse(s: &str) -> Result<Self, UuidErr> {
        // Accept the canonical hyphenated form: 36 chars total,
        // dashes at positions 8, 13, 18, 23.
        let s = s.trim();
        if s.len() != 36 {
            return Err(UuidErr::Parse(format!(
                "expected 36 chars, got {}",
                s.len()
            )));
        }
        let bytes = s.as_bytes();
        if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
            return Err(UuidErr::Parse(
                "dashes must be at positions 8, 13, 18, 23".into(),
            ));
        }
        let mut out = [0u8; 16];
        let mut byte_i = 0;
        let mut i = 0;
        while i < 36 {
            if bytes[i] == b'-' {
                i += 1;
                continue;
            }
            let hi = nibble(bytes[i]).ok_or_else(|| {
                UuidErr::Parse(format!(
                    "non-hex char at index {}: {:?}",
                    i, bytes[i] as char
                ))
            })?;
            let lo = nibble(bytes[i + 1]).ok_or_else(|| {
                UuidErr::Parse(format!(
                    "non-hex char at index {}: {:?}",
                    i + 1,
                    bytes[i + 1] as char
                ))
            })?;
            out[byte_i] = (hi << 4) | lo;
            byte_i += 1;
            i += 2;
        }
        Ok(Self { bytes: out })
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = &self.bytes;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-\
             {:02x}{:02x}-\
             {:02x}{:02x}-\
             {:02x}{:02x}-\
             {:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0],
            b[1],
            b[2],
            b[3],
            b[4],
            b[5],
            b[6],
            b[7],
            b[8],
            b[9],
            b[10],
            b[11],
            b[12],
            b[13],
            b[14],
            b[15]
        )
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_is_nil() {
        let u = Uuid::nil();
        assert!(u.is_nil());
        assert_eq!(u.to_string(), "00000000-0000-0000-0000-000000000000");
    }

    #[test]
    fn parse_canonical_form() {
        let s = "550e8400-e29b-41d4-a716-446655440000";
        let u = Uuid::parse(s).unwrap();
        assert_eq!(u.to_string(), s);
        assert_eq!(u.version(), 4);
    }

    #[test]
    fn parse_uppercase() {
        let s = "550E8400-E29B-41D4-A716-446655440000";
        let u = Uuid::parse(s).unwrap();
        // Renders back as lowercase.
        assert_eq!(u.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn parse_rejects_wrong_length() {
        assert!(matches!(
            Uuid::parse("550e8400-e29b-41d4-a716"),
            Err(UuidErr::Parse(_))
        ));
        assert!(Uuid::parse("").is_err());
    }

    #[test]
    fn parse_rejects_missing_dashes() {
        let s = "550e8400e29b41d4a716446655440000abcd"; // 36 chars, no dashes
        assert!(matches!(Uuid::parse(s), Err(UuidErr::Parse(_))));
    }

    #[test]
    fn parse_rejects_non_hex_char() {
        let s = "550e8400-e29b-41d4-a716-44665544000z";
        assert!(matches!(Uuid::parse(s), Err(UuidErr::Parse(_))));
    }

    #[test]
    fn round_trip_parse_to_string() {
        let original = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
        let u = Uuid::parse(original).unwrap();
        assert_eq!(u.to_string(), original);
    }

    #[test]
    fn from_bytes_round_trip() {
        let bytes = [
            0xf4, 0x7a, 0xc1, 0x0b, 0x58, 0xcc, 0x43, 0x72, 0xa5, 0x67, 0x0e, 0x02, 0xb2, 0xc3,
            0xd4, 0x79,
        ];
        let u = Uuid::from_bytes(bytes);
        assert_eq!(u.bytes, bytes);
        let s = u.to_string();
        let parsed = Uuid::parse(&s).unwrap();
        assert_eq!(parsed, u);
    }

    #[test]
    fn display_matches_to_string() {
        let u = Uuid::from_bytes([0u8; 16]);
        assert_eq!(format!("{}", u), u.to_string());
    }

    #[test]
    fn trim_whitespace() {
        let s = "  550e8400-e29b-41d4-a716-446655440000  ";
        let u = Uuid::parse(s).unwrap();
        assert_eq!(u.to_string(), s.trim());
    }
}
