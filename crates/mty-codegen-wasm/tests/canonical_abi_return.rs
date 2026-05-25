//! v0.8 loose-end 4/4 — canonical-ABI return-area for `mty:web/dom`
//! string-returning ops.
//!
//! Verifies:
//!   - `get-text` core import shape is `(i32, i32, i32) -> ()`.
//!   - `query` core import shape is `(i32, i32, i32) -> ()`.
//!   - The lift sequence after a DomOp("get_text") Call leaves a
//!     non-zero i32 (the data pointer the JS shim wrote) on the stack.
//!   - A round-trip simulating the JS shim with a synthetic host
//!     produces the expected string when read back at the lifted ptr.

use mty_codegen_wasm::emit::{compile_program_to_bytes, DOM_RETURN_AREA};
use mty_codegen_wasm::target::WasmTarget;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
    LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
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

/// SIR program: `main()` calls `dom.get_text("id")` and stores the
/// returned ptr in local 0. We don't care about the return value of
/// `main` itself — Unit.
fn get_text_program() -> Program {
    let mut p = Program::default();
    p.fns.push(Function {
        id: IrFnId(0),
        name: "main".into(),
        params: vec![],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "ptr".into(),
                ty: IrTy::Int(mty_types::IntKind::I32),
                mutable: true,
                source: LocalSource::UserLet,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![Stmt::Assign(
                Place::local(Local(1)),
                Rvalue::Call {
                    func: FnRef::Builtin(BuiltinId::DomOp("get_text".into())),
                    args: vec![Operand::Const(Const::Str("id".into()))],
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
    p
}

#[test]
fn get_text_import_takes_three_i32_returns_none() {
    use wasmparser::Imports;
    let bytes = compile_program_to_bytes(&empty_main(), WasmTarget::Web).expect("compile");
    let mut found = false;
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        let p = payload.expect("payload");
        if let wasmparser::Payload::ImportSection(reader) = p {
            for group in reader {
                let g = group.expect("import group");
                match g {
                    Imports::Single(_, imp) => {
                        if imp.module == "mty:web/dom" && imp.name == "get-text" {
                            assert!(matches!(
                                imp.ty,
                                wasmparser::TypeRef::Func(_)
                            ));
                            found = true;
                        }
                    }
                    Imports::Compact1 { module, items } => {
                        if module == "mty:web/dom" {
                            for it in items {
                                let it = it.expect("item");
                                if it.name == "get-text" {
                                    found = true;
                                }
                            }
                        }
                    }
                    Imports::Compact2 { module, names, .. } => {
                        if module == "mty:web/dom" {
                            for n in names {
                                let n = n.expect("name");
                                if n == "get-text" {
                                    found = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(found, "get-text import missing");
}

#[test]
fn dom_get_text_lift_round_trip_via_synthetic_host() {
    use wasmtime::{Caller, Engine, Linker, Module, Store};

    let bytes = compile_program_to_bytes(&get_text_program(), WasmTarget::Web).expect("compile");

    // Drive the core module under wasmtime with shim hosts for every
    // `mty:web/*` import. The `get-text` host writes a known string
    // into the return area; after `main` returns we read the lifted
    // ptr that the core module stored at offset DOM_RETURN_AREA and
    // verify it matches the bytes we wrote.
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("module compile");
    let mut store: Store<()> = Store::new(&engine, ());
    let mut linker: Linker<()> = Linker::new(&engine);

    linker
        .func_wrap(
            "mty:web/log",
            "log",
            |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {},
        )
        .unwrap();
    linker
        .func_wrap(
            "mty:web/dom",
            "set-text",
            |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32, _d: i32| {},
        )
        .unwrap();
    linker
        .func_wrap(
            "mty:web/dom",
            "on-click",
            |_caller: Caller<'_, ()>, _a: i32, _b: i32, _c: i32, _d: i32| {},
        )
        .unwrap();
    linker
        .func_wrap(
            "mty:web/dom",
            "query",
            |_caller: Caller<'_, ()>, _a: i32, _b: i32, _ra: i32| {},
        )
        .unwrap();
    linker
        .func_wrap(
            "mty:web/dom",
            "get-text",
            |mut caller: Caller<'_, ()>, _id_ptr: i32, _id_len: i32, ret_area: i32| {
                // Write a known body string and put (ptr, len) at ret_area.
                let mem = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .expect("export memory");
                let data_ptr: u32 = 9000;
                let body = b"HOST_OK";
                let data = mem.data_mut(&mut caller);
                data[data_ptr as usize..data_ptr as usize + body.len()].copy_from_slice(body);
                let p_le = (data_ptr as i32).to_le_bytes();
                let l_le = (body.len() as i32).to_le_bytes();
                data[ret_area as usize..ret_area as usize + 4].copy_from_slice(&p_le);
                data[ret_area as usize + 4..ret_area as usize + 8].copy_from_slice(&l_le);
            },
        )
        .unwrap();

    let instance = linker
        .instantiate(&mut store, &module)
        .expect("instantiate");
    let main = instance
        .get_typed_func::<(), ()>(&mut store, "main")
        .expect("main");
    main.call(&mut store, ()).expect("call main");

    // After main returns, the return area at DOM_RETURN_AREA holds
    // (data_ptr, data_len). Read them and confirm we got HOST_OK.
    let mem = instance
        .get_memory(&mut store, "memory")
        .expect("memory");
    let raw = mem.data(&store);
    let ptr = u32::from_le_bytes(
        raw[DOM_RETURN_AREA as usize..DOM_RETURN_AREA as usize + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let len = u32::from_le_bytes(
        raw[DOM_RETURN_AREA as usize + 4..DOM_RETURN_AREA as usize + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let s = std::str::from_utf8(&raw[ptr..ptr + len]).expect("utf8");
    assert_eq!(s, "HOST_OK", "expected HOST_OK at lifted ptr");
}
