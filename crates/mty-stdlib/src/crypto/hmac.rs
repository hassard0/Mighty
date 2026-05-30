//! HMAC — Keyed-Hash Message Authentication Code (RFC 2104).
//!
//! Two flavours: HMAC-SHA-256 (32-byte tag) and HMAC-SHA-512 (64-byte
//! tag). Both are constant-time on the verify path via [`subtle_eq`].
//!
//! The implementation goes through the `hmac` crate, which delegates to
//! the `sha2` digests already pulled in by [`crate::crypto::hash`].

use ::hmac::{Mac, SimpleHmac};
use sha2::{Sha256, Sha512};

/// HMAC-SHA-256 — one-shot. Returns the 32-byte MAC tag.
///
/// ```
/// # use mty_stdlib::crypto::hmac::hmac_sha256;
/// // RFC 4231 Test Case 1
/// let key = [0x0bu8; 20];
/// let mac = hmac_sha256(&key, b"Hi There");
/// assert_eq!(
///     hex::encode(mac),
///     "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
/// );
/// ```
#[must_use]
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut mac = SimpleHmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// HMAC-SHA-512 — one-shot. Returns the 64-byte MAC tag.
#[must_use]
pub fn hmac_sha512(key: &[u8], message: &[u8]) -> [u8; 64] {
    let mut mac = SimpleHmac::<Sha512>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// Constant-time slice equality. Returns `true` iff the two slices are
/// the same length and the same bytes. Use this for tag verification —
/// the naive `==` short-circuits on the first differing byte and leaks
/// timing.
#[must_use]
pub fn subtle_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{:02x}", b));
        }
        out
    }

    // ---- RFC 4231 known-answer-tests for HMAC-SHA-256 ----

    #[test]
    fn rfc4231_test_case_1_sha256() {
        let key = vec![0x0bu8; 20];
        let data = b"Hi There";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            to_hex(&mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn rfc4231_test_case_2_sha256() {
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let mac = hmac_sha256(key, data);
        assert_eq!(
            to_hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn rfc4231_test_case_3_sha256() {
        let key = vec![0xaau8; 20];
        let data = vec![0xddu8; 50];
        let mac = hmac_sha256(&key, &data);
        assert_eq!(
            to_hex(&mac),
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe"
        );
    }

    #[test]
    fn rfc4231_test_case_4_sha256() {
        // Key is the bytes 0x01..=0x19 (25 bytes).
        let key: Vec<u8> = (1u8..=25).collect();
        let data = vec![0xcdu8; 50];
        let mac = hmac_sha256(&key, &data);
        assert_eq!(
            to_hex(&mac),
            "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b"
        );
    }

    #[test]
    fn rfc4231_test_case_5_sha256_truncated() {
        // "Test Truncation"
        let key = vec![0x0cu8; 20];
        let data = b"Test With Truncation";
        let mac = hmac_sha256(&key, data);
        // RFC tests t=128 truncation; we hold the full 256-bit tag,
        // so prefix to 16 bytes and compare.
        assert_eq!(to_hex(&mac[..16]), "a3b6167473100ee06e0c796c2955552b");
    }

    #[test]
    fn rfc4231_test_case_6_sha256_large_key() {
        let key = vec![0xaau8; 131];
        let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            to_hex(&mac),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn rfc4231_test_case_7_sha256_large_key_and_data() {
        let key = vec![0xaau8; 131];
        let data = b"This is a test using a larger than block-size key \
                     and a larger than block-size data. The key needs to \
                     be hashed before being used by the HMAC algorithm.";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            to_hex(&mac),
            "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2"
        );
    }

    // ---- RFC 4231 KATs for HMAC-SHA-512 ----

    #[test]
    fn rfc4231_test_case_1_sha512() {
        let key = vec![0x0bu8; 20];
        let data = b"Hi There";
        let mac = hmac_sha512(&key, data);
        let expected = "87aa7cdea5ef619d4ff0b4241a1d6cb0\
                        2379f4e2ce4ec2787ad0b30545e17cde\
                        daa833b7d6b8a702038b274eaea3f4e4\
                        be9d914eeb61f1702e696c203a126854";
        assert_eq!(to_hex(&mac), expected.replace(' ', ""));
    }

    #[test]
    fn rfc4231_test_case_2_sha512() {
        let key = b"Jefe";
        let data = b"what do ya want for nothing?";
        let mac = hmac_sha512(key, data);
        let expected = "164b7a7bfcf819e2e395fbe73b56e0a3\
                        87bd64222e831fd610270cd7ea250554\
                        9758bf75c05a994a6d034f65f8f0e6fd\
                        caeab1a34d4a6b4b636e070a38bce737";
        assert_eq!(to_hex(&mac), expected);
    }

    // ---- streaming + verification helpers ----

    #[test]
    fn empty_key_and_empty_message() {
        // HMAC accepts any key length, including zero.
        let mac256 = hmac_sha256(b"", b"");
        let mac512 = hmac_sha512(b"", b"");
        // Length checks — the actual values are pinned for regression.
        assert_eq!(mac256.len(), 32);
        assert_eq!(mac512.len(), 64);
        assert_eq!(
            to_hex(&mac256),
            "b613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad"
        );
    }

    #[test]
    fn different_keys_produce_different_tags() {
        let m = b"same message";
        let a = hmac_sha256(b"key-A", m);
        let b = hmac_sha256(b"key-B", m);
        assert_ne!(a, b);
    }

    #[test]
    fn different_messages_produce_different_tags() {
        let k = b"shared-key";
        let a = hmac_sha256(k, b"alpha");
        let b = hmac_sha256(k, b"beta");
        assert_ne!(a, b);
    }

    #[test]
    fn subtle_eq_matches_naive_for_equal() {
        let a = [1u8, 2, 3, 4];
        let b = [1u8, 2, 3, 4];
        assert!(subtle_eq(&a, &b));
    }

    #[test]
    fn subtle_eq_rejects_length_mismatch() {
        assert!(!subtle_eq(&[1, 2, 3], &[1, 2, 3, 4]));
    }

    #[test]
    fn subtle_eq_rejects_byte_mismatch() {
        assert!(!subtle_eq(&[1, 2, 3], &[1, 2, 4]));
    }

    #[test]
    fn subtle_eq_on_empty_slices() {
        // RFC convention: empty is equal to empty.
        assert!(subtle_eq(&[], &[]));
    }
}
