//! `std.fmt` — string-formatting conversion helpers (v0.24 + v0.25).
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
//! ## v0.24 baseline conversions
//!
//! | Mighty method        | Format spec  | Rust analog          |
//! |----------------------|--------------|----------------------|
//! | `to_str()`           | `{}`         | `Display::fmt`       |
//! | `to_hex_str()`       | `{:x}`       | `LowerHex::fmt`      |
//! | `to_hex_upper_str()` | `{:X}`       | `UpperHex::fmt`      |
//! | `to_debug_str()`     | `{:?}`       | `Debug::fmt`         |
//!
//! ## v0.25 Track D additions
//!
//! Two new bare conversions:
//!
//! | Mighty method     | Format spec | Rust analog       |
//! |-------------------|-------------|-------------------|
//! | `to_bin_str()`    | `{:b}`      | `Binary::fmt`     |
//! | `to_oct_str()`    | `{:o}`      | `Octal::fmt`      |
//!
//! Plus **spec helpers** that fold sign/alternate/precision into one
//! call before width-padding kicks in:
//!
//! ```text
//! to_str_spec(sign_plus: Bool, alternate: Bool, precision: U32) -> String
//! to_hex_str_spec(sign_plus, alternate, precision) -> String
//! to_hex_upper_str_spec(sign_plus, alternate, precision) -> String
//! to_debug_str_spec(sign_plus, alternate, precision) -> String
//! to_bin_str_spec(sign_plus, alternate, precision) -> String
//! to_oct_str_spec(sign_plus, alternate, precision) -> String
//! ```
//!
//! `precision = u32::MAX` (4294967295) is the "no precision" sentinel.
//!
//! And a **width-padding tail** on the spec helper's string result:
//!
//! ```text
//! pad_str(width: U32, fill: Char, align: Str) -> String
//! ```
//!
//! `align` accepts `"left"`, `"right"`, `"center"`, or the sentinel
//! `"default"` (picks right for numeric-looking strings, left
//! otherwise — matches Rust's per-formatter defaults).
//!
//! Composed: `format!("{:#05x}", 0xff)` becomes
//! `(0xff).to_hex_str_spec(false, true, 4294967295).pad_str(5, '0', "right")`,
//! which the interpreter renders as `"0x0ff"`.

// ---- v0.24 baseline conversion method names ----

/// Canonical name of the default `{}` conversion method.
pub const METHOD_DISPLAY: &str = "to_str";

/// Canonical name of the lowercase-hex (`{:x}`) conversion method.
pub const METHOD_HEX_LOWER: &str = "to_hex_str";

/// Canonical name of the uppercase-hex (`{:X}`) conversion method.
pub const METHOD_HEX_UPPER: &str = "to_hex_upper_str";

/// Canonical name of the debug (`{:?}`) conversion method.
pub const METHOD_DEBUG: &str = "to_debug_str";

// ---- v0.25 added conversion methods ----

/// Canonical name of the binary (`{:b}`) conversion method.
pub const METHOD_BIN: &str = "to_bin_str";

/// Canonical name of the octal (`{:o}`) conversion method.
pub const METHOD_OCT: &str = "to_oct_str";

// ---- v0.25 spec-helper method names ----

/// Spec helper for Display: `to_str_spec(sign, alt, precision)`.
pub const METHOD_DISPLAY_SPEC: &str = "to_str_spec";

/// Spec helper for hex lower.
pub const METHOD_HEX_LOWER_SPEC: &str = "to_hex_str_spec";

/// Spec helper for hex upper.
pub const METHOD_HEX_UPPER_SPEC: &str = "to_hex_upper_str_spec";

/// Spec helper for debug.
pub const METHOD_DEBUG_SPEC: &str = "to_debug_str_spec";

/// Spec helper for binary.
pub const METHOD_BIN_SPEC: &str = "to_bin_str_spec";

/// Spec helper for octal.
pub const METHOD_OCT_SPEC: &str = "to_oct_str_spec";

/// Width-padding helper on string values.
pub const METHOD_PAD_STR: &str = "pad_str";

/// Sentinel `precision` value used by the spec helpers to mean
/// "no precision specified" (passed as a literal `u32::MAX`).
pub const PRECISION_NONE: u32 = u32::MAX;

/// Every conversion method recognised by the v0.24 baseline `format!`
/// expander. Bare conversions only.
pub const FORMAT_CONV_METHODS: &[&str] = &[
    METHOD_DISPLAY,
    METHOD_HEX_LOWER,
    METHOD_HEX_UPPER,
    METHOD_DEBUG,
    METHOD_BIN,
    METHOD_OCT,
];

/// Every spec-helper method introduced by v0.25 Track D. Codegen
/// backends pattern-match on this to lower the
/// `sign+alt+precision` chained call.
pub const FORMAT_SPEC_METHODS: &[&str] = &[
    METHOD_DISPLAY_SPEC,
    METHOD_HEX_LOWER_SPEC,
    METHOD_HEX_UPPER_SPEC,
    METHOD_DEBUG_SPEC,
    METHOD_BIN_SPEC,
    METHOD_OCT_SPEC,
];

/// Every method name recognised by the v0.25 `format!` expander
/// (baseline conversions + spec helpers + pad_str).
pub const FORMAT_ALL_METHODS: &[&str] = &[
    METHOD_DISPLAY,
    METHOD_HEX_LOWER,
    METHOD_HEX_UPPER,
    METHOD_DEBUG,
    METHOD_BIN,
    METHOD_OCT,
    METHOD_DISPLAY_SPEC,
    METHOD_HEX_LOWER_SPEC,
    METHOD_HEX_UPPER_SPEC,
    METHOD_DEBUG_SPEC,
    METHOD_BIN_SPEC,
    METHOD_OCT_SPEC,
    METHOD_PAD_STR,
];

/// True if `name` is one of the bare conversion methods. Codegen
/// backends pattern-match on this to decide whether to lower a method
/// call via the format-conv fast path.
pub fn is_format_conv_method(name: &str) -> bool {
    FORMAT_CONV_METHODS.contains(&name)
}

/// True if `name` is one of the v0.25 spec-helper methods.
pub fn is_format_spec_method(name: &str) -> bool {
    FORMAT_SPEC_METHODS.contains(&name)
}

/// True if `name` is any format-related method (bare conv, spec helper,
/// or pad_str).
pub fn is_any_format_method(name: &str) -> bool {
    FORMAT_ALL_METHODS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_are_stable() {
        // v0.24 baseline.
        assert_eq!(METHOD_DISPLAY, "to_str");
        assert_eq!(METHOD_HEX_LOWER, "to_hex_str");
        assert_eq!(METHOD_HEX_UPPER, "to_hex_upper_str");
        assert_eq!(METHOD_DEBUG, "to_debug_str");
        // v0.25 additions.
        assert_eq!(METHOD_BIN, "to_bin_str");
        assert_eq!(METHOD_OCT, "to_oct_str");
        assert_eq!(METHOD_DISPLAY_SPEC, "to_str_spec");
        assert_eq!(METHOD_HEX_LOWER_SPEC, "to_hex_str_spec");
        assert_eq!(METHOD_HEX_UPPER_SPEC, "to_hex_upper_str_spec");
        assert_eq!(METHOD_DEBUG_SPEC, "to_debug_str_spec");
        assert_eq!(METHOD_BIN_SPEC, "to_bin_str_spec");
        assert_eq!(METHOD_OCT_SPEC, "to_oct_str_spec");
        assert_eq!(METHOD_PAD_STR, "pad_str");
    }

    #[test]
    fn dispatch_lookup_works() {
        assert!(is_format_conv_method("to_str"));
        assert!(is_format_conv_method("to_hex_str"));
        assert!(is_format_conv_method("to_hex_upper_str"));
        assert!(is_format_conv_method("to_debug_str"));
        assert!(is_format_conv_method("to_bin_str"));
        assert!(is_format_conv_method("to_oct_str"));
        assert!(!is_format_conv_method("to_int"));
        assert!(!is_format_conv_method("read"));
    }

    #[test]
    fn spec_helper_lookup_works() {
        assert!(is_format_spec_method("to_str_spec"));
        assert!(is_format_spec_method("to_hex_str_spec"));
        assert!(is_format_spec_method("to_bin_str_spec"));
        assert!(!is_format_spec_method("to_str"));
        assert!(!is_format_spec_method("pad_str"));
    }

    #[test]
    fn any_format_method_covers_pad_str() {
        assert!(is_any_format_method("pad_str"));
        assert!(is_any_format_method("to_str"));
        assert!(is_any_format_method("to_hex_str_spec"));
        assert!(!is_any_format_method("len"));
    }

    #[test]
    fn methods_lists_have_no_duplicates() {
        for list in [FORMAT_CONV_METHODS, FORMAT_SPEC_METHODS, FORMAT_ALL_METHODS] {
            let mut sorted: Vec<&&str> = list.iter().collect();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), list.len());
        }
    }

    #[test]
    fn precision_none_sentinel_is_u32_max() {
        assert_eq!(PRECISION_NONE, u32::MAX);
        assert_eq!(PRECISION_NONE, 4294967295);
    }
}
