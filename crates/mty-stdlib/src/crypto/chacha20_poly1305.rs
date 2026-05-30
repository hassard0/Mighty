//! ChaCha20-Poly1305 — RFC 8439 AEAD.
//!
//! v0.40 T4. ChaCha20-Poly1305 is the second mandatory AEAD in any
//! modern stack: TLS 1.3 mandates it as a ciphersuite, WireGuard uses
//! it exclusively, and it's the right choice on hardware without
//! AES-NI (ARM, embedded, server farms without AES instructions —
//! constant-time C/assembly ChaCha is faster than software AES).
//!
//! Backed by the [`chacha20poly1305`] crate from RustCrypto — audited
//! and constant-time.
//!
//! Identical API shape to [`aes_gcm`](super::aes_gcm) on purpose so
//! the caller can pick the cipher by changing one function name:
//!
//! ```ignore
//! use std.crypto.chacha20_poly1305.{encrypt, decrypt};
//!
//! let key: [U8; 32] = ...;
//! let nonce: [U8; 12] = ...;          // MUST be unique per (key, message)
//! let aad: &[U8] = b"v1";
//! let ct: Vec<U8> = encrypt(&key, &nonce, aad, b"hello")?;
//! let pt: Vec<U8> = decrypt(&key, &nonce, aad, &ct)?;
//! ```
//!
//! Security properties match AES-GCM — see [`aes_gcm`](super::aes_gcm)
//! for the full discussion. One nuance worth calling out:
//!
//! - The 96-bit nonce ceiling for random nonces in ChaCha20-Poly1305
//!   is the same ~2^32 messages per key. If you need more, use
//!   XChaCha20-Poly1305 (192-bit nonce, randomly safe). v0.40 doesn't
//!   ship that variant — followups can.
//!
//! No capability required.

use super::aes_gcm::AeadErr;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

/// ChaCha20-Poly1305 encrypt. Returns `plaintext_len + 16` bytes:
/// ciphertext followed by the 128-bit Poly1305 auth tag.
pub fn chacha20_poly1305_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, AeadErr> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    cipher
        .encrypt(Nonce::from_slice(nonce), payload)
        .map_err(|e| AeadErr::Encrypt(e.to_string()))
}

/// ChaCha20-Poly1305 decrypt. Verifies the tag in constant time before
/// returning the plaintext. Any tampering yields [`AeadErr::Decrypt`].
pub fn chacha20_poly1305_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, AeadErr> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let payload = Payload {
        msg: ciphertext,
        aad,
    };
    cipher
        .decrypt(Nonce::from_slice(nonce), payload)
        .map_err(|_| AeadErr::Decrypt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_decode(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn arr32(s: &str) -> [u8; 32] {
        let v = hex_decode(s);
        let mut a = [0u8; 32];
        a.copy_from_slice(&v);
        a
    }

    fn arr12(s: &str) -> [u8; 12] {
        let v = hex_decode(s);
        let mut a = [0u8; 12];
        a.copy_from_slice(&v);
        a
    }

    // -------------------------------------------------------------------
    // RFC 8439 §2.8.2 — ChaCha20-Poly1305 AEAD known-answer test.
    //
    // Plaintext is the famous "Ladies and Gentlemen of the class of '99"
    // paragraph; key, nonce, AAD, and expected ciphertext+tag are pinned
    // by the RFC. This is THE canonical KAT every ChaCha20-Poly1305
    // implementation gets validated against.
    // -------------------------------------------------------------------

    fn rfc8439_plaintext() -> Vec<u8> {
        b"Ladies and Gentlemen of the class of '99: If I could offer you o\
          nly one tip for the future, sunscreen would be it."
            .to_vec()
    }

    fn rfc8439_key() -> [u8; 32] {
        arr32(
            "808182838485868788898a8b8c8d8e8f\
             909192939495969798999a9b9c9d9e9f",
        )
    }

    fn rfc8439_nonce() -> [u8; 12] {
        arr12("070000004041424344454647")
    }

    fn rfc8439_aad() -> Vec<u8> {
        hex_decode("50515253c0c1c2c3c4c5c6c7")
    }

    fn rfc8439_expected_ct() -> Vec<u8> {
        hex_decode(
            "d31a8d34648e60db7b86afbc53ef7ec2\
             a4aded51296e08fea9e2b5a736ee62d6\
             3dbea45e8ca9671282fafb69da92728b\
             1a71de0a9e060b2905d6a5b67ecd3b36\
             92ddbd7f2d778b8c9803aee328091b58\
             fab324e4fad675945585808b4831d7bc\
             3ff4def08e4b7a9de576d26586cec64b\
             6116",
        )
    }

    fn rfc8439_expected_tag() -> Vec<u8> {
        hex_decode("1ae10b594f09e26a7e902ecbd0600691")
    }

    #[test]
    fn rfc8439_aead_kat_encrypt() {
        let out = chacha20_poly1305_encrypt(
            &rfc8439_key(),
            &rfc8439_nonce(),
            &rfc8439_aad(),
            &rfc8439_plaintext(),
        )
        .unwrap();
        let expected_ct = rfc8439_expected_ct();
        let expected_tag = rfc8439_expected_tag();
        assert_eq!(out.len(), expected_ct.len() + 16);
        assert_eq!(&out[..expected_ct.len()], expected_ct.as_slice());
        assert_eq!(&out[expected_ct.len()..], expected_tag.as_slice());
    }

    #[test]
    fn rfc8439_aead_kat_decrypt() {
        let mut blob = rfc8439_expected_ct();
        blob.extend_from_slice(&rfc8439_expected_tag());
        let pt = chacha20_poly1305_decrypt(&rfc8439_key(), &rfc8439_nonce(), &rfc8439_aad(), &blob)
            .unwrap();
        assert_eq!(pt, rfc8439_plaintext());
    }

    // -------------------------------------------------------------------
    // RFC 8439 Appendix A.5 — second vector: all-zero key + zero nonce.
    // -------------------------------------------------------------------

    #[test]
    fn rfc8439_appendix_a5_round_trip() {
        // Appendix A.5 has a longer vector with a non-trivial plaintext —
        // we exercise round-trip rather than pinning the exact bytes (the
        // RFC 8439 §2.8.2 KAT above already pins one bit-for-bit).
        let key = arr32(
            "1c9240a5eb55d38af333888604f6b5f0\
             473917c1402b80099dca5cbc207075c0",
        );
        let nonce = arr12("000000000102030405060708");
        let aad = hex_decode("f33388860000000000004e91");
        let pt: Vec<u8> = (0u8..114).collect();
        let ct = chacha20_poly1305_encrypt(&key, &nonce, &aad, &pt).unwrap();
        let pt2 = chacha20_poly1305_decrypt(&key, &nonce, &aad, &ct).unwrap();
        assert_eq!(pt2, pt);
    }

    // -------------------------------------------------------------------
    // Round-trip + tamper coverage.
    // -------------------------------------------------------------------

    #[test]
    fn round_trip_long_plaintext() {
        let key = [0x42u8; 32];
        let nonce = [0xa5u8; 12];
        let pt = vec![0x55u8; 4096];
        let ct = chacha20_poly1305_encrypt(&key, &nonce, b"v=1", &pt).unwrap();
        assert_eq!(ct.len(), pt.len() + 16);
        let pt2 = chacha20_poly1305_decrypt(&key, &nonce, b"v=1", &ct).unwrap();
        assert_eq!(pt2, pt);
    }

    #[test]
    fn round_trip_empty_plaintext() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let ct = chacha20_poly1305_encrypt(&key, &nonce, b"meta", b"").unwrap();
        assert_eq!(ct.len(), 16);
        let pt = chacha20_poly1305_decrypt(&key, &nonce, b"meta", &ct).unwrap();
        assert!(pt.is_empty());
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let mut ct = chacha20_poly1305_encrypt(&key, &nonce, b"", b"sensitive").unwrap();
        ct[0] ^= 0x01;
        assert!(matches!(
            chacha20_poly1305_decrypt(&key, &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn tampered_tag_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let mut ct = chacha20_poly1305_encrypt(&key, &nonce, b"", b"sensitive").unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x80;
        assert!(matches!(
            chacha20_poly1305_decrypt(&key, &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_aad_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let ct = chacha20_poly1305_encrypt(&key, &nonce, b"aad-A", b"msg").unwrap();
        assert!(matches!(
            chacha20_poly1305_decrypt(&key, &nonce, b"aad-B", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_key_rejected() {
        let nonce = [0x02u8; 12];
        let ct = chacha20_poly1305_encrypt(&[0x01u8; 32], &nonce, b"", b"msg").unwrap();
        assert!(matches!(
            chacha20_poly1305_decrypt(&[0x02u8; 32], &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_nonce_rejected() {
        let key = [0x01u8; 32];
        let ct = chacha20_poly1305_encrypt(&key, &[0xaau8; 12], b"", b"msg").unwrap();
        assert!(matches!(
            chacha20_poly1305_decrypt(&key, &[0xbbu8; 12], b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn distinct_nonces_yield_distinct_ciphertexts() {
        let key = [0x42u8; 32];
        let pt = b"deterministic plaintext";
        let ct1 = chacha20_poly1305_encrypt(&key, &[0x01u8; 12], b"", pt).unwrap();
        let ct2 = chacha20_poly1305_encrypt(&key, &[0x02u8; 12], b"", pt).unwrap();
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn ciphertext_format_is_pt_then_tag() {
        let key = [0x42u8; 32];
        let nonce = [0xaau8; 12];
        let pt = b"X"; // 1 byte
        let ct = chacha20_poly1305_encrypt(&key, &nonce, b"", pt).unwrap();
        assert_eq!(ct.len(), 1 + 16);
    }
}
