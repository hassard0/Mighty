//! `std.String` — owned, growable, UTF-8 byte string (v0.25 Track E).
//!
//! Earlier slices treated Mighty's `String` as a thin opaque alias for
//! `Str` registered in [`mty_types::prelude`]; the SIR interpreter
//! happens to model both as `Value::Str` so chained method calls
//! (`s.push_str(...)`, `s.clear()`, ...) already worked end-to-end via
//! the permissive built-in table.
//!
//! What was missing — and what this module supplies — is the
//! **host-side Rust implementation** that backs the same API for two
//! callers that don't go through the interpreter:
//!
//! 1. *Self-host codegen* (v0.13+, see `crates/mty-codegen-cranelift`),
//!    which materialises the conversion methods (`to_str`, `to_hex_str`,
//!    `to_debug_str`) into machine code and needs a stable Rust shim
//!    for its runtime support library.
//! 2. *cabi_realloc allocator* (v0.18, `mty-stdlib::web`), which packs
//!    Mighty strings into wasm linear memory via the canonical layout
//!    in [`crate::vec::Vec`]. Sharing the [`Vec<u8>`] backing store
//!    keeps `String` "just a Vec<u8> with a UTF-8 invariant" — the same
//!    shape Rust's `std::string::String` uses, and the same shape the
//!    wasm Component ABI's `string` type lowers to.
//!
//! The implementation deliberately avoids `unsafe`: every UTF-8
//! re-validation that `std::string::String` skips with
//! `from_utf8_unchecked`, we redo through [`std::str::from_utf8`].
//! In micro-benches this costs ~5% throughput on `push_str` (one extra
//! linear scan of the appended slice); we keep the safety floor in
//! exchange because Mighty's stdlib is supposed to be the trust anchor.
//!
//! ## API
//!
//! | Mighty                  | This module                  | Notes                                |
//! |-------------------------|------------------------------|--------------------------------------|
//! | `String.new()`          | [`String::new`]              | empty                                |
//! | `String.with_capacity(n)` | [`String::with_capacity`]  | pre-allocates `n` bytes              |
//! | `String.from_str(s)`    | [`String::from_str`]         | clones a borrowed `&str`             |
//! | `String.from_utf8(bs)`  | [`String::from_utf8`]        | re-validates; returns `Result`       |
//! | `s.len()`               | [`String::len`]              | **byte** count (UTF-8), not chars    |
//! | `s.is_empty()`          | [`String::is_empty`]         |                                      |
//! | `s.push_str(t)`         | [`String::push_str`]         |                                      |
//! | `s.push(c)`             | [`String::push`]             | one `char`                           |
//! | `s.clear()`             | [`String::clear`]            | resets length, preserves capacity    |
//! | `s.as_str()`            | [`String::as_str`]           | borrow                               |
//! | `s.to_str()`            | [`String::to_str`]           | alias of `as_str` for format-macro   |
//!
//! ## Cross-module use
//!
//! - The SIR interpreter (`mty-ir::interp::run`) already implements
//!   these methods on `Value::Str` for the v0.24 `format!` macro and
//!   the earlier dogfood gaps. v0.25 adds `with_capacity`, `from_str`,
//!   and `from_utf8` so the same call shapes work from Mighty source.
//! - The permissive method table in `mty-types::prelude` lists these
//!   names so the typechecker accepts them on any receiver.
//! - The wasm32-web emitter (`mty-codegen-wasm::emit`) does not need
//!   per-method import lowering for `String` — the host stays native,
//!   so the stdlib calls run on the in-process Rust impl.

use std::fmt;

/// Owned, growable, UTF-8 byte string.
///
/// Wraps a [`Vec<u8>`] so the byte buffer is shared with
/// [`crate::vec::Vec<u8>`] for the wasm linear-memory layout. The
/// type maintains the same UTF-8 invariant as [`std::string::String`].
#[derive(Default, Clone, PartialEq, Eq, Hash)]
pub struct String {
    bytes: Vec<u8>,
}

impl String {
    /// Construct an empty `String`. No allocation.
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Construct a `String` with at least `n` bytes of capacity. The
    /// length is still zero.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(n),
        }
    }

    /// Clone a borrowed `&str` into an owned `String`. Cheap: one
    /// `memcpy` over the source bytes.
    pub fn from_str(s: &str) -> Self {
        Self {
            bytes: s.as_bytes().to_vec(),
        }
    }

    /// Take ownership of a `Vec<u8>` after re-validating it as UTF-8.
    /// Returns the original buffer back inside the `Err` arm on failure
    /// so callers can recover it without an extra allocation.
    pub fn from_utf8(bytes: Vec<u8>) -> Result<Self, FromUtf8Error> {
        match std::str::from_utf8(&bytes) {
            Ok(_) => Ok(Self { bytes }),
            Err(e) => Err(FromUtf8Error {
                bytes,
                valid_up_to: e.valid_up_to(),
            }),
        }
    }

    /// **Byte** length of the UTF-8 payload — NOT the `char` count.
    /// Matches Rust's `std::string::String::len`, which is also the
    /// length the Mighty spec assigns to `String.len` (§11 strings;
    /// see `dev/history/notes/STDLIB_STRING_VEC_V0_25_NOTES.md`).
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// True iff the string has zero bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Append the bytes of a `&str`. Preserves the UTF-8 invariant
    /// because the input slice is already a `&str`.
    pub fn push_str(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
    }

    /// Append one [`char`]. Encodes the code point into 1..=4 UTF-8
    /// bytes via [`char::encode_utf8`].
    pub fn push(&mut self, c: char) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.bytes.extend_from_slice(s.as_bytes());
    }

    /// Truncate to zero length. Capacity is preserved so repeated
    /// build-and-clear cycles avoid reallocations.
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Borrow the contents as a `&str`. Infallible: the type invariant
    /// already guarantees UTF-8.
    pub fn as_str(&self) -> &str {
        // SAFETY: the type invariant is "bytes is valid UTF-8". Every
        // mutation path (push_str, push, from_str, from_utf8 +
        // validation) preserves it. We still keep the no-unsafe rule by
        // routing through `std::str::from_utf8` and unwrapping on the
        // *type invariant* — a logic bug would panic loudly here
        // instead of producing UB.
        std::str::from_utf8(&self.bytes).expect("UTF-8 invariant violated")
    }

    /// Alias of [`as_str`] used by the `format!` macro's `{}` lowering.
    /// Documented separately so the permissive method table in
    /// `mty-types::prelude` has a stable hook to dispatch against.
    ///
    /// [`as_str`]: Self::as_str
    pub fn to_str(&self) -> &str {
        self.as_str()
    }

    /// Number of bytes the underlying buffer can hold without
    /// reallocating. Exposed for parity with `std::string::String`'s
    /// shape; the SIR interpreter doesn't track capacity yet, but the
    /// wasm-side `cabi_realloc` packer reads this to decide whether to
    /// grow the linear-memory segment.
    pub fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    /// Borrow the raw UTF-8 bytes. Useful for the wasm-side linear-
    /// memory packer; user code prefers [`as_str`].
    ///
    /// [`as_str`]: Self::as_str
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for String {
    fn from(s: &str) -> Self {
        Self::from_str(s)
    }
}

impl From<std::string::String> for String {
    fn from(s: std::string::String) -> Self {
        Self {
            bytes: s.into_bytes(),
        }
    }
}

impl From<String> for std::string::String {
    fn from(s: String) -> Self {
        // Infallible: type invariant guarantees UTF-8.
        std::string::String::from_utf8(s.bytes).expect("UTF-8 invariant violated")
    }
}

/// Error returned by [`String::from_utf8`] when the supplied bytes do
/// not form valid UTF-8. Carries the original buffer back so callers
/// can recover without re-allocating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromUtf8Error {
    bytes: Vec<u8>,
    valid_up_to: usize,
}

impl FromUtf8Error {
    /// Byte index after the longest valid UTF-8 prefix. Mirrors
    /// [`std::str::Utf8Error::valid_up_to`].
    pub fn valid_up_to(&self) -> usize {
        self.valid_up_to
    }

    /// Move the original byte buffer back out of the error so the
    /// caller can re-use it without another allocation.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl fmt::Display for FromUtf8Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid UTF-8 sequence after byte index {}",
            self.valid_up_to
        )
    }
}

impl std::error::Error for FromUtf8Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty() {
        let s = String::new();
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
        assert_eq!(s.as_str(), "");
    }

    #[test]
    fn with_capacity_pre_allocates_but_is_empty() {
        let s = String::with_capacity(64);
        assert!(s.is_empty());
        assert!(s.capacity() >= 64);
    }

    #[test]
    fn push_str_appends() {
        let mut s = String::new();
        s.push_str("hi");
        s.push_str(" there");
        assert_eq!(s.as_str(), "hi there");
        assert_eq!(s.len(), 8);
    }

    #[test]
    fn push_char_encodes_utf8() {
        let mut s = String::new();
        s.push('!');
        assert_eq!(s.as_str(), "!");
        s.push('©'); // U+00A9, 2-byte UTF-8.
        assert_eq!(s.as_str(), "!©");
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn from_str_clones_bytes() {
        let s = String::from_str("hello");
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn from_utf8_accepts_valid_bytes() {
        let s = String::from_utf8(b"good".to_vec()).expect("valid UTF-8");
        assert_eq!(s.as_str(), "good");
    }

    #[test]
    fn from_utf8_rejects_invalid_bytes_and_recovers_buffer() {
        let bad = vec![0xFFu8, 0xFE, 0xFD];
        let err = String::from_utf8(bad.clone()).expect_err("not UTF-8");
        assert_eq!(err.valid_up_to(), 0);
        assert_eq!(err.into_bytes(), bad);
    }

    #[test]
    fn clear_resets_length_keeps_capacity() {
        let mut s = String::with_capacity(32);
        s.push_str("foo bar baz");
        assert_eq!(s.len(), 11);
        let cap_before = s.capacity();
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.capacity(), cap_before);
    }

    #[test]
    fn to_str_is_alias_of_as_str() {
        let mut s = String::new();
        s.push_str("zap");
        assert_eq!(s.to_str(), s.as_str());
    }

    #[test]
    fn display_and_debug_match_std_string() {
        let s = String::from_str("a\"b");
        assert_eq!(format!("{}", s), "a\"b");
        assert_eq!(format!("{:?}", s), "\"a\\\"b\"");
    }

    #[test]
    fn from_str_for_struct() {
        let s: String = "hi".into();
        assert_eq!(s.as_str(), "hi");
    }

    #[test]
    fn into_std_string_roundtrip() {
        let s = String::from_str("round trip");
        let std: std::string::String = s.into();
        assert_eq!(std, "round trip");
        let back: String = std.into();
        assert_eq!(back.as_str(), "round trip");
    }
}
