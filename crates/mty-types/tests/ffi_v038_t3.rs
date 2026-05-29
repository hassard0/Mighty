//! v0.38 Track T3 — FFI follow-ups from v0.37 T3.
//!
//! Three surfaces:
//!   1. Returned-struct binding (row 7 of extern_c matrix)
//!   2. Function pointer surface (row 11)
//!   3. `#[ffi_nul_ok]` attribute on extern fn params
//!
//! These tests pin the typeck side. Lowering coverage lives in the
//! cranelift backend crate (see `crates/mty-codegen-cranelift/tests/`).

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package_typed;

fn diag_codes(src: &str) -> (Vec<String>, mty_types::TypedPackage) {
    let parsed = parse_source(src.into(), "ffi_v038_t3.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    let any_lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    let typed = if !any_lower_err {
        let typed = check_package_typed(&pkg);
        diags.extend(typed.diagnostics.clone());
        typed
    } else {
        mty_types::TypedPackage::default()
    };
    let codes = diags
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str().to_string())
        .collect();
    (codes, typed)
}

fn assert_clean(src: &str) -> mty_types::TypedPackage {
    let (codes, typed) = diag_codes(src);
    assert!(
        codes.is_empty(),
        "expected no typeck errors, got {:?}",
        codes
    );
    typed
}

// ---------- Surface 1: returned-struct binding (row 7) ----------

#[test]
fn return_struct_bind_to_let() {
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn make_point(x: I32, y: I32) -> Point
}

fn main() {
  let p: Point = make_point(3, 4)
  let _ = p.x
  let _ = p.y
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn return_struct_inferred_let() {
    // Same as above but the `let` has no type annotation — should
    // infer Point from the call's return type.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn make_point(x: I32, y: I32) -> Point
}

fn main() {
  let p = make_point(1, 2)
  let _ = p.x
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn return_struct_field_access_chain() {
    let src = r#"
struct Pair { a: I32, b: I32 }

extern c {
  fn make_pair() -> Pair
}

fn main() {
  let result = make_pair().a
  let _ = result
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn return_large_struct_sret_path() {
    // Five I32s = 20 bytes, exceeds the 16-byte register-return
    // threshold; the cranelift backend will use the sret out-pointer.
    // Typeck doesn't care about the size — it's only IR-side.
    let src = r#"
struct Big { a: I32, b: I32, c: I32, d: I32, e: I32 }

extern c {
  fn make_big() -> Big
}

fn main() {
  let b: Big = make_big()
  let _ = b.a
  let _ = b.e
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn return_struct_to_fn_arg() {
    // The result of an FFI-returned struct flows directly into another
    // fn call. Pin the chain-through-temp shape.
    let src = r#"
struct Color { r: I32, g: I32, b: I32 }

extern c {
  fn make_color() -> Color
  fn consume_color(c: Color) -> I32
}

fn main() {
  let _ = consume_color(make_color())
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn return_struct_mismatch_still_diagnoses() {
    // Sanity: if you bind the call result to a wrong-typed let, you
    // still get an MT2001 mismatch — the coercion path for returned
    // structs does NOT silently swallow the error.
    let src = r#"
struct Point { x: I32, y: I32 }
struct Quad { a: I32, b: I32, c: I32, d: I32 }

extern c {
  fn make_point() -> Point
}

fn main() {
  let _q: Quad = make_point()
}
"#;
    let (codes, _) = diag_codes(src);
    assert!(
        codes.iter().any(|c| c == "MT2001"),
        "expected MT2001 mismatch, got {:?}",
        codes
    );
}

// ---------- Surface 2: function pointer (row 11) ----------

#[test]
fn fn_pointer_extern_decl_parses() {
    // Just parse the extern fn signature with a fn-typed param.
    let src = r#"
extern c {
  fn qsort_stub(cmp: fn(I32, I32) -> I32) -> Unit
}

fn main() {
  log("decl-only")
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn fn_pointer_pass_named_fn() {
    // Pass a named Mighty fn as the cb arg of an extern-c fn that
    // expects `fn(I32) -> I32`.
    let src = r#"
extern c {
  fn ffi_with_cb(cb: fn(I32) -> I32) -> I32
}

fn my_cmp(a: I32) -> I32 {
  a + 1
}

fn main() {
  let _ = ffi_with_cb(my_cmp)
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn fn_pointer_two_arg_cb() {
    // C-typical `int (*cmp)(const void*, const void*)` shape — we use
    // `*U8` pointers and an i32 return to mirror qsort. Caller passes
    // a Mighty Str (Str→*U8 coercion) for the buffer to avoid the
    // raw_ptr-prelude question and pin the fn-pointer surface alone.
    let src = r#"
extern c {
  fn ffi_sort(buf: *U8, n: USize, sz: USize, cmp: fn(*U8, *U8) -> I32) -> Unit
}

fn my_cmp(a: *U8, b: *U8) -> I32 {
  0
}

fn main() {
  let n: USize = 4
  let sz: USize = 4
  ffi_sort("buf", n, sz, my_cmp)
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn fn_pointer_arity_mismatch_diagnoses() {
    // Passing a fn with the wrong arity should hit MT2001 mismatch —
    // the unify path catches Fn[1] vs Fn[2] cleanly.
    let src = r#"
extern c {
  fn ffi_with_cb(cb: fn(I32) -> I32) -> I32
}

fn my_two_arg_cmp(a: I32, b: I32) -> I32 {
  0
}

fn main() {
  let _ = ffi_with_cb(my_two_arg_cmp)
}
"#;
    let (codes, _) = diag_codes(src);
    assert!(
        codes.iter().any(|c| c == "MT2001"),
        "expected MT2001 arity mismatch, got {:?}",
        codes
    );
}

#[test]
fn fn_pointer_return_type_mismatch_diagnoses() {
    let src = r#"
extern c {
  fn ffi_with_cb(cb: fn(I32) -> I32) -> I32
}

fn my_cmp(a: I32) -> Bool {
  true
}

fn main() {
  let _ = ffi_with_cb(my_cmp)
}
"#;
    let (codes, _) = diag_codes(src);
    assert!(
        codes.iter().any(|c| c == "MT2001"),
        "expected MT2001 return-type mismatch, got {:?}",
        codes
    );
}

// ---------- Surface 3: #[ffi_nul_ok] attribute ----------

#[test]
fn ffi_nul_ok_attr_parses() {
    let src = r#"
extern c {
  fn strlen(#[ffi_nul_ok] s: *U8) -> USize
}

fn main() {
  let _ = strlen("hello")
}
"#;
    let typed = assert_clean(src);
    // The Str→*U8 coercion should still fire so the arg flows through.
    assert!(
        !typed.coerce_str_to_ptr.is_empty(),
        "expected coerce_str_to_ptr entry, got none"
    );
    // The new ffi_nul_ok side-table should also light up for this arg.
    assert!(
        !typed.coerce_nul_ok.is_empty(),
        "expected coerce_nul_ok entry, got none"
    );
}

#[test]
fn ffi_nul_ok_only_on_marked_params() {
    // Two extern fns: one marked `#[ffi_nul_ok]`, one not. Only the
    // marked one should land in coerce_nul_ok.
    let src = r#"
extern c {
  fn safe_strlen(s: *U8) -> USize
  fn fast_strlen(#[ffi_nul_ok] s: *U8) -> USize
}

fn main() {
  let _ = safe_strlen("a")
  let _ = fast_strlen("b")
}
"#;
    let typed = assert_clean(src);
    // Both call sites coerced Str→*U8.
    assert_eq!(typed.coerce_str_to_ptr.len(), 2);
    // Only the fast_strlen call is in nul_ok.
    assert_eq!(typed.coerce_nul_ok.len(), 1);
}

#[test]
fn ffi_nul_ok_attr_on_one_of_multi_params() {
    // Mixed: only the `s` param has the attribute. The `n` param is
    // a plain USize, not eligible.
    let src = r#"
extern c {
  fn write_n(#[ffi_nul_ok] s: *U8, n: USize) -> I32
}

fn main() {
  let n: USize = 5
  let _ = write_n("hi", n)
}
"#;
    let typed = assert_clean(src);
    assert_eq!(typed.coerce_str_to_ptr.len(), 1);
    assert_eq!(typed.coerce_nul_ok.len(), 1);
}

#[test]
fn ffi_nul_ok_no_str_arg_no_entry() {
    // Marked param but the caller passes a non-Str via `&` borrow
    // (an `&mut u8` is the addr-of path, not the Str-coercion path).
    // No Str→*U8 coercion fires, so no nul_ok entry either: the
    // optimization is a property of the coercion, not the param.
    let src = r#"
extern c {
  fn write_buf(#[ffi_nul_ok] s: *U8) -> I32
}

fn main() {
  let mut b: U8 = 0u8
  let _ = write_buf(&mut b)
}
"#;
    let typed = assert_clean(src);
    // No Str→*U8 coercion (the arg was an address-of, not a Str).
    assert!(typed.coerce_str_to_ptr.is_empty());
    // And therefore no nul_ok entry either.
    assert!(typed.coerce_nul_ok.is_empty());
}

#[test]
fn ffi_nul_ok_attr_only_at_extern_sites() {
    // The attribute is parsed and accepted on `extern c` params; for
    // non-extern callees the coercion table stays empty regardless.
    let src = r#"
fn take(s: *U8) -> USize { 0usize }

fn main() {
  let _ = take("hello")
}
"#;
    let (_codes, typed) = diag_codes(src);
    assert!(typed.coerce_nul_ok.is_empty());
}
