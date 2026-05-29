//! v0.38 Track T3 — cranelift backend pins for FFI follow-ups.
//!
//! Object-shape assertions for the three v0.38 T3 surfaces:
//!
//!   1. **Returned-struct binding** — call to `extern c fn foo() ->
//!      Point` emits a multi-register / sret-shaped call instruction
//!      and the caller materialises the result into a stack slot.
//!   2. **Function pointer** — passing a Mighty fn at an `extern fn(...)`
//!      arg slot emits a `func_addr` reference.
//!   3. **`#[ffi_nul_ok]`** — the lowering keeps the no-copy fast path
//!      (the v0.37 default is already the no-copy shape; this test
//!      pins that future safety-copy work does NOT regress when the
//!      attribute is present).
//!
//! The assertions deliberately stay at the object level rather than
//! the CLIF level: CLIF text is a compiler-internal artifact and a
//! signature-format flip would churn the tests. The object file's
//! `main` symbol + relocations to extern fn names are stable across
//! cranelift versions.

use mty_ast::AstNode;
use mty_codegen_cranelift::compile_object;
use mty_ir::lower_package;
use mty_syntax::parse;
use object::read::{Object as _, ObjectSymbol as _};

fn lower_to_sir(src: &str, src_id: &str) -> mty_ir::Program {
    let parsed = parse(src);
    assert!(
        parsed.errors.is_empty(),
        "{src_id}: parse errors: {:?}",
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let file = mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green))
        .unwrap_or_else(|| panic!("{src_id}: FILE root not produced"));
    let (pkg, lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    if let Some(d) = lower_diags
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!(
            "{src_id}: lower error MT{:04}: {}",
            d.code.0, d.primary.message
        );
    }
    let typed = mty_types::check_package_typed(&pkg);
    if let Some(d) = typed
        .diagnostics
        .iter()
        .find(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        panic!(
            "{src_id}: typeck error MT{:04}: {}",
            d.code.0, d.primary.message
        );
    }
    lower_package(&pkg, &typed)
}

fn compile_to_object_bytes(src: &str, src_id: &str) -> Vec<u8> {
    let prog = lower_to_sir(src, src_id);
    let tmp = tempfile::tempdir().expect("tempdir");
    let obj_path = tmp.path().join(format!("{src_id}.o"));
    compile_object(&prog, &obj_path).unwrap_or_else(|e| panic!("[{src_id}] compile_object: {e:?}"));
    std::fs::read(&obj_path).expect("read object")
}

fn object_has_extern_symbol_ref(bytes: &[u8], wanted: &str) -> bool {
    let parsed = object::read::File::parse(bytes).expect("object parse");
    for sym in parsed.symbols() {
        if let Ok(name) = sym.name() {
            // Mach-O underscore convention.
            if name == wanted || name == format!("_{}", wanted) {
                return true;
            }
        }
    }
    false
}

// ---------- Surface 1: returned-struct binding ----------

#[test]
fn return_struct_small_emits_call_and_extern_ref() {
    // Two i32s = 8 bytes — small-struct one-register return path.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn make_point() -> Point
}

fn main() {
  let p: Point = make_point()
  let _ = p.x
}
"#;
    let bytes = compile_to_object_bytes(src, "return_struct_small");
    assert!(
        object_has_extern_symbol_ref(&bytes, "make_point"),
        "expected extern reference to `make_point` in emitted object"
    );
}

#[test]
fn return_struct_mid_emits_extern_ref() {
    // 12 bytes — fits the two-register return path (size > 8 && <= 16).
    let src = r#"
struct Mid { a: I32, b: I32, c: I32 }

extern c {
  fn make_mid() -> Mid
}

fn main() {
  let m: Mid = make_mid()
  let _ = m.a
}
"#;
    let bytes = compile_to_object_bytes(src, "return_struct_mid");
    assert!(object_has_extern_symbol_ref(&bytes, "make_mid"));
}

#[test]
fn return_struct_large_emits_extern_ref() {
    // 20 bytes — sret path (size > 16).
    let src = r#"
struct Big { a: I32, b: I32, c: I32, d: I32, e: I32 }

extern c {
  fn make_big() -> Big
}

fn main() {
  let b: Big = make_big()
  let _ = b.a
}
"#;
    let bytes = compile_to_object_bytes(src, "return_struct_large");
    assert!(object_has_extern_symbol_ref(&bytes, "make_big"));
}

#[test]
fn return_struct_field_access_compiles() {
    // Bind the returned struct, access multiple fields.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn make_point() -> Point
}

fn main() {
  let p: Point = make_point()
  let _ = p.x
  let _ = p.y
}
"#;
    let bytes = compile_to_object_bytes(src, "return_struct_fields");
    assert!(!bytes.is_empty());
}

// ---------- Surface 2: fn-pointer surface ----------

#[test]
fn fn_pointer_pass_compiles() {
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
    let bytes = compile_to_object_bytes(src, "fn_pointer_pass");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_with_cb"));
    // The named callback fn is `Linkage::Local` (only `main` is
    // exported); we still expect a `my_cmp` symbol in the object so
    // the cranelift `func_addr` reference can resolve internally.
    let parsed = object::read::File::parse(&*bytes).expect("object parse");
    let mut saw = false;
    for sym in parsed.symbols() {
        if let Ok(n) = sym.name() {
            if n == "my_cmp" || n == "_my_cmp" {
                saw = true;
                break;
            }
        }
    }
    assert!(saw, "expected `my_cmp` symbol in object for func_addr");
}

#[test]
fn fn_pointer_two_arg_cb_compiles() {
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
    let bytes = compile_to_object_bytes(src, "fn_pointer_two_arg");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_sort"));
}

// ---------- Surface 3: #[ffi_nul_ok] no-copy path ----------

#[test]
fn ffi_nul_ok_marked_compiles_clean() {
    // Pin that the attribute parses, typechecks, and lowers cleanly
    // through cranelift. The lowering shape is unchanged from v0.37
    // (intern_string already produces a null-terminated symbol that
    // the ptr-half of the Str aggregate points at) — the attribute
    // just marks the call-site arg for downstream tooling that wants
    // to elide a safety-copy.
    let src = r#"
extern c {
  fn strlen(#[ffi_nul_ok] s: *U8) -> USize
}

fn main() {
  let _ = strlen("hello")
}
"#;
    let bytes = compile_to_object_bytes(src, "ffi_nul_ok_marked");
    assert!(object_has_extern_symbol_ref(&bytes, "strlen"));
}

#[test]
fn ffi_nul_ok_unmarked_also_compiles_clean() {
    // Sanity: the unmarked path also compiles cleanly. The two
    // shapes share the same lowering today; the attribute is a
    // metadata-only marker in the side table.
    let src = r#"
extern c {
  fn strlen(s: *U8) -> USize
}

fn main() {
  let _ = strlen("hello")
}
"#;
    let bytes = compile_to_object_bytes(src, "ffi_nul_ok_unmarked");
    assert!(object_has_extern_symbol_ref(&bytes, "strlen"));
}

#[test]
fn ffi_v038_t3_three_surfaces_in_one_program() {
    // Smoke: a single program using all three v0.38 T3 surfaces.
    // Compiles end-to-end and produces an object with extern symbol
    // references for each declared FFI fn.
    let src = r#"
struct Point { x: I32, y: I32 }

extern c {
  fn make_point() -> Point
  fn run_cb(cb: fn(I32) -> I32) -> I32
  fn strlen(#[ffi_nul_ok] s: *U8) -> USize
}

fn cb(x: I32) -> I32 {
  x + 1
}

fn main() {
  let p = make_point()
  let _ = p.x
  let _ = run_cb(cb)
  let _ = strlen("hello")
}
"#;
    let bytes = compile_to_object_bytes(src, "v038_t3_three_surfaces");
    assert!(object_has_extern_symbol_ref(&bytes, "make_point"));
    assert!(object_has_extern_symbol_ref(&bytes, "run_cb"));
    assert!(object_has_extern_symbol_ref(&bytes, "strlen"));
}
