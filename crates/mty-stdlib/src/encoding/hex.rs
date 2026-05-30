//! Hex (base16) — RFC 4648 § 8.
//!
//! Encodes/decodes byte slices to and from hex strings. Both lowercase
//! and uppercase emit functions are exposed; the decoder accepts either
//! case interchangeably.
//!
//! Implemented inline rather than going through the workspace's `hex`
//! crate so the public surface matches the rest of `std.encoding` and
//! we don't expose the crate's quirks (the workspace `hex` crate doesn't
//! provide an explicit uppercase emitter).

#[derive(Debug, thiserror::Error)]
pub enum HexErr {
    #[error("invalid hex character {0:?} at index {1}")]
    BadChar(char, usize),
    #[error("hex string must have even length, got {0}")]
    OddLength(usize),
}

/// Lowercase hex encode. Returns a 2 × N string.
///
/// ```
/// # use mty_stdlib::encoding::hex::encode;
/// assert_eq!(encode(b"\xde\xad\xbe\xef"), "deadbeef");
/// ```
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    static LUT: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

/// Uppercase hex encode. Same as [`encode`] but `0-9 A-F`.
#[must_use]
pub fn encode_upper(bytes: &[u8]) -> String {
    static LUT: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(LUT[(b >> 4) as usize] as char);
        s.push(LUT[(b & 0x0f) as usize] as char);
    }
    s
}

/// Decode a hex string into bytes. Accepts mixed case. Rejects odd
/// length and non-hex characters.
pub fn decode(s: &str) -> Result<Vec<u8>, HexErr> {
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(HexErr::OddLength(bytes.len()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks(2) {
        let hi = nibble(chunk[0]).ok_or_else(|| HexErr::BadChar(chunk[0] as char, 0))?;
        let lo = nibble(chunk[1]).ok_or_else(|| HexErr::BadChar(chunk[1] as char, 1))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
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

    // ---- RFC 4648 § 10 base16 test vectors ----

    #[test]
    fn rfc4648_vectors_encode_upper() {
        assert_eq!(encode_upper(b""), "");
        assert_eq!(encode_upper(b"f"), "66");
        assert_eq!(encode_upper(b"fo"), "666F");
        assert_eq!(encode_upper(b"foo"), "666F6F");
        assert_eq!(encode_upper(b"foob"), "666F6F62");
        assert_eq!(encode_upper(b"fooba"), "666F6F6261");
        assert_eq!(encode_upper(b"foobar"), "666F6F626172");
    }

    #[test]
    fn rfc4648_vectors_encode_lower() {
        // RFC 4648 prefers uppercase but lowercase is universal in
        // hash-id contexts (git sha-1, sha-256 digests…).
        assert_eq!(encode(b"foobar"), "666f6f626172");
    }

    #[test]
    fn rfc4648_vectors_decode() {
        assert_eq!(decode("").unwrap(), b"");
        assert_eq!(decode("66").unwrap(), b"f");
        assert_eq!(decode("666f6f626172").unwrap(), b"foobar");
        assert_eq!(decode("666F6F626172").unwrap(), b"foobar");
    }

    #[test]
    fn decode_mixed_case() {
        assert_eq!(decode("DeAdBeEf").unwrap(), [0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn round_trip_all_bytes() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let s = encode(&bytes);
        assert_eq!(decode(&s).unwrap(), bytes);
    }

    #[test]
    fn round_trip_random_bytes() {
        let bytes: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        let s = encode(&bytes);
        let d = decode(&s).unwrap();
        assert_eq!(d, bytes);
    }

    #[test]
    fn empty_round_trips() {
        assert_eq!(encode(b""), "");
        assert_eq!(decode("").unwrap(), b"");
    }

    #[test]
    fn deadbeef() {
        assert_eq!(encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(encode_upper(&[0xde, 0xad, 0xbe, 0xef]), "DEADBEEF");
    }

    #[test]
    fn decode_rejects_odd_length() {
        let err = decode("abc").unwrap_err();
        assert!(matches!(err, HexErr::OddLength(3)));
    }

    #[test]
    fn decode_rejects_bad_char() {
        let err = decode("0g").unwrap_err();
        assert!(matches!(err, HexErr::BadChar('g', _)));
    }

    #[test]
    fn decode_rejects_whitespace() {
        // We intentionally don't accept whitespace — most uses (digest
        // comparison) want strict input. Tests that feed "ab cd" should
        // sanitize first.
        assert!(decode("ab cd").is_err());
    }

    #[test]
    fn encode_then_decode_sha256_digest_shape() {
        // 32-byte digest round-trips to a 64-char string.
        let digest = [0xa5u8; 32];
        let s = encode(&digest);
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(decode(&s).unwrap(), digest);
    }

    #[test]
    fn single_byte_all_values() {
        for b in 0u8..=255 {
            let s = encode(&[b]);
            assert_eq!(s.len(), 2);
            assert_eq!(decode(&s).unwrap(), vec![b]);
        }
    }

    #[test]
    fn upper_and_lower_decode_to_same() {
        let bytes = [0x12, 0xab, 0xcd, 0xef];
        let upper = encode_upper(&bytes);
        let lower = encode(&bytes);
        assert_eq!(decode(&upper).unwrap(), decode(&lower).unwrap());
    }

    #[test]
    fn error_messages_describe_the_problem() {
        let odd = format!("{}", decode("abc").unwrap_err());
        assert!(odd.contains("even length"), "msg: {}", odd);
        let bad = format!("{}", decode("0x").unwrap_err());
        assert!(bad.contains("invalid hex"), "msg: {}", bad);
    }

    #[test]
    fn encode_is_lowercase_by_default() {
        let bytes = [0xab, 0xcd];
        let s = encode(&bytes);
        assert!(s
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
    }
}
