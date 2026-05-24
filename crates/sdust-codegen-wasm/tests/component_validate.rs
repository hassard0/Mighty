//! Component Model validation: wrap a core module and assert
//! `wasmparser` accepts the resulting bytes as a valid component.

mod common;

use sdust_codegen_wasm::{
    compile_program_to_bytes, compile_program_to_file_with_options, emit_wit, is_component,
    wrap_as_component, BuildOptions, WasmTarget,
};

#[test]
fn wraps_empty_main_into_a_valid_component() {
    let prog = common::empty_main();
    let core = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("core");
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("wit");
    let comp = wrap_as_component(&core, &doc).expect("wrap");
    assert!(is_component(&comp), "expected component preamble");

    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&comp).expect("component validates");
}

#[test]
fn write_to_file_default_emits_component() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join("hello.wasm");
    let prog = common::empty_main();
    let opts = BuildOptions::new("hello");
    let art =
        compile_program_to_file_with_options(&prog, WasmTarget::Wasi, &out, &opts).expect("write");
    assert!(out.exists());
    assert_eq!(
        art.format,
        sdust_codegen_wasm::artifact::WasmFormat::Component
    );
    let bytes = std::fs::read(&out).expect("read");
    assert!(is_component(&bytes));
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes).expect("re-read validates");
}

#[test]
fn component_carries_component_type_custom_section() {
    let prog = common::empty_main();
    let core = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("core");
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("wit");
    let comp = wrap_as_component(&core, &doc).expect("wrap");
    // Walk payloads and confirm a component-typed section is present.
    let mut saw_component_section = false;
    for payload in wasmparser::Parser::new(0).parse_all(&comp).flatten() {
        if let wasmparser::Payload::ComponentTypeSection(_) = payload {
            saw_component_section = true;
            break;
        }
    }
    assert!(
        saw_component_section,
        "expected at least one ComponentTypeSection"
    );
}
