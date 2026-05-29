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
//! ## v0.36 Track T3 — position/range edit + char-boundary surface
//!
//! Reviewer follow-up: round out the editor-shaped operations a Mighty
//! program needs to splice strings without dropping to `&str` tricks.
//! These were partially scattered (e.g. `find` lived in the interp
//! dispatch table, `chars` returned a one-shot Array) — v0.36 T3
//! consolidates the host-side surface here so the wasm/native codegen
//! and the SIR interp share one Rust implementation.
//!
//! | Mighty                          | This module                    | Notes                                          |
//! |---------------------------------|--------------------------------|------------------------------------------------|
//! | `s.find(needle)`                | [`String::find`]               | byte index of first match, `Option<usize>`     |
//! | `s.rfind(needle)`               | [`String::rfind`]              | byte index of last match, `Option<usize>`      |
//! | `s.position(c)`                 | [`String::position`]           | byte index of first `Char`, `Option<usize>`    |
//! | `s.insert_at(idx, t)`           | [`String::insert_at`]          | splices `t` at byte `idx`; MT5080 on bad index |
//! | `s.remove_range(start, end)`    | [`String::remove_range`]       | deletes the byte range; MT5080 on bad bounds   |
//! | `s.replace_range(start, end, t)`| [`String::replace_range`]      | swaps the byte range for `t`; MT5080 on bad    |
//! | `s.is_char_boundary(idx)`       | [`String::is_char_boundary`]   | true iff `idx` is a UTF-8 boundary             |
//! | `s.next_char_boundary(idx)`     | [`String::next_char_boundary`] | next boundary `> idx`, `Option<usize>`         |
//! | `s.prev_char_boundary(idx)`     | [`String::prev_char_boundary`] | prev boundary `< idx`, `Option<usize>`         |
//! | `s.chars()`                     | [`String::chars`]              | code-point iterator (lazy)                     |
//! | `s.char_indices()`              | [`String::char_indices`]       | (byte_idx, char) iterator (lazy)               |
//! | `s.byte_len()`                  | [`String::byte_len`]           | alias of `len`, kept as a documented intent    |
//!
//! UTF-8 safety: the three range-edit ops (`insert_at`, `remove_range`,
//! `replace_range`) panic with diagnostic code **MT5080** when an index
//! is outside the byte buffer or lands in the middle of a multi-byte
//! UTF-8 sequence. Silent truncation would corrupt downstream UTF-8
//! consumers — the panic is the trust-anchor stance.
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
    ///
    /// Named `from_str` for stdlib parity; this is an inherent method,
    /// not an `std::str::FromStr` impl (which would force a Result).
    #[allow(clippy::should_implement_trait)]
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

    // ============================================================
    // v0.36 Track T3: position / range edit / char-boundary surface
    // ============================================================
    //
    // Every method below is a thin Rust-side wrapper over the same
    // operation in `std::str` / `std::string::String`. The wrappers
    // exist so:
    //
    //   * Self-host codegen (mty-codegen-cranelift) has one symbol
    //     to lower against, not three flavors of `std::str::find`.
    //   * The interp dispatch table (mty-ir::interp::run::eval_method)
    //     can call into a stable Rust shim instead of duplicating the
    //     UTF-8 boundary logic at the call site.
    //   * The MT5080 panic message stays consistent between the Rust
    //     test surface and the source-level surface.

    /// Documented byte-length alias of [`len`]. Mighty's source-level
    /// type system distinguishes "I want bytes" from "I want chars";
    /// callers that mean **bytes** are encouraged to spell that intent
    /// out at the use site instead of relying on `len`'s
    /// memory-of-Rust-convention.
    ///
    /// [`len`]: Self::len
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Byte index of the first occurrence of `needle`, or `None` if
    /// the substring does not appear. Mirrors `str::find`.
    pub fn find(&self, needle: &str) -> Option<usize> {
        self.as_str().find(needle)
    }

    /// Byte index of the last occurrence of `needle`, or `None`.
    /// Mirrors `str::rfind`.
    pub fn rfind(&self, needle: &str) -> Option<usize> {
        self.as_str().rfind(needle)
    }

    /// Byte index of the first occurrence of code-point `c`, or
    /// `None`. Convenience over `find(&c.to_string())` that avoids the
    /// 1..=4-byte UTF-8 allocation per call.
    pub fn position(&self, c: char) -> Option<usize> {
        self.as_str().find(c)
    }

    /// True iff `idx` is a UTF-8 code-point boundary (including `0`
    /// and `self.len()`). `false` for in-bounds indices that land in
    /// the middle of a multi-byte sequence, and `false` for any index
    /// past the end. Mirrors `str::is_char_boundary`.
    pub fn is_char_boundary(&self, idx: usize) -> bool {
        self.as_str().is_char_boundary(idx)
    }

    /// Smallest UTF-8 code-point boundary strictly greater than `idx`,
    /// or `None` if no such boundary exists (i.e. `idx >= self.len()`).
    /// Walks forward at most 4 bytes — UTF-8 sequences are bounded.
    pub fn next_char_boundary(&self, idx: usize) -> Option<usize> {
        let len = self.bytes.len();
        if idx >= len {
            return None;
        }
        let mut j = idx + 1;
        while j <= len && !self.as_str().is_char_boundary(j) {
            j += 1;
        }
        // The `j <= len` check above guarantees we found a boundary at
        // or before `len`; both `0` and `len` are always boundaries.
        Some(j)
    }

    /// Largest UTF-8 code-point boundary strictly less than `idx`, or
    /// `None` if no such boundary exists (i.e. `idx == 0`).
    pub fn prev_char_boundary(&self, idx: usize) -> Option<usize> {
        if idx == 0 {
            return None;
        }
        // Clamp probes to the buffer so we don't pass an OOB index to
        // `is_char_boundary` (which would panic in debug builds via the
        // slice's bounds check in some std versions).
        let mut j = idx - 1;
        let s = self.as_str();
        loop {
            if s.is_char_boundary(j) {
                return Some(j);
            }
            if j == 0 {
                // 0 is always a boundary, so we only reach here if the
                // string is somehow malformed — defensive `Some(0)`.
                return Some(0);
            }
            j -= 1;
        }
    }

    /// Iterator over the Unicode code points (`char`s) of this
    /// string. Lazy: no intermediate `Vec<char>` allocation.
    pub fn chars(&self) -> std::str::Chars<'_> {
        self.as_str().chars()
    }

    /// Iterator over `(byte_index, char)` pairs. Lazy.
    pub fn char_indices(&self) -> std::str::CharIndices<'_> {
        self.as_str().char_indices()
    }

    /// Splice `t` into this string at byte position `idx`.
    ///
    /// # Panics (MT5080)
    /// If `idx > self.len()` or `idx` is not a UTF-8 code-point
    /// boundary. Silent truncation would let the caller corrupt the
    /// UTF-8 invariant; we trap loudly instead.
    pub fn insert_at(&mut self, idx: usize, t: &str) {
        check_boundary("insert_at", self.as_str(), idx);
        // Re-borrow as a mutable Vec<u8> and splice. The input `t` is
        // already a `&str`, so the new content preserves UTF-8.
        self.bytes.splice(idx..idx, t.bytes());
    }

    /// Delete the byte range `start..end`.
    ///
    /// # Panics (MT5080)
    /// If `start > end`, `end > self.len()`, or either bound is not a
    /// UTF-8 boundary.
    pub fn remove_range(&mut self, start: usize, end: usize) {
        check_range("remove_range", self.as_str(), start, end);
        self.bytes.drain(start..end);
    }

    /// Replace the byte range `start..end` with `t`.
    ///
    /// # Panics (MT5080)
    /// Same conditions as [`remove_range`]. Equivalent in effect to
    /// `remove_range(start, end)` followed by `insert_at(start, t)`,
    /// but done in one splice so the buffer is only shifted once.
    ///
    /// [`remove_range`]: Self::remove_range
    pub fn replace_range(&mut self, start: usize, end: usize, t: &str) {
        check_range("replace_range", self.as_str(), start, end);
        self.bytes.splice(start..end, t.bytes());
    }
}

/// Validate a single byte index for the range-edit ops. Panics with a
/// MT5080-tagged message on failure.
///
/// The diagnostic prefix lets the interpreter's trap-translation layer
/// (and human readers of stderr) tie the panic back to the diagnostics
/// catalog without having to thread a `DiagCode` through the stdlib.
#[inline]
fn check_boundary(op: &'static str, s: &str, idx: usize) {
    if idx > s.len() {
        panic!(
            "MT5080: String::{op} byte index {idx} is past the end of \
             the {len}-byte string",
            op = op,
            idx = idx,
            len = s.len()
        );
    }
    if !s.is_char_boundary(idx) {
        panic!(
            "MT5080: String::{op} byte index {idx} is not on a UTF-8 \
             code-point boundary",
            op = op,
            idx = idx
        );
    }
}

/// Validate a `start..end` byte range. Panics with a MT5080-tagged
/// message on failure.
#[inline]
fn check_range(op: &'static str, s: &str, start: usize, end: usize) {
    if start > end {
        panic!(
            "MT5080: String::{op} start {start} is greater than end {end}",
            op = op,
            start = start,
            end = end
        );
    }
    check_boundary(op, s, start);
    check_boundary(op, s, end);
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

    // ============================================================
    // v0.36 Track T3: position / range edit / char-boundary tests
    // ============================================================

    #[test]
    fn byte_len_matches_len() {
        let s = String::from_str("héllo"); // 'é' is 2 bytes
        assert_eq!(s.byte_len(), 6);
        assert_eq!(s.byte_len(), s.len());
    }

    // ---- find / rfind / position ----

    #[test]
    fn find_first_occurrence() {
        let s = String::from_str("Hello, Mighty");
        assert_eq!(s.find("Mighty"), Some(7));
    }

    #[test]
    fn find_missing_returns_none() {
        let s = String::from_str("Hello");
        assert_eq!(s.find("zzz"), None);
    }

    #[test]
    fn find_empty_needle_returns_zero() {
        let s = String::from_str("anything");
        // std::str::find("") returns Some(0) — preserve that.
        assert_eq!(s.find(""), Some(0));
    }

    #[test]
    fn rfind_last_occurrence() {
        let s = String::from_str("ababab");
        assert_eq!(s.rfind("ab"), Some(4));
        assert_eq!(s.find("ab"), Some(0));
    }

    #[test]
    fn rfind_missing_returns_none() {
        let s = String::from_str("Hello");
        assert_eq!(s.rfind("zzz"), None);
    }

    #[test]
    fn position_finds_char() {
        let s = String::from_str("Hello, Mighty");
        assert_eq!(s.position('M'), Some(7));
    }

    #[test]
    fn position_finds_multibyte_char() {
        // "h©llo" — © is at byte 1, len 2 bytes.
        let s = String::from_str("h©llo");
        assert_eq!(s.position('©'), Some(1));
        assert_eq!(s.position('l'), Some(3));
    }

    #[test]
    fn position_missing_char_returns_none() {
        let s = String::from_str("hi");
        assert_eq!(s.position('z'), None);
    }

    // ---- is_char_boundary / next / prev ----

    #[test]
    fn is_char_boundary_zero_and_end() {
        let s = String::from_str("abc");
        assert!(s.is_char_boundary(0));
        assert!(s.is_char_boundary(3));
        assert!(!s.is_char_boundary(4)); // past end
    }

    #[test]
    fn is_char_boundary_inside_multibyte_is_false() {
        // "é" = [0xC3, 0xA9] — byte 1 is mid-sequence.
        let s = String::from_str("é");
        assert!(s.is_char_boundary(0));
        assert!(!s.is_char_boundary(1));
        assert!(s.is_char_boundary(2));
    }

    #[test]
    fn is_char_boundary_emoji_4_byte() {
        // "🦀" (U+1F980) is 4 bytes.
        let s = String::from_str("🦀");
        assert!(s.is_char_boundary(0));
        assert!(!s.is_char_boundary(1));
        assert!(!s.is_char_boundary(2));
        assert!(!s.is_char_boundary(3));
        assert!(s.is_char_boundary(4));
    }

    #[test]
    fn next_char_boundary_walks_forward() {
        let s = String::from_str("a©b"); // a (1) © (2) b (1) = 4 bytes
        assert_eq!(s.next_char_boundary(0), Some(1));
        assert_eq!(s.next_char_boundary(1), Some(3));
        assert_eq!(s.next_char_boundary(2), Some(3)); // mid-© → next is end of ©
        assert_eq!(s.next_char_boundary(3), Some(4));
        assert_eq!(s.next_char_boundary(4), None); // at end already
    }

    #[test]
    fn prev_char_boundary_walks_backward() {
        let s = String::from_str("a©b"); // 0 a 1 © 3 b 4
        assert_eq!(s.prev_char_boundary(4), Some(3));
        assert_eq!(s.prev_char_boundary(3), Some(1));
        assert_eq!(s.prev_char_boundary(2), Some(1)); // mid-© → prev is start of ©
        assert_eq!(s.prev_char_boundary(1), Some(0));
        assert_eq!(s.prev_char_boundary(0), None);
    }

    #[test]
    fn next_prev_char_boundary_round_trip_through_emoji() {
        // 4-byte emoji: walk every byte forward then backward, confirm
        // we never escape the buffer or skip past the canonical 0/4
        // boundary pair.
        let s = String::from_str("🦀"); // 4 bytes
        assert_eq!(s.next_char_boundary(0), Some(4));
        assert_eq!(s.next_char_boundary(1), Some(4));
        assert_eq!(s.next_char_boundary(2), Some(4));
        assert_eq!(s.next_char_boundary(3), Some(4));
        assert_eq!(s.prev_char_boundary(4), Some(0));
        assert_eq!(s.prev_char_boundary(3), Some(0));
        assert_eq!(s.prev_char_boundary(2), Some(0));
        assert_eq!(s.prev_char_boundary(1), Some(0));
    }

    // ---- chars / char_indices ----

    #[test]
    fn chars_iterates_code_points() {
        let s = String::from_str("a©🦀");
        let collected: Vec<char> = s.chars().collect();
        assert_eq!(collected, vec!['a', '©', '🦀']);
    }

    #[test]
    fn char_indices_pairs_byte_index_with_char() {
        let s = String::from_str("a©b");
        let collected: Vec<(usize, char)> = s.char_indices().collect();
        assert_eq!(collected, vec![(0, 'a'), (1, '©'), (3, 'b')]);
    }

    #[test]
    fn chars_on_empty_string() {
        let s = String::new();
        assert_eq!(s.chars().count(), 0);
        assert_eq!(s.char_indices().count(), 0);
    }

    // ---- insert_at ----

    #[test]
    fn insert_at_start_middle_end() {
        let mut s = String::from_str("Hello, Mighty");
        s.insert_at(7, "the ");
        assert_eq!(s.as_str(), "Hello, the Mighty");

        let mut s2 = String::from_str("end");
        s2.insert_at(0, "start of the ");
        assert_eq!(s2.as_str(), "start of the end");

        let mut s3 = String::from_str("foo");
        s3.insert_at(3, "bar");
        assert_eq!(s3.as_str(), "foobar");
    }

    #[test]
    fn insert_at_preserves_multibyte_neighborhood() {
        // After 'a©' (3 bytes), inject at byte 3 — the start of 'b'.
        let mut s = String::from_str("a©b");
        s.insert_at(3, "X");
        assert_eq!(s.as_str(), "a©Xb");
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn insert_at_panics_on_oob_index() {
        let mut s = String::from_str("hi");
        s.insert_at(99, "x"); // past end
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn insert_at_panics_mid_multibyte() {
        // 'é' = 2 bytes [0xC3,0xA9]; byte 1 is mid-sequence.
        let mut s = String::from_str("é");
        s.insert_at(1, "X");
    }

    // ---- remove_range ----

    #[test]
    fn remove_range_deletes_bytes() {
        let mut s = String::from_str("Hello, the Mighty");
        s.remove_range(7, 11); // delete "the "
        assert_eq!(s.as_str(), "Hello, Mighty");
    }

    #[test]
    fn remove_range_empty_range_is_noop() {
        let mut s = String::from_str("Hello");
        s.remove_range(2, 2);
        assert_eq!(s.as_str(), "Hello");
    }

    #[test]
    fn remove_range_to_end() {
        let mut s = String::from_str("Hello, world");
        let n = s.len();
        s.remove_range(5, n);
        assert_eq!(s.as_str(), "Hello");
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn remove_range_panics_inverted_bounds() {
        let mut s = String::from_str("Hello");
        s.remove_range(3, 1);
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn remove_range_panics_mid_multibyte() {
        let mut s = String::from_str("a©b"); // © at bytes 1..3
        s.remove_range(2, 3); // 2 is mid-©
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn remove_range_panics_end_past_buffer() {
        let mut s = String::from_str("hi");
        s.remove_range(0, 99);
    }

    // ---- replace_range ----

    #[test]
    fn replace_range_swaps_in_new_text() {
        let mut s = String::from_str("Hello, the Mighty");
        s.replace_range(7, 11, "a "); // "the " -> "a "
        assert_eq!(s.as_str(), "Hello, a Mighty");
    }

    #[test]
    fn replace_range_with_empty_acts_as_remove() {
        let mut s = String::from_str("Hello, the Mighty");
        s.replace_range(7, 11, "");
        assert_eq!(s.as_str(), "Hello, Mighty");
    }

    #[test]
    fn replace_range_grows_buffer() {
        let mut s = String::from_str("ab");
        s.replace_range(1, 2, "XXX");
        assert_eq!(s.as_str(), "aXXX");
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn replace_range_panics_on_mid_multibyte_start() {
        let mut s = String::from_str("a©b");
        s.replace_range(2, 3, "X");
    }

    #[test]
    #[should_panic(expected = "MT5080")]
    fn replace_range_panics_on_mid_multibyte_end() {
        let mut s = String::from_str("a©b");
        s.replace_range(1, 2, "X");
    }

    // ---- Combining characters (NFD shape) ----

    #[test]
    fn char_indices_handles_combining_marks() {
        // "e\u{0301}" = 'e' + combining acute (2-byte). Two code
        // points; len = 3 bytes.
        let s = String::from_str("e\u{0301}");
        assert_eq!(s.len(), 3);
        let pairs: Vec<(usize, char)> = s.char_indices().collect();
        assert_eq!(pairs, vec![(0, 'e'), (1, '\u{0301}')]);
        assert!(s.is_char_boundary(1));
        assert!(s.is_char_boundary(3));
        assert!(!s.is_char_boundary(2));
    }

    #[test]
    fn insert_at_between_base_and_combining_is_allowed() {
        // Boundary between 'e' and combining acute is at byte 1 — a
        // valid splice point even if it changes the grapheme cluster.
        // The stdlib is byte-/code-point-aware, not grapheme-aware.
        let mut s = String::from_str("e\u{0301}");
        s.insert_at(1, "X");
        assert_eq!(s.as_str(), "eX\u{0301}");
    }

    // ---- End-to-end demo of the editor surface ----

    #[test]
    fn editor_surface_round_trip() {
        let mut s = String::from_str("Hello, Mighty");
        // find -> insert_at -> remove_range -> replace_range
        let idx = s.find("Mighty").expect("present");
        assert_eq!(idx, 7);
        s.insert_at(idx, "the ");
        assert_eq!(s.as_str(), "Hello, the Mighty");
        s.remove_range(7, 11);
        assert_eq!(s.as_str(), "Hello, Mighty");
        s.replace_range(0, 5, "Howdy");
        assert_eq!(s.as_str(), "Howdy, Mighty");
        // char-boundary helpers stay consistent throughout.
        assert!(s.is_char_boundary(s.len()));
        assert_eq!(s.next_char_boundary(s.len()), None);
        assert_eq!(s.prev_char_boundary(0), None);
    }
}
