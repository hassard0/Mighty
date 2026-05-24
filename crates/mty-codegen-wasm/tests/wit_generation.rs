//! WIT generation: emit a WIT document for a fixture program,
//! re-parse it with `wit_parser`, and assert the expected exports
//! and imports are present.

mod common;

use mty_codegen_wasm::{emit_wit, WasmTarget};

#[test]
fn empty_main_wit_round_trips() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    // Must mention main fn export in the lib interface.
    assert!(doc.text.contains("export main: func()"));
    // Re-parse must succeed.
    let (_resolve, _pkg, _world) = doc.resolve().expect("resolve");
}

#[test]
fn struct_lowers_to_record() {
    let prog = common::program_with_adts_and_fn();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    assert!(
        doc.text.contains("record point"),
        "expected record point in wit: {}",
        doc.text
    );
    assert!(doc.text.contains("x: s32"));
    assert!(doc.text.contains("y: s32"));
}

#[test]
fn payload_free_enum_uses_enum_keyword() {
    let prog = common::program_with_adts_and_fn();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    // Color has no payloads → should be `enum`, not `variant`.
    assert!(doc.text.contains("enum color"), "wit was: {}", doc.text);
    assert!(doc.text.contains("red"));
    assert!(doc.text.contains("green"));
    assert!(doc.text.contains("blue"));
}

#[test]
fn payload_enum_uses_variant_keyword() {
    let prog = common::program_with_adts_and_fn();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    // Shape carries payloads → variant.
    assert!(doc.text.contains("variant shape"), "wit was: {}", doc.text);
    assert!(doc.text.contains("circle(f64)"));
    assert!(doc.text.contains("square(f64)"));
}

#[test]
fn public_fn_signature_round_trips() {
    let prog = common::program_with_adts_and_fn();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    assert!(
        doc.text.contains("export add: func(a: s32, b: s32) -> s32"),
        "wit was: {}",
        doc.text
    );
}

#[test]
fn world_exports_functions_inline() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    // Functions are declared inline in the world, not via
    // `export lib;`, so the core-wasm export name stays simple
    // (`main`) and matches the slice-8 lowerer.
    assert!(doc.text.contains("export main: func"));
}

#[test]
fn package_id_is_kebab_normalized() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "Hello_World", WasmTarget::Wasi).expect("emit");
    assert_eq!(doc.package_id, "stardust:hello-world");
}

#[test]
fn private_underscore_fns_are_not_exported() {
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, LocalDecl, LocalSource, Operand, Term,
    };
    let mut prog = common::empty_main();
    prog.fns.push(Function {
        id: IrFnId(1),
        name: "_helper".into(),
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
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    assert!(
        !doc.text.contains("export helper: func"),
        "underscore-prefixed fn leaked: {}",
        doc.text
    );
}
