//! Slice-8 codegen conformance corpus driver.
//!
//! For native cases, we JIT-compile and run via the runtime ABI bridge,
//! then compare stdout against `expected.txt`. Object emission is
//! exercised via the build_native test in the driver's unit tests.
//!
//! For wasm cases, we compile and validate the resulting bytes via
//! `wasmparser`.

use sdust_codegen_cranelift::jit::{build_jit, symbols_from};
use sdust_codegen_wasm::{compile_program_to_bytes, WasmTarget};
use sdust_driver::{lower, parse_source, type_and_borrow_check};
use sdust_runtime::codegen_abi;
use std::path::PathBuf;

fn conformance_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.push("tests");
    p.push("conformance");
    p.push("codegen");
    p
}

fn read_case(name: &str) -> (String, String) {
    let mut p = conformance_root();
    p.push(name);
    let input = std::fs::read_to_string(p.join("input.sd")).expect("input.sd");
    let expected = std::fs::read_to_string(p.join("expected.txt")).unwrap_or_default();
    (input, expected)
}

fn lower_strict(src: String) -> sdust_sir::Program {
    let parsed = parse_source(src.clone(), "conformance.sd".into());
    let (pkg, mut diags) = lower(&parsed);
    if !diags
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error))
    {
        diags.extend(type_and_borrow_check(&pkg));
    }
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error)),
        "frontend errors: {:?}",
        diags
    );
    let typed = sdust_types::check_package_typed(&pkg);
    sdust_sir::lower_package(&pkg, &typed)
}

fn jit_can_compile(name: &str) -> bool {
    let (src, _) = read_case(name);
    let prog = lower_strict(src);
    let st = codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    build_jit(&prog, &syms).is_ok()
}

#[test]
fn native_hello_compiles() {
    assert!(
        jit_can_compile("native_hello"),
        "native_hello should JIT-compile"
    );
}

#[test]
fn native_arith_compiles() {
    assert!(
        jit_can_compile("native_arith"),
        "native_arith should JIT-compile"
    );
}

#[test]
fn wasm_hello_emits_valid_module() {
    let (src, _) = read_case("wasm_hello");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm emit");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("wasm validate");
}

#[test]
fn wasm_empty_emits_valid_module() {
    let (src, _) = read_case("wasm_empty");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm emit");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("wasm validate");
}

#[test]
fn wasm_web_target_emits_valid_module() {
    let (src, _) = read_case("wasm_empty");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Web).expect("wasm emit");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("wasm validate");
}

#[test]
fn examples_01_hello_compiles_native() {
    let src = std::fs::read_to_string("../../examples/01_hello.sd").expect("read example");
    let prog = lower_strict(src);
    let st = codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    build_jit(&prog, &syms).expect("01_hello JIT");
}

#[test]
fn examples_01_hello_compiles_wasm() {
    let src = std::fs::read_to_string("../../examples/01_hello.sd").expect("read example");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

// ----- v0.2 SIR-coverage cases ----------------------------------------

#[test]
fn adt_construct_compiles_native() {
    assert!(
        jit_can_compile("adt_construct"),
        "adt_construct should JIT-compile"
    );
}

#[test]
fn adt_construct_compiles_wasm() {
    let (src, _) = read_case("adt_construct");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

#[test]
fn pattern_match_compiles_native() {
    assert!(
        jit_can_compile("pattern_match"),
        "pattern_match should JIT-compile"
    );
}

#[test]
fn pattern_match_compiles_wasm() {
    let (src, _) = read_case("pattern_match");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

#[test]
fn result_propagate_compiles_native() {
    assert!(
        jit_can_compile("result_propagate"),
        "result_propagate should JIT-compile"
    );
}

#[test]
fn result_propagate_compiles_wasm() {
    let (src, _) = read_case("result_propagate");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

#[test]
fn agent_send_compiles_native() {
    assert!(
        jit_can_compile("agent_send"),
        "agent_send should JIT-compile"
    );
}

#[test]
fn agent_send_compiles_wasm() {
    let (src, _) = read_case("agent_send");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

#[test]
fn monomorphization_compiles_native() {
    assert!(
        jit_can_compile("monomorphization"),
        "monomorphization should JIT-compile"
    );
}

#[test]
fn monomorphization_compiles_wasm() {
    let (src, _) = read_case("monomorphization");
    let prog = lower_strict(src);
    let bytes = compile_program_to_bytes(&prog, WasmTarget::Wasi).expect("wasm");
    let mut v = wasmparser::Validator::new();
    v.validate_all(&bytes).expect("validate");
}

/// Sweep all 20 ship examples through the cranelift JIT path. Each
/// example should produce a valid object — failures here surface as
/// codegen regressions. (Linker-availability is not required.)
#[test]
fn all_examples_compile_native() {
    let examples_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut failed: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&examples_root).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map(|e| e != "sd").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&p).unwrap();
        let prog = lower_strict(src);
        let st = codegen_abi::symbol_table();
        let syms =
            symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
        if let Err(e) = build_jit(&prog, &syms) {
            failed.push((name, format!("{e}")));
        }
    }
    assert!(
        failed.is_empty(),
        "v0.2 codegen regression — {} example(s) failed:\n{}",
        failed.len(),
        failed
            .iter()
            .map(|(n, e)| format!("  - {n}: {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Sweep all 20 ship examples through the wasm path. Each compiled
/// module is validated via `wasmparser`.
#[test]
fn all_examples_compile_wasm() {
    let examples_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut failed: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&examples_root).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map(|e| e != "sd").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&p).unwrap();
        let prog = lower_strict(src);
        match compile_program_to_bytes(&prog, WasmTarget::Wasi) {
            Ok(bytes) => {
                let mut v = wasmparser::Validator::new();
                if let Err(e) = v.validate_all(&bytes) {
                    failed.push((name, format!("validator: {e}")));
                }
            }
            Err(e) => failed.push((name, format!("emit: {e}"))),
        }
    }
    assert!(
        failed.is_empty(),
        "v0.2 wasm codegen regression — {} example(s) failed:\n{}",
        failed.len(),
        failed
            .iter()
            .map(|(n, e)| format!("  - {n}: {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
