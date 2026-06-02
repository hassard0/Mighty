//! v0.46 Track T3 — cranelift backend pins for the (ptr, len) FFI
//! lowering of Mighty `Str` / `String` at extern-c param slots.
//!
//! L52 fix. The codegen now emits TWO ABI args at the call site for
//! every Str / String param of an `extern c` fn: the ptr-half and the
//! byte-len half. This file pins object-level shapes only — the full
//! link + run is exercised by the matrix integration test
//! `crates/mty-driver/tests/extern_c_matrix.rs::row_12_*`.
//!
//! The assertions stay at the object level (extern symbol references,
//! emitted main) rather than the CLIF level because CLIF text is a
//! cranelift-internal artifact that drifts across upgrades. Object
//! symbols are stable.

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
            if name == wanted || name == format!("_{}", wanted) {
                return true;
            }
        }
    }
    false
}

// ---------- Str at extern-c param slot ----------

#[test]
fn str_slice_single_arg_compiles() {
    // Simplest shape: one Str param, no return. Pins that the
    // signature builder + call-site lowering both accept Str and emit
    // the (ptr, len) expansion without a verifier failure.
    let src = r#"
extern c {
  fn ffi_take_str(s: Str) -> Unit
}

fn main() {
  ffi_take_str("hello")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_single_arg");
    assert!(
        object_has_extern_symbol_ref(&bytes, "ffi_take_str"),
        "expected extern reference to `ffi_take_str` in emitted object"
    );
}

#[test]
fn str_slice_with_scalar_prefix_compiles() {
    // (handle: I64, s: Str) — the IDE's prompted-command shape. Pins
    // that the (ptr, len) expansion slots in alongside a scalar arg
    // without shifting the ABI ordering.
    let src = r#"
extern c {
  fn mui_file_rename(handle: I64, path: Str) -> Unit
}

fn main() {
  let h: I64 = 1
  mui_file_rename(h, "newname.txt")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_scalar_prefix");
    assert!(object_has_extern_symbol_ref(&bytes, "mui_file_rename"));
}

#[test]
fn str_slice_returns_i32() {
    // Str arg + scalar return — pins that the return-value plumbing
    // still works when the param ABI shape doubled.
    let src = r#"
extern c {
  fn ffi_count(s: Str) -> I32
}

fn main() {
  let n = ffi_count("hello-world")
  let _ = n
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_returns_i32");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_count"));
}

#[test]
fn str_slice_two_str_args_compiles() {
    // Two Str slots — each independently expanded to (ptr, len). Pins
    // that the slot-doubling isn't a once-only special case.
    let src = r#"
extern c {
  fn ffi_two(a: Str, b: Str) -> I32
}

fn main() {
  let _ = ffi_two("first", "second")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_two_args");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_two"));
}

#[test]
fn str_slice_empty_literal_compiles() {
    // Empty literal — pins that the lowering produces a valid (ptr,
    // len=0) pair even when the Str literal has zero bytes.
    let src = r#"
extern c {
  fn ffi_take(s: Str) -> Unit
}

fn main() {
  ffi_take("")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_empty");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_take"));
}

#[test]
fn str_slice_interleaved_with_scalar_compiles() {
    // (Str, I32, Str) — Str slots not contiguous. Pins that the
    // signature builder walks params in order and expands only the
    // Str slots, not the I32 between them.
    let src = r#"
extern c {
  fn ffi_mixed(a: Str, count: I32, b: Str) -> Unit
}

fn main() {
  ffi_mixed("first", 3, "second")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_interleaved");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_mixed"));
}

#[test]
fn str_slice_with_pointer_and_str_compiles() {
    // (Str, *U8) — pins that the existing *U8 coercion (Str -> *U8
    // for null-terminated C strings) and the new Str-slice expansion
    // can co-exist on the same call.
    let src = r#"
extern c {
  fn ffi_blend(slice: Str, cstr: *U8) -> I32
}

fn main() {
  let _ = ffi_blend("payload", "cstring")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_blend");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_blend"));
}

#[test]
fn str_slice_three_strs_compiles() {
    // L52's worst-case dispatcher shape — three Str args in a row,
    // all expanded.
    let src = r#"
extern c {
  fn ffi_three(a: Str, b: Str, c: Str) -> I32
}

fn main() {
  let _ = ffi_three("one", "two", "three")
}
"#;
    let bytes = compile_to_object_bytes(src, "str_slice_three");
    assert!(object_has_extern_symbol_ref(&bytes, "ffi_three"));
}
