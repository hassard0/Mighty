//! v0.36 T1 — end-to-end: build a real native object that uses dynamic
//! `log()` and pin that the build succeeds.
//!
//! We don't try to drive the host linker (CI matrix is too varied) —
//! we settle for "the cranelift backend produced an object file
//! without raising `Unsupported`", which is precisely the failure mode
//! the dynamic-log fix targets. Pre-fix, the program below would have
//! caused `string_pair` to return
//! `CodegenError::Unsupported("non-literal string in log/print")`,
//! and `build_native` would have surfaced
//! `BuildOutcome::BackendError` instead of writing an object.

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

#[test]
fn native_build_with_dynamic_log_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = r#"
        fn greet() -> Str { "v0.36 T1" }
        fn main() {
          let g = greet()
          log(g)
          log("static-too")
        }
    "#;
    let outcome = build_native(
        src.to_string(),
        "dyn_log.mty".into(),
        &opts(&dir, "dyn_log"),
    );
    match outcome {
        BuildOutcome::NativeOk(p) | BuildOutcome::NativeOkNoLinker(p) => {
            assert!(p.exists(), "expected built artifact at {}", p.display());
            let bytes = std::fs::read(&p).expect("read artifact");
            assert!(!bytes.is_empty(), "artifact is empty");
        }
        BuildOutcome::BackendError(e) => panic!(
            "v0.36 T1 regression: dynamic log shouldn't raise Unsupported any more — got {e}"
        ),
        BuildOutcome::FrontendError => panic!("frontend rejected dynamic-log source"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome variant for native build"),
    }
}

#[test]
fn native_build_with_u8_widening_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Mighty's typeck doesn't auto-widen U8 to I64 in binops; this
    // test just verifies the codegen doesn't choke on programs that
    // pass U8s through fn calls + struct fields (the codegen paths
    // that were emitting `sextend` pre-fix).
    let src = r#"
        struct Pixel { r: U8 }
        fn pick(p: Pixel) -> U8 { p.r }
        fn main() {
          let p = Pixel { r: 0xFF_u8 }
          let _x: U8 = pick(p)
          log("widened")
        }
    "#;
    let outcome = build_native(
        src.to_string(),
        "widen.mty".into(),
        &opts(&dir, "widen_main"),
    );
    match outcome {
        BuildOutcome::NativeOk(p) | BuildOutcome::NativeOkNoLinker(p) => {
            assert!(p.exists());
        }
        BuildOutcome::BackendError(e) => panic!("U8 widening backend error: {e}"),
        BuildOutcome::FrontendError => panic!("frontend rejected U8 widening source"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome variant for native build"),
    }
}

#[test]
fn native_build_with_hex_suffix_literals() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = r#"
        fn main() {
          let _a: U8 = 0xFF_u8
          let _b: U16 = 0xDEAD_u16
          let _c: U32 = 0xDEAD_BEEF_u32
          let _d: U64 = 0xCAFE_BABE_DEAD_BEEF_u64
          log("hex-with-suffix")
        }
    "#;
    let outcome = build_native(
        src.to_string(),
        "hex.mty".into(),
        &opts(&dir, "hex_literals"),
    );
    match outcome {
        BuildOutcome::NativeOk(p) | BuildOutcome::NativeOkNoLinker(p) => {
            assert!(p.exists());
        }
        BuildOutcome::BackendError(e) => panic!("hex-suffix backend error: {e}"),
        BuildOutcome::FrontendError => panic!("frontend rejected hex literal source"),
        BuildOutcome::WasmOk(_) => panic!("wrong outcome variant for native build"),
    }
}
