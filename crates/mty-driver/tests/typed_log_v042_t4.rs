//! v0.42 T4 (L23 fix) — end-to-end: build a real native object that
//! traces computed values via `log(n)`, `log(n.to_str())`, and
//! `log("count=" + n.to_str())`.
//!
//! Pre-fix the cranelift backend's `log` lowering only accepted Str
//! operands, so any of these programs would have surfaced
//! `BuildOutcome::BackendError` (or, worse, mis-lowered to the
//! string path and produced garbage). v0.42 T4 wires:
//!
//!   - typed `mty_runtime_log_i32/_i64/_u32/_u64/_usize/_f32/_f64/_bool`
//!     so `log(n)` for any scalar lowers cleanly,
//!   - `to_str()` on scalar receivers (via `mty_runtime_fmt_*`),
//!   - `String + String` concat (via `mty_runtime_str_concat`).
//!
//! Mirror of `native_dynamic_log.rs`: we don't try to drive the host
//! linker (the CI matrix is too varied); the assertion is "codegen
//! produced an object without raising Unsupported".

use mty_codegen_cranelift::artifact::BuildMode;
use mty_driver::{build_native, BuildOptions, BuildOutcome, BuildTarget};

fn opts(dir: &tempfile::TempDir, name: &str) -> BuildOptions {
    BuildOptions {
        target: BuildTarget::Native,
        mode: BuildMode::Debug,
        out_dir: dir.path().to_path_buf(),
        binary_name: name.into(),
        no_component: false,
        wasi_preview: Default::default(),
        user_wit: None,
        extern_libs: Vec::new(),
        manifest_dir: None,
        build_config: None,
    }
}

fn must_native_object(name: &str, src: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = build_native(src.to_string(), format!("{name}.mty"), &opts(&dir, name));
    match outcome {
        BuildOutcome::NativeOk(p) | BuildOutcome::NativeOkNoLinker { object_path: p, .. } => {
            assert!(p.exists(), "expected built artifact at {}", p.display());
            let bytes = std::fs::read(&p).expect("read artifact");
            assert!(!bytes.is_empty(), "artifact is empty");
        }
        BuildOutcome::NativeLinkError { object_path, .. } => {
            assert!(
                object_path.exists(),
                "expected object artifact at {}",
                object_path.display()
            );
        }
        BuildOutcome::BackendError(e) => panic!(
            "v0.42 T4 regression: typed `log` shouldn't raise Unsupported — got {e}\nsource:\n{src}"
        ),
        BuildOutcome::FrontendError => panic!("frontend rejected v0.42 T4 source:\n{src}"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome variant for native build"),
    }
}

#[test]
fn log_i32_computed_value_compiles_native() {
    let src = r#"
        fn double(x: I32) -> I32 { x + x }
        fn main() {
          log(double(21))
        }
    "#;
    must_native_object("log_i32_computed", src);
}

#[test]
fn log_multi_arg_str_and_int_compiles_native() {
    // The motivating L23 case: `log("count=", n)` for a computed n.
    let src = r#"
        fn main() {
          let n: I32 = 42
          log("count=", n)
        }
    "#;
    must_native_object("log_multi_arg", src);
}

#[test]
fn log_concat_str_plus_to_str_compiles_native() {
    // The other realistic shape: `log("count=" + n.to_str())`.
    let src = r#"
        fn main() {
          let n: I32 = 42
          log("count=" + n.to_str())
        }
    "#;
    must_native_object("log_concat", src);
}

#[test]
fn log_float_and_bool_computed_compiles_native() {
    let src = r#"
        fn main() {
          let x: F64 = 3.5_f64
          log(x)
          let b = true
          log(b)
        }
    "#;
    must_native_object("log_f_bool", src);
}

#[test]
fn scalar_to_str_methods_compile_native() {
    let src = r#"
        fn main() {
          let n: I32 = 7
          let s: Str = n.to_str()
          log(s)
          let b = false
          log(b.to_string())
        }
    "#;
    must_native_object("to_str_native", src);
}
