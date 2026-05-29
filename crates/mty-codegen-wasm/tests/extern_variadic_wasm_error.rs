//! v0.37 T6 — variadic extern fns can't be lowered to wasm.
//!
//! Core wasm function types are fully-typed and the Component Model
//! FFI surface has no varargs ABI. The wasm emitter rejects a program
//! that declares a variadic extern, with a single line pointing at
//! the matrix doc.
//!
//! The cranelift backend still accepts the same shape (modulo "call
//! with extras" — see `crates/mty-codegen-cranelift/`).

mod common;

use mty_codegen_wasm::emit::compile_program_to_bytes;
use mty_codegen_wasm::target::WasmTarget;
use mty_codegen_wasm::WasmError;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, ExternBinding, Function, IrFnId, IrTy, Local, LocalDecl, LocalSource, Operand,
    Program, Term,
};
use mty_types::IntKind;

/// Build a minimal program with one variadic extern fn + a no-op main.
fn variadic_extern_program(abi: &str) -> Program {
    let mut p = Program::default();

    // ---- variadic extern fn ----
    let extern_id = IrFnId(0);
    p.fns.push(Function {
        id: extern_id,
        name: "printf".into(),
        params: vec![Local(1)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "fmt".into(),
                ty: IrTy::Int(IntKind::I64), // pretend pointer
                mutable: false,
                source: LocalSource::Param,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(mty_ir::ir::Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    p.extern_bindings.insert(
        extern_id,
        ExternBinding {
            abi: abi.into(),
            name: "printf".into(),
            // ★ the only material difference from the non-variadic
            // tests in `extern_js_imports.rs`.
            is_variadic: true,
        },
    );

    // ---- main fn ----
    let main_id = IrFnId(1);
    p.fns.push(Function {
        id: main_id,
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
            terminator: Term::Return(Operand::Const(mty_ir::ir::Const::Unit)),
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
fn variadic_extern_c_rejected_in_wasm32_wasi() {
    let p = variadic_extern_program("c");
    let err = compile_program_to_bytes(&p, WasmTarget::Wasi)
        .expect_err("variadic extern in wasm should error");
    match err {
        WasmError::Unsupported(msg) => {
            assert!(
                msg.contains("variadic extern fn `printf`"),
                "error didn't mention the fn name: {msg}"
            );
            assert!(
                msg.contains("extern-c-matrix"),
                "error didn't point at the matrix doc: {msg}"
            );
        }
        other => panic!("expected WasmError::Unsupported, got {other:?}"),
    }
}

#[test]
fn variadic_extern_c_rejected_in_wasm32_web() {
    let p = variadic_extern_program("c");
    let err = compile_program_to_bytes(&p, WasmTarget::Web)
        .expect_err("variadic extern in wasm should error");
    assert!(matches!(err, WasmError::Unsupported(_)));
}

#[test]
fn variadic_extern_js_also_rejected() {
    // Even abi=js can't carry varargs — the JS ABI lowering goes
    // through the same Component Model surface.
    let p = variadic_extern_program("js");
    let err = compile_program_to_bytes(&p, WasmTarget::Web)
        .expect_err("variadic extern js in wasm should error");
    assert!(matches!(err, WasmError::Unsupported(_)));
}

#[test]
fn non_variadic_extern_c_still_skipped_cleanly() {
    // Sanity: dropping the `is_variadic` flag back to false brings us
    // back to the v0.36 T2 behaviour (skip emission, no error).
    let mut p = variadic_extern_program("c");
    if let Some(b) = p.extern_bindings.get_mut(&IrFnId(0)) {
        b.is_variadic = false;
    }
    let _bytes = compile_program_to_bytes(&p, WasmTarget::Wasi)
        .expect("non-variadic extern c should compile fine");
}
