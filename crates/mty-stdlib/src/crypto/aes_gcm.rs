//! AES-GCM — authenticated encryption with associated data (AEAD).
//!
//! v0.40 T4. AES-256-GCM is the workhorse symmetric AEAD that backs
//! every real-world cookie/token/file encryption stack — TLS 1.3,
//! AWS S3-SSE, Signal, Fernet successors, etc. We surface the
//! 256-bit-key variant only; 128-bit AES is an acceptable downgrade
//! choice but it doesn't pull its weight in a new stdlib.
//!
//! Backed by the [`aes-gcm`] crate from RustCrypto — audited (NCC
//! Group + Trail of Bits), constant-time on the AES key schedule (via
//! `aes` crate's bitsliced or AES-NI backend), and `no_std`-compatible.
//!
//! API shape:
//!
//! ```ignore
//! use std.crypto.aes_gcm.{encrypt, decrypt};
//!
//! let key: [U8; 32] = ...;          // user-derived (HMAC, HKDF, ...)
//! let nonce: [U8; 12] = ...;         // MUST be unique per (key, message)
//! let aad: &[U8] = b"v1";            // optional bound metadata
//! let ct: Vec<U8> = encrypt(&key, &nonce, aad, b"hello")?;
//! let pt: Vec<U8> = decrypt(&key, &nonce, aad, &ct)?;
//! assert_eq!(pt, b"hello");
//! ```
//!
//! Security properties:
//!
//! - 256-bit key — secret. Use [`crate::crypto::hmac`] +
//!   [`crate::crypto::hash`] for HKDF if you're deriving from a master.
//! - 96-bit nonce — **MUST be unique** for every `(key, message)`
//!   pair. Reusing a nonce under the same key catastrophically breaks
//!   GCM (an attacker can recover the GHASH authentication key). Use
//!   a counter (sequence number, kept in storage) OR
//!   [`crate::crypto::rand::random_bytes(12)`] with the understanding
//!   that ~2^32 messages per key is the safe ceiling for random
//!   nonces.
//! - AAD (additional authenticated data) — not encrypted, but bound
//!   into the auth tag. Use this to commit the ciphertext to a
//!   version tag, key id, request id, etc. The decryptor must pass
//!   the same AAD bytes or decrypt fails.
//! - Ciphertext layout — `aes-gcm` returns `plaintext_len + 16` bytes:
//!   the encrypted plaintext followed by the 128-bit GCM auth tag.
//!   [`decrypt`] verifies the tag in constant time before returning
//!   the plaintext (any tampering yields [`AeadErr::Decrypt`]).
//!
//! No capability required — key material is user-owned and no entropy
//! is consumed inside the call. (The caller that *generates* nonces
//! via `crypto.rand` is what surfaces an entropy capability check.)

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};

/// Errors from the AEAD surface.
#[derive(Debug, thiserror::Error)]
pub enum AeadErr {
    /// Encryption rejected the input — the only way this fires for
    /// AES-256-GCM is plaintext longer than 2^36 - 32 bytes, which is
    /// not reachable in practice. Surfaced for completeness and to
    /// match the ChaCha20-Poly1305 shape.
    #[error("aead encrypt: {0}")]
    Encrypt(String),
    /// Decryption failed — either the auth tag didn't verify (the
    /// ciphertext, AAD, key, or nonce was tampered with / wrong) or
    /// the ciphertext is shorter than the 16-byte tag. The error is
    /// intentionally OPAQUE: leaking which failure happened helps
    /// padding-oracle-style attackers.
    #[error("aead decrypt: authentication failed")]
    Decrypt,
}

/// AES-256-GCM encrypt. Returns `plaintext_len + 16` bytes: ciphertext
/// followed by the 128-bit GCM auth tag.
///
/// `nonce` MUST be unique for every `(key, message)` pair under this
/// key (see module docs).
pub fn aes_gcm_256_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, AeadErr> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    cipher
        .encrypt(Nonce::from_slice(nonce), payload)
        .map_err(|e| AeadErr::Encrypt(e.to_string()))
}

/// AES-256-GCM decrypt. Verifies the auth tag in constant time before
/// returning plaintext. Any tampering with the ciphertext, AAD, key,
/// or nonce yields [`AeadErr::Decrypt`] — the error is intentionally
/// opaque (no distinction between "wrong key" and "wrong AAD" to
/// avoid leaking oracles).
pub fn aes_gcm_256_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, AeadErr> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
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
    // NIST CAVP gcmEncryptExtIV256.rsp known-answer-tests.
    //
    // These vectors come from NIST's GCM Validation System (GCMVS) — the
    // file is `gcmEncryptExtIV256.rsp` under the AES Known Answer Test
    // bundle (CAVP). We pick a representative slice across PT/AAD lengths.
    //
    // Each test:
    //   - encrypts known PT under known (Key, IV, AAD) and asserts the
    //     ciphertext + tag match the expected output
    //   - decrypts the same ciphertext and verifies the plaintext
    // -------------------------------------------------------------------

    /// `[Keylen=256, IVlen=96, PTlen=0, AADlen=0, Taglen=128] Count=0`
    #[test]
    fn cavp_keylen256_pt0_aad0_count0() {
        let key = arr32("b52c505a37d78eda5dd34f20c22540ea1b58963cf8e5bf8ffa85f9f2492505b4");
        let iv = arr12("516c33929df5a3284ff463d7");
        let pt: Vec<u8> = vec![];
        let aad: Vec<u8> = vec![];
        let expected_ct: Vec<u8> = vec![];
        let expected_tag = hex_decode("bdc1ac884d332457a1d2664f168c76f0");
        let out = aes_gcm_256_encrypt(&key, &iv, &aad, &pt).unwrap();
        assert_eq!(out.len(), expected_ct.len() + 16);
        assert_eq!(&out[..expected_ct.len()], expected_ct.as_slice());
        assert_eq!(&out[expected_ct.len()..], expected_tag.as_slice());
        let round = aes_gcm_256_decrypt(&key, &iv, &aad, &out).unwrap();
        assert_eq!(round, pt);
    }

    /// `[Keylen=256, IVlen=96, PTlen=128, AADlen=0, Taglen=128] Count=0`
    #[test]
    fn cavp_keylen256_pt128_aad0_count0() {
        let key = arr32("31bdadd96698c204aa9ce1448ea94ae1fb4a9a0b3c9d773b51bb1822666b8f22");
        let iv = arr12("0d18e06c7c725ac9e362e1ce");
        let pt = hex_decode("2db5168e932556f8089a0622981d017d");
        let aad: Vec<u8> = vec![];
        let expected_ct = hex_decode("fa4362189661d163fcd6a56d8bf0405a");
        let expected_tag = hex_decode("d636ac1bbedd5cc3ee727dc2ab4a9489");
        let out = aes_gcm_256_encrypt(&key, &iv, &aad, &pt).unwrap();
        assert_eq!(&out[..expected_ct.len()], expected_ct.as_slice());
        assert_eq!(&out[expected_ct.len()..], expected_tag.as_slice());
        let round = aes_gcm_256_decrypt(&key, &iv, &aad, &out).unwrap();
        assert_eq!(round, pt);
    }

    /// NIST SP 800-38D Appendix B Test Case 4 — AES-256-GCM with AAD.
    ///
    /// This is the classic GCM spec vector (the same one shipped in
    /// virtually every GCM test suite) but adapted to the 256-bit key
    /// expansion. We verify the AAD-binding property: distinct AAD
    /// under the same key/nonce/plaintext yields distinct auth tags,
    /// and the round-trip succeeds.
    #[test]
    fn aad_round_trip_with_known_aad_length() {
        let key = arr32("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
        let iv = arr12("cafebabefacedbaddecaf888");
        let pt = hex_decode(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = hex_decode("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let out = aes_gcm_256_encrypt(&key, &iv, &aad, &pt).unwrap();
        // Round-trip succeeds.
        let round = aes_gcm_256_decrypt(&key, &iv, &aad, &out).unwrap();
        assert_eq!(round, pt);
        // Output is plaintext_len + 16 byte tag.
        assert_eq!(out.len(), pt.len() + 16);
        // Wrong AAD fails — proves AAD is bound into the tag.
        let mut bad_aad = aad.clone();
        bad_aad[0] ^= 0x01;
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &iv, &bad_aad, &out),
            Err(AeadErr::Decrypt)
        ));
    }

    // -------------------------------------------------------------------
    // Round-trip + tamper coverage.
    // -------------------------------------------------------------------

    #[test]
    fn round_trip_long_plaintext() {
        let key = [0x42u8; 32];
        let nonce = [0xa5u8; 12];
        let aad = b"version=1;key-id=primary";
        let pt = vec![0x55u8; 4096];
        let ct = aes_gcm_256_encrypt(&key, &nonce, aad, &pt).unwrap();
        // Ciphertext is plaintext_len + 16 (auth tag).
        assert_eq!(ct.len(), pt.len() + 16);
        let pt2 = aes_gcm_256_decrypt(&key, &nonce, aad, &ct).unwrap();
        assert_eq!(pt2, pt);
    }

    #[test]
    fn round_trip_empty_plaintext_with_aad() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let aad = b"some bound metadata";
        let ct = aes_gcm_256_encrypt(&key, &nonce, aad, b"").unwrap();
        assert_eq!(ct.len(), 16); // just the tag
        let pt = aes_gcm_256_decrypt(&key, &nonce, aad, &ct).unwrap();
        assert!(pt.is_empty());
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let pt = b"sensitive payload";
        let mut ct = aes_gcm_256_encrypt(&key, &nonce, b"", pt).unwrap();
        ct[0] ^= 0x01; // flip a bit in the ciphertext
        let err = aes_gcm_256_decrypt(&key, &nonce, b"", &ct).unwrap_err();
        assert!(matches!(err, AeadErr::Decrypt));
    }

    #[test]
    fn tampered_tag_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let pt = b"sensitive payload";
        let mut ct = aes_gcm_256_encrypt(&key, &nonce, b"", pt).unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x80; // flip MSB of last byte (in the tag)
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_aad_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let ct = aes_gcm_256_encrypt(&key, &nonce, b"original aad", b"msg").unwrap();
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &nonce, b"DIFFERENT aad", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_key_rejected() {
        let nonce = [0x02u8; 12];
        let ct = aes_gcm_256_encrypt(&[0x01u8; 32], &nonce, b"", b"msg").unwrap();
        assert!(matches!(
            aes_gcm_256_decrypt(&[0x02u8; 32], &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn wrong_nonce_rejected() {
        let key = [0x01u8; 32];
        let ct = aes_gcm_256_encrypt(&key, &[0xaau8; 12], b"", b"msg").unwrap();
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &[0xbbu8; 12], b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn truncated_ciphertext_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        let mut ct = aes_gcm_256_encrypt(&key, &nonce, b"", b"hello").unwrap();
        ct.truncate(ct.len() - 1); // drop a byte
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &nonce, b"", &ct),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn empty_ciphertext_rejected() {
        let key = [0x01u8; 32];
        let nonce = [0x02u8; 12];
        // 0 bytes — not even room for the 16-byte tag.
        assert!(matches!(
            aes_gcm_256_decrypt(&key, &nonce, b"", &[]),
            Err(AeadErr::Decrypt)
        ));
    }

    #[test]
    fn distinct_nonces_yield_distinct_ciphertexts() {
        let key = [0x42u8; 32];
        let pt = b"deterministic plaintext";
        let ct1 = aes_gcm_256_encrypt(&key, &[0x01u8; 12], b"", pt).unwrap();
        let ct2 = aes_gcm_256_encrypt(&key, &[0x02u8; 12], b"", pt).unwrap();
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn aad_binds_metadata_into_tag() {
        // Same key/nonce/pt with different AAD → distinct ciphertexts
        // (well, distinct tags — ciphertext-proper is unaffected by
        // AAD but the appended tag differs).
        let key = [0x42u8; 32];
        let nonce = [0xaau8; 12];
        let pt = b"payload";
        let ct_a = aes_gcm_256_encrypt(&key, &nonce, b"aad-A", pt).unwrap();
        let ct_b = aes_gcm_256_encrypt(&key, &nonce, b"aad-B", pt).unwrap();
        // Same plaintext bytes, different tags.
        assert_eq!(&ct_a[..pt.len()], &ct_b[..pt.len()]);
        assert_ne!(&ct_a[pt.len()..], &ct_b[pt.len()..]);
    }

    #[test]
    fn aad_can_be_empty() {
        let key = [0x42u8; 32];
        let nonce = [0xaau8; 12];
        let ct = aes_gcm_256_encrypt(&key, &nonce, b"", b"hello").unwrap();
        let pt = aes_gcm_256_decrypt(&key, &nonce, b"", &ct).unwrap();
        assert_eq!(pt, b"hello");
    }
}
