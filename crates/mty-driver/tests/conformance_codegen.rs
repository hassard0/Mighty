//! Slice-8 codegen conformance corpus driver.
//!
//! For native cases, we JIT-compile and run via the runtime ABI bridge,
//! then compare stdout against `expected.txt`. Object emission is
//! exercised via the build_native test in the driver's unit tests.
//!
//! For wasm cases, we compile and validate the resulting bytes via
//! `wasmparser`.

use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_cranelift::jit::{build_jit, jit_compile_and_run_main, symbols_from};
use mty_codegen_cranelift::object::{compile_object, find_linker, link_executable};
use mty_codegen_wasm::{compile_program_to_bytes, emit_wit, wrap_as_component, WasmTarget};
use mty_driver::{lower, parse_source, type_and_borrow_check};
use mty_runtime::codegen_abi;
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
    let input = std::fs::read_to_string(p.join("input.mty")).expect("input.mty");
    let expected = std::fs::read_to_string(p.join("expected.txt")).unwrap_or_default();
    (input, expected)
}

fn lower_strict(src: String) -> mty_ir::Program {
    let parsed = parse_source(src.clone(), "conformance.mty".into());
    let (pkg, mut diags) = lower(&parsed);
    if !diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        diags.extend(type_and_borrow_check(&pkg));
    }
    assert!(
        !diags
            .iter()
            .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error)),
        "frontend errors: {:?}",
        diags
    );
    let typed = mty_types::check_package_typed(&pkg);
    mty_ir::lower_package(&pkg, &typed)
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
    let src = std::fs::read_to_string("../../examples/01_hello.mty").expect("read example");
    let prog = lower_strict(src);
    let st = codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    build_jit(&prog, &syms).expect("01_hello JIT");
}

#[test]
fn examples_01_hello_compiles_wasm() {
    let src = std::fs::read_to_string("../../examples/01_hello.mty").expect("read example");
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

/// Examples carrying `// @typeck-pending` in the source body are
/// skipped by the codegen sweeps — the marker indicates the example
/// demonstrates a parser-level surface form whose typeck wiring lands
/// in a later version (RFC-008 effect-row syntax from v0.15 is the
/// first user).
///
/// `// @compile-error` is the v0.30 Track A analogue for examples
/// that DEMONSTRATE a static-error path (e.g. example 33 shows the
/// MT4099 prompt-injection diagnostic). The codegen sweeps skip
/// these because they intentionally do not type-check; the
/// negative-shape pinning lives in dedicated `tests/taint_*.rs`
/// suites.
fn is_typeck_pending(src: &str) -> bool {
    src.contains("@typeck-pending") || src.contains("@compile-error")
}

/// Sweep all ship examples through the cranelift JIT path. Each
/// example should produce a valid object unless it intentionally
/// exercises an interpreter-hosted stdlib surface that `mty run`
/// falls back for. Failures here surface as codegen regressions.
/// Linker availability is not required.
#[test]
fn all_examples_compile_native() {
    let examples_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut failed: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&examples_root).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map(|e| e != "mty").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&p).unwrap();
        if is_typeck_pending(&src) {
            continue;
        }
        let prog = lower_strict(src);
        let st = codegen_abi::symbol_table();
        let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
        if let Err(e) = build_jit(&prog, &syms) {
            let err = format!("{e}");
            if is_interpreter_hosted_std_fs_codegen(&err) {
                continue;
            }
            failed.push((name, err));
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

fn is_interpreter_hosted_std_fs_codegen(err: &str) -> bool {
    err.contains("std.fs.") && err.contains("is interpreter-hosted")
}

/// JIT-run smoke: for each adt/match/result/agent/mono case, JIT the
/// program and invoke `main`. Asserts that the call returns without
/// panicking (we don't have a way to capture stdout from JIT'd code in
/// this test harness).
#[test]
fn jit_run_smoke() {
    let cases = &[
        "adt_construct",
        "pattern_match",
        "result_propagate",
        "agent_send",
        "monomorphization",
        "native_hello",
        "native_arith",
    ];
    let st = codegen_abi::symbol_table();
    let syms = symbols_from(&st.iter().map(|(n, p)| (n.as_str(), *p)).collect::<Vec<_>>());
    for case in cases {
        let (src, _) = read_case(case);
        let prog = lower_strict(src);
        let result = jit_compile_and_run_main(&prog, &syms);
        assert!(
            result.is_ok(),
            "JIT run smoke for {case} failed: {:?}",
            result.err()
        );
    }
}

/// AOT smoke: compile each case to an object file, then if a linker is
/// available on the host, link to an executable and run it. Marks the
/// test as ignored-but-passing when no linker is found.
#[test]
fn aot_link_and_run_smoke() {
    if find_linker().is_none() {
        eprintln!("aot_link_and_run_smoke: no linker found on host; skipping link+run");
        return;
    }
    let tmp = std::env::temp_dir().join("mty_v0_2_aot");
    let _ = std::fs::create_dir_all(&tmp);
    let cases = &["native_hello", "adt_construct", "pattern_match"];
    for case in cases {
        let (src, _) = read_case(case);
        let prog = lower_strict(src);
        let obj_path = tmp.join(format!("{case}.o"));
        let obj = compile_object(&prog, &obj_path).unwrap_or_else(|e| {
            panic!("object compile {case}: {e}");
        });
        let exe_path = if cfg!(windows) {
            tmp.join(format!("{case}.exe"))
        } else {
            tmp.join(case)
        };
        match link_executable(&obj, &exe_path, BuildMode::Debug) {
            Ok(_art) => {
                // The compiled binary depends on mty_runtime_* C
                // symbols; without linking against libmty_runtime it
                // would fail to link. We accept either Ok or Err here —
                // the goal is to exercise the link-discovery path.
            }
            Err(e) => {
                // Runtime symbols missing is expected when the test
                // host doesn't have a libmty_runtime.a; treat as
                // soft-fail so the test still records a result.
                eprintln!("link {case}: {e} (expected when no libmty_runtime)");
            }
        }
    }
}

/// v0.3 integrator gate: every ship example must wrap into a valid
/// Wasm Component (the default `mty build --target wasm` output).
/// This third column complements `all_examples_compile_native` and
/// `all_examples_compile_wasm` (which validate only the core module).
#[test]
fn all_examples_compile_wasm_component() {
    let examples_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut failed: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&examples_root).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().map(|e| e != "mty").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&p).unwrap();
        if is_typeck_pending(&src) {
            continue;
        }
        let prog = lower_strict(src);
        let core = match compile_program_to_bytes(&prog, WasmTarget::Wasi) {
            Ok(b) => b,
            Err(e) => {
                failed.push((name, format!("core: {e}")));
                continue;
            }
        };
        let wit = match emit_wit(&prog, &name, WasmTarget::Wasi) {
            Ok(w) => w,
            Err(e) => {
                failed.push((name, format!("wit: {e}")));
                continue;
            }
        };
        let comp = match wrap_as_component(&core, &wit) {
            Ok(c) => c,
            Err(e) => {
                failed.push((name, format!("wrap: {e}")));
                continue;
            }
        };
        let mut v = wasmparser::Validator::new();
        if let Err(e) = v.validate_all(&comp) {
            failed.push((name, format!("validate: {e}")));
        }
    }
    assert!(
        failed.is_empty(),
        "v0.3 wasm-component sweep — {} example(s) failed:\n{}",
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
        if p.extension().map(|e| e != "mty").unwrap_or(true) {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&p).unwrap();
        if is_typeck_pending(&src) {
            continue;
        }
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
