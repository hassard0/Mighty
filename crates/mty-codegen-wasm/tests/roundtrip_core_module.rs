//! `--no-component` (BuildOptions::core_only) emits a plain core
//! module that still validates under wasmparser. This is the
//! backwards-compat path slice 8 expected.

mod common;

use mty_codegen_wasm::{
    artifact::WasmFormat, compile_program_to_file, compile_program_to_file_with_options,
    is_component, BuildOptions, WasmTarget,
};

#[test]
fn core_only_emits_core_module() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("hello.wasm");
    let prog = common::empty_main();
    let opts = BuildOptions::core_only("hello");
    let art =
        compile_program_to_file_with_options(&prog, WasmTarget::Wasi, &out, &opts).expect("write");
    assert_eq!(art.format, WasmFormat::CoreModule);
    let bytes = std::fs::read(&out).expect("read");
    assert!(!is_component(&bytes), "should NOT be a component");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("core module validates");
}

#[test]
fn legacy_compile_program_to_file_still_works() {
    // Slice-8 callers used compile_program_to_file directly. We
    // promise that surface still emits a core module.
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("legacy.wasm");
    let prog = common::empty_main();
    let art = compile_program_to_file(&prog, WasmTarget::Wasi, &out).expect("write");
    assert_eq!(art.format, WasmFormat::CoreModule);
    assert!(out.exists());
    let bytes = std::fs::read(&out).expect("read");
    assert!(!is_component(&bytes));
}

#[test]
fn core_only_still_emits_wit_alongside_artifact() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("hello.wasm");
    let prog = common::empty_main();
    let opts = BuildOptions::core_only("hello");
    let art =
        compile_program_to_file_with_options(&prog, WasmTarget::Wasi, &out, &opts).expect("write");
    // The wit text is attached even in core-only mode so downstream
    // tooling can still consume it.
    assert!(art.wit_text.is_some());
    let wit = art.wit_text.unwrap();
    assert!(wit.contains("package stardust:hello"));
}
