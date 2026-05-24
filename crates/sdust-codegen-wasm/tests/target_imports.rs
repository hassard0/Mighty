//! Per-target import shape: `wasm32-wasi` should pull host fns from
//! `wasi:*`; `wasm32-web` should pull them from `stardust:web/*`.

mod common;

use sdust_codegen_wasm::{emit_wit, WasmTarget};

#[test]
fn wasi_target_imports_use_wasi_namespace() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "hello", WasmTarget::Wasi).expect("emit");
    assert!(
        doc.text.contains("import wasi:cli/log"),
        "wasi target missing wasi:cli import. wit was: {}",
        doc.text
    );
    assert!(
        !doc.text.contains("import stardust:web"),
        "wasi target should not import stardust:web"
    );
}

#[test]
fn web_target_imports_use_stardust_web_namespace() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "hello", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("import stardust:web/log"),
        "web target missing stardust:web import. wit was: {}",
        doc.text
    );
    assert!(
        !doc.text.contains("import wasi:"),
        "web target should not import wasi:*"
    );
}

#[test]
fn caps_imports_emitted_for_cap_typed_locals() {
    use sdust_hir::SourceSpan;
    use sdust_sir::sir::{
        Block, BlockId, Const, Function, LocalDecl, LocalSource, Operand, SirFnId, SirTy, Term,
    };
    use sdust_types::{CapConstraint, CapFamily};
    let mut prog = common::empty_main();
    // fn use_fs(fs: Fs) — introduces a Cap-typed local.
    prog.fns.push(Function {
        id: SirFnId(1),
        name: "use_fs".into(),
        params: vec![sdust_sir::sir::Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: SirTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "fs".into(),
                ty: SirTy::Cap {
                    family: CapFamily::Fs,
                    constraint: CapConstraint::Any,
                },
                mutable: false,
                source: LocalSource::Param,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: SirTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    let doc = emit_wit(&prog, "demo", WasmTarget::Wasi).expect("emit");
    assert!(
        doc.text.contains("import stardust:caps/fs"),
        "expected caps/fs import. wit was: {}",
        doc.text
    );
}
