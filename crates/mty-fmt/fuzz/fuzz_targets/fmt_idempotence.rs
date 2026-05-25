//! Formatter idempotence fuzz target.
//!
//! Property under test: `format(format(x)) == format(x)` for any
//! `x` that the parser can build a green tree from. The parser is
//! error-tolerant — it always returns *some* green tree — so we feed
//! arbitrary UTF-8 in, format it once, parse + format the result, and
//! assert the two strings are equal.
//!
//! This catches:
//! - non-idempotent whitespace rules (e.g. extra blank lines on each
//!   pass)
//! - re-emission of trailing trivia that drifts under re-parse
//! - any panic in either the formatter or the trivia engine

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let once = mty_fmt::format(mty_syntax::parse(s).green);
        let twice = mty_fmt::format(mty_syntax::parse(&once).green);
        assert_eq!(once, twice, "fmt not idempotent");
    }
});
