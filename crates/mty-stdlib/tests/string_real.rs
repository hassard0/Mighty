//! Integration tests for the v0.25 Track E `std.String` real impl.
//!
//! These mirror the *user-facing contract* — every assertion here is
//! something a Mighty user would write (or that the v0.24 `format!`
//! macro expands to). The inline `mod tests` in `src/string.rs`
//! exercises internal invariants (UTF-8 round-trip, FromUtf8Error
//! buffer recovery, Display/Debug impls); this file pins down the
//! API surface.
//!
//! Eight + tests, per the slice spec.

use mty_stdlib::string::String as MtyString;

#[test]
fn string_new_empty() {
    let s = MtyString::new();
    assert_eq!(s.len(), 0);
    assert!(s.is_empty());
}

#[test]
fn string_with_capacity() {
    // The exact capacity returned by Vec::with_capacity is allowed to
    // round up to the allocator's bucket size, but it must be at least
    // what the caller asked for. Match the Rust std guarantee.
    let s = MtyString::with_capacity(128);
    assert!(s.is_empty());
    assert!(s.capacity() >= 128);
}

#[test]
fn string_push_str_concats_two_pieces() {
    let mut s = MtyString::new();
    s.push_str("hi");
    s.push_str(" there");
    assert_eq!(s.as_str(), "hi there");
}

#[test]
fn string_push_char_emits_one_codepoint() {
    let mut s = MtyString::new();
    s.push('!');
    assert_eq!(s.as_str(), "!");
    assert_eq!(s.len(), 1);
}

#[test]
fn string_len_is_byte_count_not_char_count() {
    // "a©" is two chars, but three UTF-8 bytes ('a' = 1, '©' = 2).
    let s = MtyString::from_str("a©");
    assert_eq!(s.len(), 3, "len() reports bytes, NOT chars");
}

#[test]
fn string_clear_empties_and_keeps_capacity() {
    let mut s = MtyString::with_capacity(32);
    s.push_str("foo");
    assert!(!s.is_empty());
    let cap = s.capacity();
    s.clear();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s.capacity(), cap);
}

#[test]
fn string_to_str_is_alias_of_as_str() {
    // `to_str` is what the v0.24 `format!` macro's `{}` lowering
    // expands to; it MUST match `as_str` exactly. Test the contract
    // so a refactor that diverges them fails loudly.
    let mut s = MtyString::new();
    s.push_str("widget");
    assert_eq!(s.as_str(), "widget");
    assert_eq!(s.to_str(), "widget");
    assert_eq!(s.to_str(), s.as_str());
}

#[test]
fn string_from_utf8_round_trips_valid_input() {
    let bytes = "hello world".as_bytes().to_vec();
    let s = MtyString::from_utf8(bytes).expect("valid UTF-8");
    assert_eq!(s.as_str(), "hello world");
    assert_eq!(s.len(), 11);
}

#[test]
fn string_from_utf8_rejects_invalid_bytes() {
    // 0xFF is not a valid UTF-8 leading byte under any prefix.
    let err = MtyString::from_utf8(vec![b'a', 0xFF]).expect_err("invalid UTF-8");
    assert_eq!(err.valid_up_to(), 1, "valid prefix is 'a' only");
}

#[test]
fn string_push_then_clear_then_push_reuses_buffer() {
    // The realistic interpreter / format! usage pattern: build a
    // string in a buffer, log it, clear, build again. Capacity must
    // survive the clear.
    let mut s = MtyString::with_capacity(64);
    s.push_str("first attempt");
    let cap = s.capacity();
    s.clear();
    s.push_str("second attempt");
    assert_eq!(s.as_str(), "second attempt");
    assert_eq!(s.capacity(), cap);
}

#[test]
fn string_supports_format_macro_concat_shape() {
    // The v0.24 `format!` macro lowers `format!("a{}c", x)` into
    // string concatenation that builds a `String` piecewise. Pin
    // down the shape that the Mighty source canvas-game agent uses.
    let mut s = MtyString::new();
    s.push_str("score: ");
    let n: u32 = 7;
    s.push_str(&n.to_string());
    s.push(' ');
    s.push_str("pts");
    assert_eq!(s.as_str(), "score: 7 pts");
}
