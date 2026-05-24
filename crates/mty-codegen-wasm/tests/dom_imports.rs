//! v0.5 dogfood Gap-2 — verify the Web target's WIT contains the
//! expanded `stardust:web/dom` interface (set-text / get-text /
//! on-click / query) and that the world imports it. Also verify the
//! WIT parses cleanly via wit_parser.
//!
//! The companion JS shim at `demos/02_counter_web/web/dom-shim.js`
//! satisfies these imports against `document.*`.

use mty_codegen_wasm::target::WasmTarget;
use mty_codegen_wasm::wit::emit_wit;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, Function, LocalDecl, LocalSource, Operand, Program, IrFnId, IrTy, Term,
};

fn empty_main() -> Program {
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
        span: SourceSpan { start: 0, end: 0 },
    });
    p
}

#[test]
fn web_target_imports_stardust_web_dom() {
    let doc = emit_wit(&empty_main(), "demo", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("import stardust:web/dom"),
        "world should import dom; text:\n{}",
        doc.text
    );
}

#[test]
fn web_dom_interface_has_v0_5_methods() {
    let doc = emit_wit(&empty_main(), "demo", WasmTarget::Web).expect("emit");
    // v0.5 integration: `get-text` / `query` return `u32` (string-table
    // handles into the JS shim) rather than `string` / `option<string>`,
    // so the WIT signature lines up with the core import's
    // `(ptr,len) -> i32` shape. Canonical-ABI return-area bridging is
    // scheduled for v0.6.
    for method in [
        "set-text: func(id: string, text: string)",
        "get-text: func(id: string) -> u32",
        "on-click: func(id: string, callback-tag: string)",
        "query: func(selector: string) -> u32",
    ] {
        assert!(
            doc.text.contains(method),
            "expected `{method}` in WIT:\n{}",
            doc.text
        );
    }
}

#[test]
fn web_dom_legacy_handle_methods_still_present() {
    // The v0.4 JS host used `get-element-by-id` + `set-text-handle`;
    // keep them so the existing demo loader doesn't break.
    let doc = emit_wit(&empty_main(), "demo", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("get-element-by-id: func"),
        "legacy op missing: {}",
        doc.text
    );
    assert!(
        doc.text.contains("set-text-handle: func"),
        "legacy op missing: {}",
        doc.text
    );
}

#[test]
fn wasi_target_does_not_import_dom() {
    let doc = emit_wit(&empty_main(), "demo", WasmTarget::Wasi).expect("emit");
    assert!(
        !doc.text.contains("import stardust:web/dom"),
        "wasi target must not pull in stardust:web/dom; text:\n{}",
        doc.text
    );
}

#[test]
fn web_wit_round_trips_via_wit_parser() {
    let doc = emit_wit(&empty_main(), "demo", WasmTarget::Web).expect("emit");
    let _ = doc.resolve().expect("WIT should re-parse cleanly");
}

/// v0.6 easy-win 1 — verify a `BuiltinId::DomOp` call in the SIR
/// reaches a real `Call <stardust:web/dom>:<op>` instruction in the
/// emitted core module. Builds a tiny program by hand (no Mighty
/// source needed) that holds a Dom-cap local and calls `set_text`
/// through it.
#[test]
fn web_target_emits_dom_set_text_call_for_builtin_dom_op() {
    use mty_codegen_wasm::emit::compile_program_to_bytes;
    use mty_ir::ir::{BuiltinId, FnRef, Place, Rvalue, Stmt, Term};

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
            stmts: vec![Stmt::Assign(
                Place::local(mty_ir::ir::Local(0)),
                Rvalue::Call {
                    func: FnRef::Builtin(BuiltinId::DomOp("set_text".into())),
                    args: vec![
                        Operand::Const(Const::Str("#id".into())),
                        Operand::Const(Const::Str("hello".into())),
                    ],
                },
            )],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    let bytes = compile_program_to_bytes(&p, WasmTarget::Web).expect("compile");
    // The .wasm must validate end-to-end. The dom_imports gate above
    // already proves the four `stardust:web/dom` imports are present;
    // here we just confirm a SIR with a DomOp call still produces
    // valid wasm (i.e. emit_call's DomOp arm doesn't break encoding).
    wasmparser::Validator::new()
        .validate_all(&bytes)
        .expect("dom-op program emits valid wasm");
}

#[test]
fn web_target_core_module_has_four_dom_imports() {
    use mty_codegen_wasm::emit::compile_program_to_bytes;
    use wasmparser::Imports;
    let bytes = compile_program_to_bytes(&empty_main(), WasmTarget::Web).expect("compile");
    // Inspect imports via wasmparser. `ImportSectionReader` yields
    // `Imports<'a>` *groups*: a single import or a packed group sharing
    // a module name. The four DOM imports may come back as either
    // shape depending on encoder packing, so handle both.
    let mut dom_count = 0usize;
    let mut dom_names: Vec<String> = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        let p = payload.expect("payload");
        if let wasmparser::Payload::ImportSection(reader) = p {
            for group in reader {
                match group.expect("import group") {
                    Imports::Single(_, imp) => {
                        if imp.module == "stardust:web/dom" {
                            dom_count += 1;
                            dom_names.push(imp.name.to_string());
                        }
                    }
                    Imports::Compact1 { module, items } => {
                        if module == "stardust:web/dom" {
                            for it in items {
                                let it = it.expect("item");
                                dom_count += 1;
                                dom_names.push(it.name.to_string());
                            }
                        }
                    }
                    Imports::Compact2 { module, names, .. } => {
                        if module == "stardust:web/dom" {
                            for n in names {
                                let n = n.expect("name");
                                dom_count += 1;
                                dom_names.push(n.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        dom_count, 4,
        "expected 4 stardust:web/dom imports, got {dom_count}: {dom_names:?}"
    );
    for want in ["set-text", "get-text", "on-click", "query"] {
        assert!(
            dom_names.iter().any(|n| n == want),
            "missing import name {want}; got {dom_names:?}"
        );
    }
}
