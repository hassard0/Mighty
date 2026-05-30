//! Base64 — RFC 4648.
//!
//! Two alphabets:
//!
//! - **Standard** (§ 4) — `A-Z a-z 0-9 + /` with `=` padding. The
//!   canonical encoding used in most network protocols (HTTP Basic
//!   auth, SMTP MIME, etc.).
//! - **URL-safe** (§ 5) — `A-Z a-z 0-9 - _` with `=` padding. The
//!   variant used in JWT, browser `data:` URLs, and any other
//!   embed-in-URL context.
//!
//! Both encode/decode pairs round-trip arbitrary bytes; the decoder
//! also accepts the no-pad form for URL-safe (per JWT convention).

use ::base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use ::base64::Engine;

#[derive(Debug, thiserror::Error)]
pub enum Base64Err {
    #[error("invalid base64: {0}")]
    Decode(String),
}

/// Encode bytes as standard (§ 4) Base64. Always emits `=` padding.
///
/// ```
/// # use mty_stdlib::encoding::base64::encode;
/// assert_eq!(encode(b"hello"), "aGVsbG8=");
/// ```
#[must_use]
pub fn encode(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// Decode a standard (§ 4) Base64 string. Accepts both padded and
/// unpadded forms (some legacy emitters drop the trailing `=`).
pub fn decode(s: &str) -> Result<Vec<u8>, Base64Err> {
    // Try strict first, then fall back to allowing missing padding —
    // matches what most production code wants without silently masking
    // genuinely malformed input (alphabet errors still bubble up).
    match STANDARD.decode(s) {
        Ok(v) => Ok(v),
        Err(_) => ::base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(s)
            .map_err(|e| Base64Err::Decode(e.to_string())),
    }
}

/// Encode bytes as URL-safe (§ 5) Base64. Emits `=` padding by default.
///
/// ```
/// # use mty_stdlib::encoding::base64::encode_url;
/// // Bytes that would have `+` or `/` in standard alphabet:
/// let raw = [0xfb, 0xff, 0xbf];
/// assert_eq!(encode_url(&raw), "-_-_");
/// ```
#[must_use]
pub fn encode_url(bytes: &[u8]) -> String {
    URL_SAFE.encode(bytes)
}

/// Encode bytes as URL-safe Base64 with no `=` padding — the form used
/// by JWT / JWS.
#[must_use]
pub fn encode_url_no_pad(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Decode a URL-safe Base64 string. Accepts both padded and unpadded
/// forms (JWT drops the padding).
pub fn decode_url(s: &str) -> Result<Vec<u8>, Base64Err> {
    match URL_SAFE.decode(s) {
        Ok(v) => Ok(v),
        Err(_) => URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|e| Base64Err::Decode(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RFC 4648 § 10 test vectors ----

    #[test]
    fn rfc4648_vectors_standard_encode() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn rfc4648_vectors_standard_decode() {
        assert_eq!(decode("").unwrap(), b"");
        assert_eq!(decode("Zg==").unwrap(), b"f");
        assert_eq!(decode("Zm8=").unwrap(), b"fo");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
        assert_eq!(decode("Zm9vYg==").unwrap(), b"foob");
        assert_eq!(decode("Zm9vYmE=").unwrap(), b"fooba");
        assert_eq!(decode("Zm9vYmFy").unwrap(), b"foobar");
    }

    #[test]
    fn decode_accepts_missing_padding() {
        // "Zg" is the no-pad form of "Zg==" → "f"
        assert_eq!(decode("Zg").unwrap(), b"f");
        assert_eq!(decode("Zm8").unwrap(), b"fo");
    }

    #[test]
    fn decode_rejects_bad_alphabet() {
        // "!" is not in the standard alphabet.
        assert!(decode("Zg!=").is_err());
    }

    #[test]
    fn hello_world() {
        let s = encode(b"hello");
        assert_eq!(s, "aGVsbG8=");
        assert_eq!(decode(&s).unwrap(), b"hello");
    }

    #[test]
    fn long_input_round_trips() {
        let input: Vec<u8> = (0u8..=255).cycle().take(10_000).collect();
        let s = encode(&input);
        let decoded = decode(&s).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn binary_with_all_byte_values_round_trips() {
        let input: Vec<u8> = (0u8..=255).collect();
        let s = encode(&input);
        assert_eq!(decode(&s).unwrap(), input);
    }

    // ---- URL-safe variant ----

    #[test]
    fn url_safe_uses_dash_underscore() {
        // Bytes 0xfb, 0xff, 0xbf encode as "-_-_" in URL-safe, "+/+/" in standard.
        let raw = [0xfb, 0xff, 0xbf];
        assert_eq!(encode_url(&raw), "-_-_");
        assert_eq!(encode(&raw), "+/+/");
    }

    #[test]
    fn url_safe_round_trips() {
        let data = b"https://example.com/path?q=hello world";
        let s = encode_url(data);
        assert_eq!(decode_url(&s).unwrap(), data);
    }

    #[test]
    fn url_safe_no_pad_matches_jwt_shape() {
        // JWT segments are URL-safe, no padding.
        let header = br#"{"alg":"HS256","typ":"JWT"}"#;
        let s = encode_url_no_pad(header);
        assert!(!s.contains('='));
        assert_eq!(decode_url(&s).unwrap(), header);
    }

    #[test]
    fn url_safe_decode_accepts_padded_and_unpadded() {
        let raw = [0xfb, 0xff, 0xbe];
        let padded = encode_url(&raw);
        let nopad = encode_url_no_pad(&raw);
        assert_eq!(decode_url(&padded).unwrap(), raw);
        assert_eq!(decode_url(&nopad).unwrap(), raw);
    }

    #[test]
    fn url_safe_rejects_standard_chars() {
        // Standard "+" is not in the URL-safe alphabet.
        assert!(decode_url("+/+/").is_err());
    }

    // ---- empty + edge ----

    #[test]
    fn empty_bytes_round_trip() {
        let s = encode(b"");
        assert_eq!(s, "");
        assert_eq!(decode(&s).unwrap(), b"");
    }

    #[test]
    fn single_byte_round_trip() {
        for b in 0u8..=255 {
            let s = encode(&[b]);
            let d = decode(&s).unwrap();
            assert_eq!(d, vec![b], "round-trip failed for {b:#04x}");
        }
    }

    #[test]
    fn two_byte_round_trip() {
        // Sample a handful of two-byte combinations.
        for a in [0u8, 1, 0x55, 0xaa, 0xff] {
            for b in [0u8, 1, 0x55, 0xaa, 0xff] {
                let s = encode(&[a, b]);
                assert_eq!(decode(&s).unwrap(), vec![a, b]);
            }
        }
    }

    #[test]
    fn decode_error_message_is_descriptive() {
        let err = decode("not%base64").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("invalid base64"), "msg: {}", msg);
    }

    #[test]
    fn encode_does_not_emit_whitespace() {
        // base64 0.22 default engine should not emit line breaks; some
        // older libs split at column 76 (MIME). Confirm we don't.
        let s = encode(&[0u8; 200]);
        assert!(!s.contains('\n'));
        assert!(!s.contains(' '));
    }
}
