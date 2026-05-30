//! Percent-encoding — RFC 3986 § 2.
//!
//! Two flavours:
//!
//! - [`percent_encode`] — encodes anything that is NOT an "unreserved"
//!   character per RFC 3986 (`A-Z a-z 0-9 - _ . ~`). Use this for full
//!   string-to-percent-encoded conversion (the kind you put inside a
//!   query parameter value).
//! - [`percent_encode_component`] — same but also encodes `/`, useful
//!   for path components that contain slashes that should be embedded
//!   literally rather than interpreted as path separators.

/// True for "unreserved" characters per RFC 3986 § 2.3.
fn unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~')
}

/// Percent-encode `s` per RFC 3986. Bytes are emitted as their UTF-8
/// representation, and any non-unreserved byte becomes `%HH`.
///
/// `percent_encode` keeps `/` literal — it's intended for whole
/// query-parameter values where path-style slashes are expected. Use
/// [`percent_encode_component`] when `/` must also be escaped.
///
/// ```
/// # use mty_stdlib::url::encode::percent_encode;
/// assert_eq!(percent_encode("hello world"), "hello%20world");
/// assert_eq!(percent_encode("a+b/c"), "a%2Bb/c");
/// ```
#[must_use]
pub fn percent_encode(s: &str) -> String {
    encode_with(s.as_bytes(), |b| unreserved(b) || b == b'/')
}

/// Percent-encode a single URL component — like [`percent_encode`] but
/// also encodes `/`.
#[must_use]
pub fn percent_encode_component(s: &str) -> String {
    encode_with(s.as_bytes(), unreserved)
}

fn encode_with(bytes: &[u8], keep: impl Fn(u8) -> bool) -> String {
    static HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if keep(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

/// Decode a percent-encoded string. Returns `None` if the input
/// contains a malformed `%XX` triple. The decoded bytes are returned
/// as a UTF-8 string if they parse as such; if the decoded bytes are
/// not valid UTF-8, lossy replacement is applied.
#[must_use]
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_nibble(bytes[i + 1])?;
            let lo = hex_nibble(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else if b == b'+' {
            // application/x-www-form-urlencoded uses "+" for space — we
            // intentionally do NOT decode "+" as space here because RFC
            // 3986 percent-encoding is the broader spec. Callers that
            // want form-decoding can replace "+" with " " upstream.
            out.push(b'+');
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

fn hex_nibble(c: u8) -> Option<u8> {
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
    fn space_becomes_percent_20() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
    }

    #[test]
    fn unreserved_passthrough() {
        let s = "ABCdef-_.~123";
        assert_eq!(percent_encode(s), s);
    }

    #[test]
    fn reserved_chars_encoded() {
        // RFC 3986 § 2.2 reserved chars not whitelisted by us:
        // : / ? # [ ] @ ! $ & ' ( ) * + , ; =
        assert_eq!(percent_encode_component("a+b"), "a%2Bb");
        assert_eq!(percent_encode_component("a&b"), "a%26b");
        assert_eq!(percent_encode_component("a=b"), "a%3Db");
    }

    #[test]
    fn slash_kept_by_path_encoder_dropped_by_component() {
        assert_eq!(percent_encode("a/b"), "a/b");
        assert_eq!(percent_encode_component("a/b"), "a%2Fb");
    }

    #[test]
    fn utf8_input_byte_by_byte() {
        // "é" = 0xC3 0xA9 in UTF-8.
        assert_eq!(percent_encode("é"), "%C3%A9");
        // "💪" = 0xF0 0x9F 0x92 0xAA
        assert_eq!(percent_encode("💪"), "%F0%9F%92%AA");
    }

    #[test]
    fn decode_round_trips_ascii() {
        let s = "hello world";
        let enc = percent_encode(s);
        assert_eq!(percent_decode(&enc).unwrap(), s);
    }

    #[test]
    fn decode_round_trips_utf8() {
        let s = "café 💪";
        let enc = percent_encode(s);
        assert_eq!(percent_decode(&enc).unwrap(), s);
    }

    #[test]
    fn decode_handles_uppercase_and_lowercase_hex() {
        assert_eq!(percent_decode("%2f%2F").unwrap(), "//");
    }

    #[test]
    fn decode_rejects_truncated_triple() {
        assert!(percent_decode("ab%2").is_none());
        assert!(percent_decode("ab%").is_none());
    }

    #[test]
    fn decode_rejects_bad_hex() {
        assert!(percent_decode("%XZ").is_none());
    }

    #[test]
    fn decode_preserves_plus_sign() {
        // Plus is NOT decoded as space — RFC 3986 doesn't say so.
        // Callers wanting form-decoding handle that themselves.
        assert_eq!(percent_decode("a+b").unwrap(), "a+b");
    }

    #[test]
    fn decode_empty_string() {
        assert_eq!(percent_decode("").unwrap(), "");
    }

    #[test]
    fn encode_empty_string() {
        assert_eq!(percent_encode(""), "");
    }

    #[test]
    fn rfc_uses_uppercase_hex() {
        // RFC 3986 § 2.1 recommends uppercase A-F.
        let enc = percent_encode("ä"); // 0xC3 0xA4
        assert!(enc
            .chars()
            .all(|c| !c.is_ascii_lowercase() || c.is_ascii_digit()));
        assert_eq!(enc, "%C3%A4");
    }

    #[test]
    fn percent_encode_query_value_shape() {
        // Typical query-value encoding.
        let v = "hello world & foo=bar";
        let enc = percent_encode_component(v);
        assert_eq!(enc, "hello%20world%20%26%20foo%3Dbar");
    }
}
