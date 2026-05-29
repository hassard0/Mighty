//! v0.37 Track T3 — FFI call-site coercions.
//!
//! Pins three coercions for `extern c { fn ... }` callees that v0.36
//! T2 left as wrapper-pattern follow-ups. The tests share a single
//! permissive-extern call shape so the coercion logic is the only
//! variable.

use mty_diagnostics::Severity;
use mty_driver::{lower, parse_source};
use mty_types::check_package_typed;

fn diag_codes(src: &str) -> (Vec<String>, mty_types::TypedPackage, mty_hir::Package) {
    let parsed = parse_source(src.into(), "ffi_coercions_v037.mty".into());
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
        .map(|d| d.code.as_str())
        .collect();
    (codes, typed, pkg)
}

fn assert_clean(src: &str) -> (mty_types::TypedPackage, mty_hir::Package) {
    let (codes, typed, pkg) = diag_codes(src);
    assert!(
        codes.is_empty(),
        "expected no typeck errors, got {:?}",
        codes
    );
    (typed, pkg)
}

// ---------- Track 1: Str → *U8 coercion ----------

#[test]
fn coerce_str_literal_to_ptr_u8() {
    // Str literal passed where extern c expects *U8 should type-check.
    let src = r#"
extern c {
  fn ffi_take_ptr(p: *U8) -> I32
}

fn main() {
  let _ = ffi_take_ptr("hello")
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(
        !typed.coerce_str_to_ptr.is_empty(),
        "expected a coerce_str_to_ptr entry, found none"
    );
}

#[test]
fn coerce_str_variable_to_ptr_u8() {
    // Str-typed local feeding extern c *U8 should type-check.
    let src = r#"
extern c {
  fn ffi_take_ptr(p: *U8) -> I32
}

fn main() {
  let s = "hello"
  let _ = ffi_take_ptr(s)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_str_to_ptr.is_empty());
}

#[test]
fn coerce_str_with_len_param() {
    // Real-world shape: ptr + len pair, the v0.36 demo 11 winit shim.
    let src = r#"
extern c {
  fn ffi_open(p: *U8, len: USize) -> I32
}

fn main() {
  let n: USize = 7
  let _ = ffi_open("mty-ide", n)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_str_to_ptr.is_empty());
}

#[test]
fn no_coercion_for_non_extern_call() {
    // A regular Mighty fn taking *U8 must NOT silently accept Str —
    // the coercion is gated on extern-c callees.
    let src = r#"
fn take(p: *U8) -> I32 { 0 }

fn main() {
  let _ = take("hello")
}
"#;
    let (_codes, typed, _) = diag_codes(src);
    // The regular fn arg path falls through to unify; whether it
    // produces a diagnostic or not is an orthogonal question, but
    // the coercion table must stay empty.
    assert!(
        typed.coerce_str_to_ptr.is_empty(),
        "non-extern callees must not enter coerce_str_to_ptr"
    );
}

#[test]
fn coerce_str_in_multi_arg_call() {
    // Mixed: extern fn taking I32, *U8, I32. Only the Str arg should
    // land in the coercion table.
    let src = r#"
extern c {
  fn ffi_mixed(w: I32, title: *U8, h: I32) -> I32
}

fn main() {
  let _ = ffi_mixed(640, "title", 480)
}
"#;
    let (typed, _) = assert_clean(src);
    assert_eq!(typed.coerce_str_to_ptr.len(), 1);
}

#[test]
fn no_coercion_for_non_str_at_ptr_slot() {
    // Coercion only fires for Str→*U8. Passing an I32 at *U8 must
    // still go through the regular check (which would diag, but the
    // coercion table stays clean).
    let src = r#"
extern c {
  fn ffi_take_ptr(p: *U8) -> I32
}

fn main() {
  let _ = ffi_take_ptr(42)
}
"#;
    let (_codes, typed, _) = diag_codes(src);
    assert!(
        typed.coerce_str_to_ptr.is_empty(),
        "I32 at *U8 must not land in coerce_str_to_ptr"
    );
}

#[test]
fn coerce_str_pass_through_let() {
    // `let s = "x"; ffi(s)` — propagate Str type through the let and
    // still detect coercion at the call site.
    let src = r#"
extern c {
  fn ffi_take_ptr(p: *U8) -> I32
}

fn main() {
  let s: Str = "hello"
  let _ = ffi_take_ptr(s)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_str_to_ptr.is_empty());
}

// ---------- Track 2: address-of for FFI locals ----------

#[test]
fn coerce_addr_of_mut_local() {
    // `&mut x` at an extern-c `*mut I32` slot should typecheck.
    let src = r#"
extern c {
  fn ffi_write_out(p: *I32) -> Unit
}

fn main() {
  let mut x: I32 = 0
  ffi_write_out(&mut x)
  log("x=...")
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(
        !typed.coerce_addr_of.is_empty(),
        "expected an address-of entry, found none"
    );
}

#[test]
fn coerce_addr_of_shared_local() {
    // Shared borrow at extern-c `*T`.
    let src = r#"
extern c {
  fn ffi_read_in(p: *I32) -> I32
}

fn main() {
  let x: I32 = 42
  let _ = ffi_read_in(&x)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_addr_of.is_empty());
}

#[test]
fn coerce_addr_of_struct_local() {
    // `&local_struct` for an extern-c `*MyStruct` arg.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn ffi_point_read(p: *Point) -> I32
}

fn main() {
  let pt: Point = Point { x: 1, y: 2 }
  let _ = ffi_point_read(&pt)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_addr_of.is_empty());
}

#[test]
fn coerce_addr_of_only_at_extern_sites() {
    // A regular Mighty fn taking `*I32` should NOT trigger the
    // address-of coercion path — borrow ref `&x` of type `&I32` is
    // not `*I32`, so the regular unify should diag, but the address-of
    // table must stay empty.
    let src = r#"
fn take(p: *I32) -> Unit {}

fn main() {
  let x: I32 = 1
  take(&x)
}
"#;
    let (_codes, typed, _) = diag_codes(src);
    assert!(
        typed.coerce_addr_of.is_empty(),
        "non-extern callees must not enter coerce_addr_of"
    );
}

#[test]
fn coerce_addr_of_multiple_args() {
    // Two out-params and a regular arg interleaved.
    let src = r#"
extern c {
  fn ffi_two_outs(a: *I32, n: I32, b: *I32) -> Unit
}

fn main() {
  let mut x: I32 = 0
  let mut y: I32 = 0
  ffi_two_outs(&mut x, 7, &mut y)
}
"#;
    let (typed, _) = assert_clean(src);
    assert_eq!(typed.coerce_addr_of.len(), 2);
}

#[test]
fn coerce_addr_of_u8_buffer() {
    // `&mut buf` where `buf: U8` — the address-of path accepts any
    // pointee type, so this should land in the address-of table.
    let src = r#"
extern c {
  fn ffi_writeu8(p: *U8) -> Unit
}

fn main() {
  let mut b: U8 = 0u8
  ffi_writeu8(&mut b)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_addr_of.is_empty());
    // And the Str-coercion table stays untouched: the &mut U8 path
    // is address-of, NOT Str-to-ptr.
    assert!(typed.coerce_str_to_ptr.is_empty());
}

// ---------- Track 3: struct literal at FFI call site ----------

#[test]
fn struct_literal_as_extern_arg_parses_and_typechecks() {
    // Inline struct literal at extern-c call site. Verifies the
    // parser already accepts struct-literal-as-arg AND the type
    // checker doesn't reject it (rows 05/06 of the matrix).
    let src = r#"
struct Rect { x: I32, y: I32, w: I32, h: I32 }

extern c {
  fn ffi_draw_rect(r: Rect) -> Unit
}

fn main() {
  ffi_draw_rect(Rect { x: 0, y: 0, w: 100, h: 50 })
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn struct_literal_nested_in_call() {
    // Slightly more nested form — inside a let binding feeding the
    // call. The locked-in shape.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn ffi_take_point(p: Point) -> I32
}

fn main() {
  let _ = ffi_take_point(Point { x: 10, y: 20 })
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn struct_literal_multi_arg_extern() {
    // Two struct literals + a primitive. Pins the v0.36-shadowed
    // shape from `wgpu_create_pipeline(VertState{...}, FragState{...}, 0u32)`.
    let src = r#"
struct Vert { off: I32 }
struct Frag { id: I32 }

extern c {
  fn ffi_pipeline(v: Vert, f: Frag, flags: U32) -> I32
}

fn main() {
  let _ = ffi_pipeline(Vert { off: 0 }, Frag { id: 1 }, 0u32)
}
"#;
    let _ = assert_clean(src);
}

#[test]
fn struct_literal_then_address_of_struct() {
    // Combined ergonomics: `&local` of a struct passed to an extern-c
    // `*Point`. (Inline struct → address-of compound is a v0.38
    // follow-up; here we just check that the local-binding form
    // doesn't get broken by the new coercion path.)
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn ffi_point_ptr(p: *Point) -> I32
}

fn main() {
  let pt = Point { x: 7, y: 9 }
  let _ = ffi_point_ptr(&pt)
}
"#;
    let (typed, _) = assert_clean(src);
    assert!(!typed.coerce_addr_of.is_empty());
}

#[test]
fn struct_literal_at_non_extern_site_still_works() {
    // Sanity: the parser change (struct-literal-as-arg) is not new —
    // verify it works for plain Mighty fns too. Pre-existing behavior;
    // this test just locks it in alongside the extern-site tests.
    let src = r#"
struct Q { v: I32 }

fn take(q: Q) -> I32 { q.v }

fn main() {
  let _ = take(Q { v: 11 })
}
"#;
    let _ = assert_clean(src);
}
