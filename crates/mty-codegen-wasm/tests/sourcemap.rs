//! End-to-end: build wasm + name section + source-map sidecar, then
//! parse the wasm back with wasmparser and the sidecar with serde_json
//! to confirm both are structurally valid.

use mty_codegen_wasm::sourcemap::{
    append_debug_sections, build_name_section, build_source_map, sidecar_relative_filename,
    sourcemap_sidecar_path, write_sourcemap_sidecar,
};
use mty_codegen_wasm::{compile_program_to_bytes, WasmTarget};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Program, Term,
};

fn empty_main_prog() -> Program {
    let mut p = Program::default();
    p.fns.push(Function {
        id: IrFnId(0),
        name: "main".into(),
        params: vec![],
        locals: vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Unit,
            mutable: false,
            source: LocalSource::Return,
        }],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 12 },
    });
    p
}

#[test]
fn emits_name_custom_section() {
    let p = empty_main_prog();
    let core = compile_program_to_bytes(&p, WasmTarget::Wasi).expect("core wasm");
    let ns = build_name_section(&p, 1); // 1 import (log)
    let with_name = append_debug_sections(core.clone(), &ns, "ignored.map");
    // Validate the new wasm still parses.
    let mut validator = wasmparser::Validator::new();
    validator.validate_all(&with_name).expect("valid wasm");

    // Walk top-level sections; look for a custom section named "name".
    let mut found_name_section = false;
    let mut found_smu_section = false;
    let parser = wasmparser::Parser::new(0);
    for payload in parser.parse_all(&with_name) {
        let payload = payload.expect("payload ok");
        if let wasmparser::Payload::CustomSection(reader) = payload {
            match reader.name() {
                "name" => found_name_section = true,
                "sourceMappingURL" => found_smu_section = true,
                _ => {}
            }
        }
    }
    assert!(found_name_section, "wasm has a `name` custom section");
    assert!(found_smu_section, "wasm has a `sourceMappingURL` section");
}

#[test]
fn emits_valid_sourcemap_v3_sidecar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let wasm_path = dir.path().join("hello.wasm");
    let p = empty_main_prog();
    let sm = build_source_map(&p, "hello.mty", "fn main() {}\n", "hello.wasm");
    let sidecar = write_sourcemap_sidecar(&wasm_path, &sm).expect("write sidecar");
    let expected = sourcemap_sidecar_path(&wasm_path);
    assert_eq!(sidecar, expected);
    assert!(sidecar.exists());

    // Parse + spot-check structure.
    let bytes = std::fs::read(&sidecar).expect("read sidecar");
    let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid json");
    assert_eq!(v["version"], 3);
    assert_eq!(v["sources"][0], "hello.mty");
    assert_eq!(v["file"], "hello.wasm");
    assert!(v["mappings"].is_string());
    // sourcesContent should preserve our literal source.
    assert!(v["sourcesContent"][0]
        .as_str()
        .map(|s| s.contains("fn main()"))
        .unwrap_or(false));
}

#[test]
fn sidecar_relative_filename_matches_url_section() {
    let p = empty_main_prog();
    let core = compile_program_to_bytes(&p, WasmTarget::Wasi).expect("core");
    let ns = build_name_section(&p, 1);
    let sidecar_name = "hello.wasm.map";
    let with_dbg = append_debug_sections(core, &ns, sidecar_name);
    let parser = wasmparser::Parser::new(0);
    for payload in parser.parse_all(&with_dbg) {
        let payload = payload.unwrap();
        if let wasmparser::Payload::CustomSection(reader) = payload {
            if reader.name() == "sourceMappingURL" {
                let data = reader.data();
                // The URL is encoded as a wasm "name" (uleb128 len + bytes).
                let needle = sidecar_name.as_bytes();
                assert!(
                    data.windows(needle.len()).any(|w| w == needle),
                    "sourceMappingURL section should contain the sidecar filename",
                );
            }
        }
    }
    // Also exercise sidecar_relative_filename to confirm round-trip.
    let p2 = sourcemap_sidecar_path(std::path::Path::new("out/hello.wasm"));
    assert_eq!(sidecar_relative_filename(&p2), sidecar_name);
}
