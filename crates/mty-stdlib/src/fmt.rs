//! `std.fmt` — string-formatting conversion helpers (v0.24 Track B).
//!
//! This module is the *runtime contract* for the `format!` builtin
//! macro. The macro lives in `mty-macros/src/stdlib/format.rs`; at
//! expansion time it rewrites `format!("...{:x}...", n)` into
//! `("..." + (n).to_hex_str() + "...")`. The conversion methods on the
//! right are dispatched by the SIR interpreter
//! (`mty-ir/src/interp/run.rs`) using `Value::as_str` plus per-method
//! formatting hooks; they're declared as permissive built-in methods
//! in `mty-types::prelude` so the typechecker accepts them on any
//! receiver and returns `String`.
//!
//! ## Method contract
//!
//! Every conversion method takes no arguments, has receiver type
//! `&self` (in pure-Mighty terms: `(self) -> String`), and is pure
//! (no effects). Implementations may not panic — the contract is
//! "best-effort string rendering" because `format!` is meant to be a
//! drop-in for `println!`-style ergonomics, not a serialization
//! protocol.
//!
//! | Mighty method        | Format spec  | Rust analog          |
//! |----------------------|--------------|----------------------|
//! | `to_str()`           | `{}`         | `Display::fmt`       |
//! | `to_hex_str()`       | `{:x}`       | `LowerHex::fmt`      |
//! | `to_hex_upper_str()` | `{:X}`       | `UpperHex::fmt`      |
//! | `to_debug_str()`     | `{:?}`       | `Debug::fmt`         |
//!
//! ## Why no Rust impls here?
//!
//! Runtime dispatch lives entirely in the SIR interpreter — the
//! formatting methods are dispatched by name against `Value::*` and
//! produce a `Value::Str`. There's no FFI layer to plug into for `std.fmt`
//! the way `std.json` or `std.http` do, so this module deliberately
//! ships *just* the documented constants for the canonical method
//! names. Future codegen backends (cranelift/wasm) that materialize
//! these methods into machine code will pattern-match on these
//! constants.
//!
//! ## v0.25 follow-ups
//!
//! The `format!` parser rejects width / precision / alignment specs
//! today (MT6010). When those land, two new methods will join the
//! table:
//!
//! - `to_str_pad(width: USize, fill: Char, align: Align) -> String`
//! - `to_str_precision(places: USize) -> String` (floats only)
//!
//! See `dev/history/notes/FORMAT_MACRO_V0_24_NOTES.md` for the
//! roadmap.

/// Canonical name of the default `{}` conversion method. The parser
/// emits this for placeholders with no spec.
pub const METHOD_DISPLAY: &str = "to_str";

/// Canonical name of the lowercase-hex (`{:x}`) conversion method.
pub const METHOD_HEX_LOWER: &str = "to_hex_str";

/// Canonical name of the uppercase-hex (`{:X}`) conversion method.
pub const METHOD_HEX_UPPER: &str = "to_hex_upper_str";

/// Canonical name of the debug (`{:?}`) conversion method.
pub const METHOD_DEBUG: &str = "to_debug_str";

/// Every conversion method recognised by the v0.24 `format!` expander.
/// Used by codegen backends to register import lowering.
pub const FORMAT_CONV_METHODS: &[&str] = &[
    METHOD_DISPLAY,
    METHOD_HEX_LOWER,
    METHOD_HEX_UPPER,
    METHOD_DEBUG,
];

/// True if `name` is one of the v0.24 conversion methods. Codegen
/// backends pattern-match on this to decide whether to lower a method
/// call via the format-conv fast path.
pub fn is_format_conv_method(name: &str) -> bool {
    FORMAT_CONV_METHODS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_stable() {
        // These names cross the macro / typechecker / interp boundary;
        // changing them is a versioned spec amendment.
        assert_eq!(METHOD_DISPLAY, "to_str");
        assert_eq!(METHOD_HEX_LOWER, "to_hex_str");
        assert_eq!(METHOD_HEX_UPPER, "to_hex_upper_str");
        assert_eq!(METHOD_DEBUG, "to_debug_str");
    }

    #[test]
    fn dispatch_lookup_works() {
        assert!(is_format_conv_method("to_str"));
        assert!(is_format_conv_method("to_hex_str"));
        assert!(is_format_conv_method("to_hex_upper_str"));
        assert!(is_format_conv_method("to_debug_str"));
        assert!(!is_format_conv_method("to_int"));
        assert!(!is_format_conv_method("read"));
    }

    #[test]
    fn methods_list_has_no_duplicates() {
        let mut sorted: Vec<&&str> = FORMAT_CONV_METHODS.iter().collect();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), FORMAT_CONV_METHODS.len());
    }
}
