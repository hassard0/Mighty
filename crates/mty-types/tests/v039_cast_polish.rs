//! v0.39 T2 — cast surface polish:
//!   * Bool ↔ Int (both directions accepted; was Bool→Int only).
//!   * Reference cast `&T as *T` / `&mut T as *mut T` (Ref→Ref with
//!     matching inner).
//!   * Char codepoint validity for literal `Int as Char` casts
//!     (MT2028 INVALID_CODEPOINT).
//!
//! These tests sit alongside `mt2027_invalid_cast.rs`, which still
//! owns the v0.37 T2 acceptance/rejection matrix; this file extends
//! the matrix with the v0.39 T2 polish items.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package;

fn diag_codes(src: &str) -> Vec<String> {
    let parsed = parse_source(src.into(), "v039_cast_polish.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !any_lower_err {
        diags.extend(check_package(&pkg));
    }
    diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect()
}

fn assert_accepted(src: &str, label: &str) {
    let codes = diag_codes(src);
    assert!(
        !codes.contains(&"MT2027".to_string()) && !codes.contains(&"MT2028".to_string()),
        "{} must be accepted, got {:?}",
        label,
        codes
    );
}

fn assert_rejected_mt2027(src: &str, label: &str) {
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2027".to_string()),
        "{} must emit MT2027, got {:?}",
        label,
        codes
    );
}

fn assert_rejected_mt2028(src: &str, label: &str) {
    let codes = diag_codes(src);
    assert!(
        codes.contains(&"MT2028".to_string()),
        "{} must emit MT2028, got {:?}",
        label,
        codes
    );
}

// ─────────────────────────────────────────────────────────────────────
// Bool ↔ Int  (12 tests — round-trip, sign-extension, widths, both dirs)
// ─────────────────────────────────────────────────────────────────────

#[test]
fn bool_to_i32_accepted() {
    // Long-standing direction. Re-asserted here so the matrix is
    // self-contained.
    assert_accepted("fn f(b: Bool) -> I32 { b as I32 }", "Bool → I32");
}

#[test]
fn bool_to_i64_accepted() {
    assert_accepted("fn f(b: Bool) -> I64 { b as I64 }", "Bool → I64");
}

#[test]
fn bool_to_u8_accepted() {
    assert_accepted("fn f(b: Bool) -> U8 { b as U8 }", "Bool → U8");
}

#[test]
fn bool_to_isize_accepted() {
    assert_accepted("fn f(b: Bool) -> ISize { b as ISize }", "Bool → ISize");
}

#[test]
fn i32_to_bool_accepted_v039() {
    // The new v0.39 T2 direction. Pre-v0.39 this emitted MT2027 because
    // `is_valid_cast` only allowed Bool → Int.
    assert_accepted(
        "fn f(x: I32) -> Bool { x as Bool }",
        "I32 → Bool (v0.39 T2)",
    );
}

#[test]
fn i64_to_bool_accepted_v039() {
    assert_accepted(
        "fn f(x: I64) -> Bool { x as Bool }",
        "I64 → Bool (v0.39 T2)",
    );
}

#[test]
fn u8_to_bool_accepted_v039() {
    assert_accepted("fn f(x: U8) -> Bool { x as Bool }", "U8 → Bool (v0.39 T2)");
}

#[test]
fn u64_to_bool_accepted_v039() {
    assert_accepted(
        "fn f(x: U64) -> Bool { x as Bool }",
        "U64 → Bool (v0.39 T2)",
    );
}

#[test]
fn bool_round_trip_via_i32() {
    // `(b as I32) as Bool` must round-trip cleanly (typeck-wise). The
    // runtime semantics are tested separately in the codegen suite.
    assert_accepted(
        "fn f(b: Bool) -> Bool { (b as I32) as Bool }",
        "Bool round-trip via I32",
    );
}

#[test]
fn bool_round_trip_via_u8() {
    // Tight round-trip — Bool is stored at I8 width so this is the
    // narrowest path. Documents that no widening lossiness fires.
    assert_accepted(
        "fn f(b: Bool) -> Bool { (b as U8) as Bool }",
        "Bool round-trip via U8",
    );
}

#[test]
fn float_to_bool_still_rejected() {
    // v0.39 T2 explicitly leaves Float ↔ Bool out — there's no obvious
    // policy for NaN. Documented in docs/reference/casts.md.
    assert_rejected_mt2027("fn f(x: F32) -> Bool { x as Bool }", "F32 → Bool");
}

#[test]
fn bool_to_float_still_rejected() {
    assert_rejected_mt2027("fn f(b: Bool) -> F64 { b as F64 }", "Bool → F64");
}

// ─────────────────────────────────────────────────────────────────────
// Reference cast `&T as *T`  (6 tests — positive + negative)
// ─────────────────────────────────────────────────────────────────────
//
// Note: the surface parser maps both `&T` and `*T` onto TYPE_BORROW
// (slice-1 simplification), so the cast resolves as Ref→Ref with the
// inner-type unify check in `is_valid_cast`.

#[test]
fn ref_i32_to_ptr_i32_accepted_v039() {
    // Matching inner types — should round-trip cleanly through the
    // typeck. Pre-v0.39 this emitted MT2027 because `is_valid_cast`
    // didn't know about Ref→Ref.
    assert_accepted(
        "fn f(x: &I32) -> *I32 { x as *I32 }",
        "&I32 → *I32 (v0.39 T2)",
    );
}

#[test]
fn mut_ref_to_mut_ptr_accepted_v039() {
    // `&mut T as *mut T` — same shape with mutability flag. We don't
    // enforce mut→mut symmetry yet (the IR ref carries the flag but the
    // cast surface is permissive on the mutability bit), but the inner
    // types must still unify.
    assert_accepted(
        "fn f(x: &mut I32) -> *mut I32 { x as *mut I32 }",
        "&mut I32 → *mut I32 (v0.39 T2)",
    );
}

#[test]
fn ref_u8_to_ptr_u8_accepted_v039() {
    // Equivalent to the FFI string→ptr coercion's typeck surface,
    // now spellable as an explicit cast outside an extern-c call site.
    assert_accepted("fn f(x: &U8) -> *U8 { x as *U8 }", "&U8 → *U8 (v0.39 T2)");
}

#[test]
fn ref_inner_mismatch_rejected() {
    // `&U8 as *I32` — inner type mismatch. The cast surface allows
    // Ref→Ref only when inner types unify; this must NOT silently
    // bit-cast across pointee types.
    assert_rejected_mt2027(
        "fn f(x: &U8) -> *I32 { x as *I32 }",
        "&U8 → *I32 (inner mismatch)",
    );
}

#[test]
fn ref_to_int_still_rejected() {
    // Address-of-int via `as` would be an `unsafe`-only operation;
    // surface `as` doesn't admit it. The user must spell it via a
    // dedicated builtin in an `unsafe` block.
    assert_rejected_mt2027("fn f(x: &I32) -> USize { x as USize }", "&I32 → USize");
}

#[test]
fn int_to_ptr_still_rejected() {
    // Symmetric to the previous: `as` doesn't admit Int → Ref/RawPtr.
    // `unsafe { raw_ptr(addr) }` is the supported builtin.
    assert_rejected_mt2027("fn f(x: USize) -> *I32 { x as *I32 }", "USize → *I32");
}

// ─────────────────────────────────────────────────────────────────────
// Char codepoint validity  (8 tests — literal MT2028 + accepted shapes)
// ─────────────────────────────────────────────────────────────────────

#[test]
fn char_literal_in_range_accepted() {
    // 'A' = 0x41.
    assert_accepted("fn f() -> Char { 0x41 as Char }", "0x41 as Char");
}

#[test]
fn char_literal_zero_accepted() {
    // U+0000 is a valid (if unusual) scalar value.
    assert_accepted("fn f() -> Char { 0 as Char }", "0 as Char");
}

#[test]
fn char_literal_max_accepted() {
    // U+10FFFF — the top of the Unicode scalar value range.
    assert_accepted("fn f() -> Char { 0x10FFFF as Char }", "0x10FFFF as Char");
}

#[test]
fn char_literal_above_max_rejected_mt2028() {
    // 0x110000 — first value past the SMR. Must emit MT2028, not silently
    // pass through to codegen.
    assert_rejected_mt2028("fn f() -> Char { 0x110000 as Char }", "0x110000 as Char");
}

#[test]
fn char_literal_surrogate_d800_rejected_mt2028() {
    // 0xD800 — start of the UTF-16 surrogate gap. Mighty's `Char`
    // (like Rust's) excludes these; allowing them would corrupt
    // UTF-8 invariants when the char flowed into a String.
    assert_rejected_mt2028(
        "fn f() -> Char { 0xD800 as Char }",
        "0xD800 as Char (surrogate)",
    );
}

#[test]
fn char_literal_surrogate_dfff_rejected_mt2028() {
    // 0xDFFF — end of the surrogate gap. Symmetric to the D800 test.
    assert_rejected_mt2028(
        "fn f() -> Char { 0xDFFF as Char }",
        "0xDFFF as Char (surrogate)",
    );
}

#[test]
fn char_literal_just_below_surrogate_accepted() {
    // 0xD7FF — last codepoint before the surrogate gap. Must be
    // accepted; tests the boundary condition on the low side.
    assert_accepted(
        "fn f() -> Char { 0xD7FF as Char }",
        "0xD7FF as Char (boundary)",
    );
}

#[test]
fn char_non_literal_cast_rejected_v040() {
    // v0.40 T3 flipped the v0.39 T2 stance: non-literal `Int as Char`
    // now rejects at the cast surface with MT2027 + a fix-suggestion
    // pointing at `Char.from_u32(value) -> Option[Char]`. The full
    // suite for the v0.40 T3 behaviour lives in
    // `v040_cast_char_runtime.rs`; this entry preserves the historical
    // test slot so a `git blame` of the test name picks up the pivot.
    let codes = diag_codes("fn f(x: U32) -> Char { x as Char }");
    assert!(
        codes.contains(&"MT2027".to_string()) && !codes.contains(&"MT2028".to_string()),
        "non-literal U32 → Char must emit MT2027 (v0.40 T3); got {:?}",
        codes
    );
}
