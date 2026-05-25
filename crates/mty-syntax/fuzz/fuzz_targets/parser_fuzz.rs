//! Parser fuzz target.
//!
//! Goal: `mty_syntax::parse` must never panic, abort, or stack-overflow
//! on arbitrary input. Errors are fine — they're recorded in
//! `ParseResult.errors`. Crashes are not.
//!
//! Seed corpus: see `fuzz/corpus/parser_fuzz/` (examples + selfhost
//! sources). Reproduce with:
//!
//! ```bash
//! cargo +nightly fuzz run parser_fuzz <crash_input_path>
//! ```

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = mty_syntax::parse(s);
    }
});
