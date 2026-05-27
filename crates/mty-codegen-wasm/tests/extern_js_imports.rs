//! v0.25 Track B — `extern js { fn _foo() }` actually emits a wasm
//! `(import "mty:web/js" "_foo" ...)` declaration in the core module.
//!
//! Before this slice the wasm32-web emitter treated `extern js` fns
//! as ordinary user fns: they got an empty body and never appeared in
//! the import section. User code that called `_alert("hi")` therefore
//! ran a stub fn instead of crossing the JS boundary — `extern js`
//! was effectively documentation only (the v0.24 Track E gap).
//!
//! After this slice each `extern js { fn name(args) -> ret }` produces
//! a real import slot at `mty:web/js#name`, the call dispatch routes
//! through that slot, and `wasmparser` verifies the import + call
//! pair is present.
//!
//! The six tests below cover:
//!
//! 1. `extern_js_fn_emits_import` — minimal case: `extern js { fn
//!    _foo() }`; the emitted core module has a single import for
//!    `_foo` under the `mty:web/js` namespace.
//! 2. `extern_js_fn_with_args` — `extern js { fn _bar(x: I32, y: I32) }`;
//!    the import signature is `(param i32 i32)`.
//! 3. `extern_js_fn_with_return` — `extern js { fn _len(s: Str) -> U32 }`;
//!    the import returns `(result i32)` and a string param expands to
//!    `(ptr i32, len i32)` per the canonical-ABI flat layout.
//! 4. `extern_js_call_routes_to_import` — user code calls `_foo()`;
//!    the wasm body has a `Call` instruction referencing the import
//!    index (NOT a module-local fn index).
//! 5. `extern_js_unused_still_imported` — declare-but-don't-call: the
//!    import is still emitted (defensive — some users wire externs as
//!    runtime feature-flags toggled via JS, where the export presence
//!    matters even when Mighty never calls them).
//! 6. `extern_js_underscore_prefix_works` — the leading `_` is
//!    preserved verbatim in the import name AND the fn stays out of
//!    the WIT world's export list (it's an import, not an export).
//!
//! A seventh test (`example_15_extern_js_compiles_with_imports`)
//! pipelines example 15 (`examples/15_extern_js.mty`) end-to-end and
//! asserts the resulting wasm carries the `_alert` import — the
//! canonical surface the v0.24 Track E gap notes called out.
//!
//! See `dev/history/notes/EXTERN_JS_IMPORTS_V0_25_NOTES.md` for the
//! design discussion + investigation trail.

mod common;

use mty_ast::AstNode;
use mty_codegen_wasm::emit::compile_program_to_bytes;
use mty_codegen_wasm::target::WasmTarget;
use mty_codegen_wasm::wit::emit_wit;
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, ExternBinding, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
    LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
};
use mty_types::IntKind;
use wasmparser::{Imports, Operator, Parser, Payload, TypeRef};

/// Build a `Program` containing a `main` fn that does nothing AND a
/// synthesized "extern js" fn with the given name + signature. The
/// extern fn is marked via `prog.extern_bindings` so the wasm emitter
/// recognises it and turns it into an import (v0.25 Track B).
fn program_with_extern_js(
    fn_name: &str,
    param_tys: &[IrTy],
    ret_ty: IrTy,
    call_after_decl: bool,
) -> Program {
    let mut p = Program::default();

    // ---- extern fn ----------------------------------------------------
    let extern_id = IrFnId(0);
    let mut locals = vec![LocalDecl {
        name: "_0".into(),
        ty: ret_ty.clone(),
        mutable: false,
        source: LocalSource::Return,
    }];
    let mut params: Vec<Local> = Vec::with_capacity(param_tys.len());
    for (i, t) in param_tys.iter().enumerate() {
        let local_idx = (i + 1) as u32;
        locals.push(LocalDecl {
            name: format!("p{i}"),
            ty: t.clone(),
            mutable: false,
            source: LocalSource::Param,
        });
        params.push(Local(local_idx));
    }
    p.fns.push(Function {
        id: extern_id,
        name: fn_name.into(),
        params,
        locals,
        // Body left empty — the wasm emitter skips body emission for
        // extern-js fns (the `fn_index` entry already points at the
        // import slot).
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![],
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });
    p.extern_bindings.insert(
        extern_id,
        ExternBinding {
            abi: "js".into(),
            name: fn_name.into(),
        },
    );

    // ---- main fn ------------------------------------------------------
    let main_stmts = if call_after_decl {
        // _foo() — discarding the result.
        vec![Stmt::Assign(
            Place::local(Local(1)),
            Rvalue::Call {
                func: FnRef::User(extern_id),
                args: vec![],
            },
        )]
    } else {
        vec![]
    };
    let main_locals = if call_after_decl {
        vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Unit,
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "_sink".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: true,
                source: LocalSource::Temp,
            },
        ]
    } else {
        vec![LocalDecl {
            name: "_0".into(),
            ty: IrTy::Unit,
            mutable: false,
            source: LocalSource::Return,
        }]
    };
    p.fns.push(Function {
        id: IrFnId(1),
        name: "main".into(),
        params: vec![],
        locals: main_locals,
        blocks: vec![Block {
            id: BlockId(0),
            stmts: main_stmts,
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

/// Walk the wasm core module and return every `(module, name)` pair
/// in the import section that targets a Function. The order matches
/// the import-section order, which is the order the emitter wrote
/// them (so we can also infer the import indices: 0..N).
///
/// `wasmparser`'s import reader hands back `Imports<'_>` groups: a
/// single import OR a packed compact group sharing a module name.
/// We flatten both shapes; for compact groups every entry is a
/// function (the encoder only packs same-kind imports together).
fn imported_funcs(wasm: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ImportSection(rdr) = payload.expect("parse") {
            for group in rdr {
                match group.expect("import group") {
                    Imports::Single(_, imp) => {
                        if matches!(imp.ty, TypeRef::Func(_)) {
                            out.push((imp.module.to_string(), imp.name.to_string()));
                        }
                    }
                    Imports::Compact1 { module, items } => {
                        for it in items {
                            let it = it.expect("compact1 item");
                            if matches!(it.ty, TypeRef::Func(_)) {
                                out.push((module.to_string(), it.name.to_string()));
                            }
                        }
                    }
                    Imports::Compact2 { module, ty, names } => {
                        if matches!(ty, TypeRef::Func(_)) {
                            for n in names {
                                let n = n.expect("compact2 name");
                                out.push((module.to_string(), n.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Walk the wasm core module and return every `Call(idx)` instruction
/// found in any function body. Used to verify the call dispatch ended
/// up referencing the right import index.
fn all_call_targets(wasm: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("parse") {
            let mut reader = body.get_operators_reader().expect("ops reader");
            while !reader.eof() {
                if let Ok(Operator::Call { function_index }) = reader.read() {
                    out.push(function_index);
                }
            }
        }
    }
    out
}

/// Walk the wasm core module and return the (params, results) of
/// every function type in the type section, in declaration order. We
/// match against the import's `TypeRef::Func(idx)` to recover the
/// import signature (since `wasmparser`'s `Import` only carries the
/// type index, not the resolved sig).
fn type_section_sigs(wasm: &[u8]) -> Vec<(Vec<wasmparser::ValType>, Vec<wasmparser::ValType>)> {
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::TypeSection(rdr) = payload.expect("parse") {
            for ty in rdr {
                let rec_group = ty.expect("type");
                for sub in rec_group.types() {
                    if let wasmparser::CompositeInnerType::Func(ft) = &sub.composite_type.inner {
                        out.push((ft.params().to_vec(), ft.results().to_vec()));
                    }
                }
            }
        }
    }
    out
}

/// Walk imports + recover signatures via the type section. Returns a
/// vec of (module, name, params, results) for every imported Function.
fn imports_with_sigs(
    wasm: &[u8],
) -> Vec<(
    String,
    String,
    Vec<wasmparser::ValType>,
    Vec<wasmparser::ValType>,
)> {
    let sigs = type_section_sigs(wasm);
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ImportSection(rdr) = payload.expect("parse") {
            for group in rdr {
                match group.expect("import group") {
                    Imports::Single(_, imp) => {
                        if let TypeRef::Func(idx) = imp.ty {
                            let (params, results) = sigs[idx as usize].clone();
                            out.push((
                                imp.module.to_string(),
                                imp.name.to_string(),
                                params,
                                results,
                            ));
                        }
                    }
                    Imports::Compact1 { module, items } => {
                        for it in items {
                            let it = it.expect("compact1 item");
                            if let TypeRef::Func(idx) = it.ty {
                                let (params, results) = sigs[idx as usize].clone();
                                out.push((
                                    module.to_string(),
                                    it.name.to_string(),
                                    params,
                                    results,
                                ));
                            }
                        }
                    }
                    Imports::Compact2 { module, ty, names } => {
                        if let TypeRef::Func(idx) = ty {
                            let (params, results) = sigs[idx as usize].clone();
                            for n in names {
                                let n = n.expect("compact2 name");
                                out.push((
                                    module.to_string(),
                                    n.to_string(),
                                    params.clone(),
                                    results.clone(),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn extern_js_fn_emits_import() {
    // Minimal case: `extern js { fn _foo() }` on the Web target.
    let prog = program_with_extern_js("_foo", &[], IrTy::Unit, false);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");

    // Validate the core module first — a malformed import section
    // would trip the validator before we even reach the assertion.
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    assert!(
        imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == "_foo"),
        "expected `mty:web/js#_foo` import; got {imports:?}",
    );
}

#[test]
fn extern_js_fn_with_args() {
    // `extern js { fn _bar(x: I32, y: I32) }`. The wasm import should
    // be `(param i32 i32) (result)` — matches the SIR-side param
    // lowering for scalar ints.
    let prog = program_with_extern_js(
        "_bar",
        &[IrTy::Int(IntKind::I32), IrTy::Int(IntKind::I32)],
        IrTy::Unit,
        false,
    );
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imports_with_sigs(&wasm);
    let bar = imports
        .iter()
        .find(|(m, n, _, _)| m == "mty:web/js" && n == "_bar")
        .unwrap_or_else(|| panic!("missing _bar import; got {imports:?}"));
    assert_eq!(
        bar.2,
        vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
        "expected (i32, i32) params",
    );
    assert!(bar.3.is_empty(), "expected void return; got {:?}", bar.3);
}

#[test]
fn extern_js_fn_with_return() {
    // `extern js { fn _len(s: Str) -> U32 }`. The Str param expands to
    // (ptr: i32, len: i32) per the canonical-ABI flat layout. The
    // U32 return lowers to `i32`.
    let prog = program_with_extern_js("_len", &[IrTy::Str], IrTy::Int(IntKind::U32), false);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imports_with_sigs(&wasm);
    let len = imports
        .iter()
        .find(|(m, n, _, _)| m == "mty:web/js" && n == "_len")
        .unwrap_or_else(|| panic!("missing _len import; got {imports:?}"));
    assert_eq!(
        len.2,
        vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
        "Str param should expand to (ptr:i32, len:i32); got {:?}",
        len.2,
    );
    assert_eq!(
        len.3,
        vec![wasmparser::ValType::I32],
        "U32 return should be i32; got {:?}",
        len.3,
    );
}

#[test]
fn extern_js_call_routes_to_import() {
    // Build a program where `main` actually calls the extern-js fn.
    // The wasm body should contain a `Call(idx)` where `idx` matches
    // the import's function index (which is its position in the
    // imports-as-functions table — for a Web build that's after the
    // log + dom + canvas + js imports declared up-front).
    let prog = program_with_extern_js("_foo", &[], IrTy::Unit, true);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    // Find the index of `_foo` in the imports list — that's its
    // function-index in the wasm core's combined imports+funcs space.
    let foo_idx = imports
        .iter()
        .position(|(m, n)| m == "mty:web/js" && n == "_foo")
        .unwrap_or_else(|| panic!("missing _foo import; got {imports:?}")) as u32;

    let calls = all_call_targets(&wasm);
    assert!(
        calls.contains(&foo_idx),
        "expected a Call referencing import idx {foo_idx}; calls = {calls:?}",
    );
}

#[test]
fn extern_js_unused_still_imported() {
    // Declare-but-don't-call. Some users wire extern-js bindings as
    // feature-flag hooks the JS side toggles independently; the
    // import must be emitted regardless of whether Mighty calls it.
    let prog = program_with_extern_js("_feature_flag", &[], IrTy::Unit, false);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    assert!(
        imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == "_feature_flag"),
        "unused extern js fn should still be imported; got {imports:?}",
    );

    // And the WIT doc generated for the same program should mention
    // it under `mty:web/js` so wit-component can resolve the binding.
    let doc = emit_wit(&prog, "demo", WasmTarget::Web).expect("emit-wit");
    assert!(
        doc.text.contains("import mty:web/js"),
        "WIT world should import mty:web/js when an extern js fn is \
         declared; text:\n{}",
        doc.text,
    );
    assert!(
        doc.text.contains("feature-flag: func()"),
        "WIT stub should list the kebab-cased fn entry; text:\n{}",
        doc.text,
    );
}

#[test]
fn extern_js_underscore_prefix_works() {
    // The leading `_` is the Mighty convention that keeps the fn out
    // of the WIT world's *export* list (`is_exportable_fn`). For
    // *imports* the underscore must be preserved verbatim because the
    // JS shim's binding-table key is the raw name.
    let prog = program_with_extern_js("_alert", &[IrTy::Str], IrTy::Unit, false);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    assert!(
        imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == "_alert"),
        "leading underscore must be preserved in import name; got {imports:?}",
    );
    // And the world should NOT export `_alert` (would re-introduce
    // the v0.24 "private fn in export list" bug fixed by
    // `is_exportable_fn`).
    let doc = emit_wit(&prog, "demo", WasmTarget::Web).expect("emit-wit");
    assert!(
        !doc.text.contains("export alert: func"),
        "underscore-prefixed extern must NOT appear as a world export; \
         text:\n{}",
        doc.text,
    );
    assert!(
        !doc.text.contains("export _alert"),
        "extern js fn must never be exported; text:\n{}",
        doc.text,
    );
}

#[test]
fn example_15_extern_js_compiles_with_imports() {
    // Pipeline example 15 (`examples/15_extern_js.mty`) end-to-end and
    // assert the resulting wasm carries the `_alert` import — the
    // canonical surface called out by the v0.24 Track E gap notes.
    //
    // This test goes through the *full* compiler frontend so a future
    // regression in any of the upstream layers (parser → hir → typeck
    // → IR-lowering → wasm-codegen) trips here instead of silently
    // dropping the import.
    use std::path::PathBuf;

    let example_path: PathBuf = {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // crates/mty-codegen-wasm → repo root → examples/15_extern_js.mty
        p.pop();
        p.pop();
        p.push("examples");
        p.push("15_extern_js.mty");
        p
    };
    let src = std::fs::read_to_string(&example_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", example_path.display()));

    // Frontend pipeline: parse → HIR → typeck → IR.
    let parsed = mty_syntax::parse(&src);
    assert!(
        parsed.errors.is_empty(),
        "parse errors on example 15: {:?}",
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>(),
    );
    let file =
        mty_ast::File::cast(mty_syntax::SyntaxNode::new_root(parsed.green)).expect("FILE root");
    let (pkg, _lower_diags) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);

    // Sanity: the lowerer must have surfaced the `_alert` extern-js
    // binding. If this fails, the IR-level wiring regressed and the
    // wasm emitter would never see the binding.
    assert!(
        prog.extern_bindings
            .values()
            .any(|b| b.abi == "js" && b.name == "_alert"),
        "IR lowerer did not record `_alert` as an extern-js binding; \
         got {:?}",
        prog.extern_bindings,
    );

    let wasm = mty_codegen_wasm::emit::compile_program_to_bytes_with_preview(
        &prog,
        WasmTarget::Web,
        mty_codegen_wasm::emit::EmitWasiPreview::P2,
    )
    .expect("compile example 15");

    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    assert!(
        imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == "_alert"),
        "example 15's wasm must import `mty:web/js#_alert`; got {imports:?}",
    );
}
