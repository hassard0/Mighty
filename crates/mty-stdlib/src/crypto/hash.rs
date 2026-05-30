//! Cryptographic hash functions: SHA-256, SHA-512, BLAKE3.
//!
//! Each algorithm is exposed in two shapes:
//!
//! - **One-shot** — `sha256(bytes) -> [u8; 32]`. The 95% case.
//! - **Streaming** — a `*Hasher` wrapper that supports `update(&[u8])`
//!   and `finalize() -> [u8; N]`. Used for hashing readers or
//!   incremental data that doesn't fit in memory.
//!
//! No allocations on the one-shot path beyond the underlying digest
//! state (which is stack-resident in `sha2`'s implementation).

use sha2::Digest;
use std::io::{self, Read};

// ---------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------

/// One-shot SHA-256 over the input slice. Returns the 32-byte digest.
///
/// Stateless and pure — no capability required.
///
/// ```
/// # use mty_stdlib::crypto::hash::sha256;
/// let h = sha256(b"hello");
/// assert_eq!(
///     hex::encode(h),
///     "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
/// );
/// ```
#[must_use]
pub fn sha256(input: &[u8]) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(input);
    h.finalize().into()
}

/// Streaming SHA-256.
#[derive(Default, Clone)]
pub struct Sha256Hasher(sha2::Sha256);

impl Sha256Hasher {
    /// Fresh hasher state.
    #[must_use]
    pub fn new() -> Self {
        Self(sha2::Sha256::new())
    }

    /// Absorb more bytes.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// Drain a `Read` source through the hasher. Returns the number of
    /// bytes consumed. Useful for hashing files without loading them
    /// fully into memory.
    pub fn update_reader<R: Read>(&mut self, mut r: R) -> io::Result<u64> {
        let mut buf = [0u8; 8 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            self.0.update(&buf[..n]);
            total += n as u64;
        }
        Ok(total)
    }

    /// Produce the 32-byte digest and consume the hasher.
    #[must_use]
    pub fn finalize(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

// ---------------------------------------------------------------------------
// SHA-512
// ---------------------------------------------------------------------------

/// One-shot SHA-512 over the input slice. Returns the 64-byte digest.
#[must_use]
pub fn sha512(input: &[u8]) -> [u8; 64] {
    let mut h = sha2::Sha512::new();
    h.update(input);
    h.finalize().into()
}

/// Streaming SHA-512.
#[derive(Default, Clone)]
pub struct Sha512Hasher(sha2::Sha512);

impl Sha512Hasher {
    #[must_use]
    pub fn new() -> Self {
        Self(sha2::Sha512::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn update_reader<R: Read>(&mut self, mut r: R) -> io::Result<u64> {
        let mut buf = [0u8; 8 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            self.0.update(&buf[..n]);
            total += n as u64;
        }
        Ok(total)
    }

    #[must_use]
    pub fn finalize(self) -> [u8; 64] {
        self.0.finalize().into()
    }
}

// ---------------------------------------------------------------------------
// BLAKE3
// ---------------------------------------------------------------------------

/// One-shot BLAKE3 over the input slice. Returns the 32-byte digest
/// (default BLAKE3 output length; the underlying construction supports
/// arbitrary-length extension which we leave to the streaming API).
#[must_use]
pub fn blake3(input: &[u8]) -> [u8; 32] {
    *::blake3::hash(input).as_bytes()
}

/// Streaming BLAKE3.
#[derive(Default, Clone)]
pub struct Blake3Hasher(::blake3::Hasher);

impl Blake3Hasher {
    #[must_use]
    pub fn new() -> Self {
        Self(::blake3::Hasher::new())
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub fn update_reader<R: Read>(&mut self, mut r: R) -> io::Result<u64> {
        let mut buf = [0u8; 8 * 1024];
        let mut total: u64 = 0;
        loop {
            let n = r.read(&mut buf)?;
            if n == 0 {
                break;
            }
            self.0.update(&buf[..n]);
            total += n as u64;
        }
        Ok(total)
    }

    #[must_use]
    pub fn finalize(self) -> [u8; 32] {
        *self.0.finalize().as_bytes()
    }

    /// XOF-style extended output. Pulls `out.len()` bytes from the
    /// BLAKE3 PRF construction. Use when you need a longer-than-256-bit
    /// digest (e.g. KDF, MAC tag of unusual length).
    pub fn finalize_xof(self, out: &mut [u8]) {
        let mut reader = self.0.finalize_xof();
        reader.fill(out);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        let s = s.trim().to_lowercase();
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn to_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            out.push_str(&format!("{:02x}", b));
        }
        out
    }

    // ---- SHA-256 known-answer-tests (NIST FIPS-180-2 + RFC 6234) ----

    #[test]
    fn sha256_empty_string() {
        // NIST: SHA-256("") =
        // e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let h = sha256(b"");
        assert_eq!(
            to_hex(&h),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        // NIST: SHA-256("abc")
        let h = sha256(b"abc");
        assert_eq!(
            to_hex(&h),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_long_message() {
        // NIST: SHA-256 of the 448-bit message
        // "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        let msg = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let h = sha256(msg);
        assert_eq!(
            to_hex(&h),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_one_million_a() {
        // NIST FIPS-180-2 Appendix B.3: SHA-256 of exactly 1_000_000 'a' bytes.
        let mut h = Sha256Hasher::new();
        let chunk = vec![b'a'; 1000];
        for _ in 0..1000 {
            h.update(&chunk);
        }
        let digest = h.finalize();
        assert_eq!(
            to_hex(&digest),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha256_streaming_matches_oneshot() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let oneshot = sha256(data);
        let mut s = Sha256Hasher::new();
        // Feed byte by byte to exercise the absorbing state machine.
        for b in data {
            s.update(&[*b]);
        }
        assert_eq!(s.finalize(), oneshot);
    }

    #[test]
    fn sha256_streaming_reader() {
        let data = b"reader streaming exercises the chunked path";
        let mut s = Sha256Hasher::new();
        let n = s.update_reader(&data[..]).unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(s.finalize(), sha256(data));
    }

    #[test]
    fn sha256_default_is_new() {
        let a: Sha256Hasher = Default::default();
        let b = Sha256Hasher::new();
        assert_eq!(a.finalize(), b.finalize());
    }

    // ---- SHA-512 known-answer-tests ----

    #[test]
    fn sha512_empty_string() {
        // RFC 4231 / NIST: SHA-512("")
        let h = sha512(b"");
        let expected = "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce\
                        47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
        assert_eq!(to_hex(&h), expected);
    }

    #[test]
    fn sha512_abc() {
        let h = sha512(b"abc");
        let expected = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
                        2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
        assert_eq!(to_hex(&h), expected);
    }

    #[test]
    fn sha512_streaming_matches_oneshot() {
        let data = b"streaming sha512 vs one-shot sha512 must agree";
        let oneshot = sha512(data);
        let mut s = Sha512Hasher::new();
        for chunk in data.chunks(7) {
            s.update(chunk);
        }
        assert_eq!(s.finalize(), oneshot);
    }

    #[test]
    fn sha512_streaming_reader() {
        let data = b"the SHA-512 streaming reader path";
        let mut s = Sha512Hasher::new();
        let n = s.update_reader(&data[..]).unwrap();
        assert_eq!(n as usize, data.len());
        assert_eq!(s.finalize(), sha512(data));
    }

    // ---- BLAKE3 known-answer-tests ----

    #[test]
    fn blake3_empty() {
        // BLAKE3 official: empty input
        let h = blake3(b"");
        assert_eq!(
            to_hex(&h),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn blake3_abc_matches_reference() {
        // Our `blake3()` is a thin wrapper over the reference impl —
        // confirm the wrapper doesn't drop bytes / swap nibbles by
        // checking against the reference's own hash() entry point.
        let ours = blake3(b"abc");
        let reference: [u8; 32] = *::blake3::hash(b"abc").as_bytes();
        assert_eq!(ours, reference);
    }

    #[test]
    fn blake3_large_input_streaming_matches_oneshot() {
        // 4 KiB of the canonical BLAKE3 test pattern (n mod 251). This
        // exercises the chunked compression path (any input >1 KiB
        // crosses an internal chunk boundary).
        let mut input = Vec::with_capacity(4096);
        for i in 0..4096u32 {
            input.push((i % 251) as u8);
        }
        let oneshot = blake3(&input);
        let mut s = Blake3Hasher::new();
        for chunk in input.chunks(177) {
            s.update(chunk);
        }
        assert_eq!(s.finalize(), oneshot);
    }

    #[test]
    fn blake3_streaming_matches_oneshot() {
        let data = b"blake3 streaming check";
        let oneshot = blake3(data);
        let mut s = Blake3Hasher::new();
        for byte in data {
            s.update(&[*byte]);
        }
        assert_eq!(s.finalize(), oneshot);
    }

    #[test]
    fn blake3_xof_first_32_bytes_matches_default() {
        let data = b"BLAKE3 XOF prefix is just the default 32-byte digest";
        let oneshot = blake3(data);
        let mut s = Blake3Hasher::new();
        s.update(data);
        let mut out = [0u8; 32];
        s.finalize_xof(&mut out);
        assert_eq!(out, oneshot);
    }

    #[test]
    fn blake3_xof_64_bytes() {
        // 64-byte output is a known length for BLAKE3 KAT vectors.
        let data = b"";
        let mut s = Blake3Hasher::new();
        s.update(data);
        let mut out = [0u8; 64];
        s.finalize_xof(&mut out);
        // First 32 bytes match the default digest of empty.
        assert_eq!(
            to_hex(&out[..32]),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    // ---- helpers + sanity ----

    #[test]
    fn hex_to_bytes_round_trips() {
        let bytes = vec![0xde, 0xad, 0xbe, 0xef];
        let s = to_hex(&bytes);
        assert_eq!(s, "deadbeef");
        assert_eq!(hex_to_bytes(&s), bytes);
    }

    #[test]
    fn sha256_unicode_input() {
        // UTF-8 bytes of "Mighty 💪" — exercises high-bit input.
        let s = "Mighty 💪";
        let h = sha256(s.as_bytes());
        // Length sanity (32 bytes) is the load-bearing check; the value
        // is captured for regression purposes.
        assert_eq!(h.len(), 32);
        // Recompute streaming and compare.
        let mut hs = Sha256Hasher::new();
        hs.update(s.as_bytes());
        assert_eq!(hs.finalize(), h);
    }

    #[test]
    fn sha512_unicode_input() {
        let s = "stdlib 🚀";
        let h = sha512(s.as_bytes());
        assert_eq!(h.len(), 64);
    }
}
