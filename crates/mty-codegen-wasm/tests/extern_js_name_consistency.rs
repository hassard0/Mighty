//! v0.26 Track D — extern-js name consistency through `wrap_as_component`.
//!
//! v0.25 Track B landed real `(import "mty:web/js" "<name>" ...)`
//! emission for `extern js { fn _foo() }` decls, but the wasm-side
//! import name and the WIT-side identifier disagreed:
//!
//! * `crates/mty-codegen-wasm/src/emit.rs::predeclare_extern_js_imports`
//!   used the verbatim source name (`_alert`),
//! * `crates/mty-codegen-wasm/src/wit.rs::emit_extern_js_interface`
//!   ran the same name through `kebab()` (stripping the leading `_`
//!   to produce `alert`).
//!
//! The wasm core module then carried `(import "mty:web/js" "_alert" ...)`
//! but the WIT stub declared `interface js { alert: func(...) }`.
//! `wit-component::wrap_as_component` resolves wasm imports via
//! `wit_parser::Resolve::wasm_import_name` (canonical WIT name); the
//! mismatch tripped the encoder with
//! `failed to resolve import "mty:web/js::_alert"`.
//!
//! v0.26 Track D pivots both sides to the canonical kebab form via
//! `crate::wit::extern_js_canonical_name(...)`. The pivot keeps the
//! leading `_` as a Mighty-source convention (it still keeps the fn
//! out of the world's export list per `is_exportable_fn`) while
//! aligning the wasm and WIT identifiers byte-for-byte.
//!
//! Tests below pin:
//!
//! 1. `example_15_extern_js_compiles_to_component` — the full
//!    Mighty-source → wasm-component pipeline succeeds. Pre-fix this
//!    panicked at `wrap_as_component` time.
//! 2. `extern_js_underscore_name_canonical_in_wit_and_wasm` — both
//!    the generated WIT text AND the wasm import name use the
//!    canonical (kebab) form, and they agree.
//! 3. `extern_js_call_routes_to_canonical_import` — the SIR-side
//!    `Call(...)` instruction resolves to the import slot named with
//!    the canonical identifier (i.e. the dispatch path doesn't
//!    silently route to a wrong index).
//! 4. `extern_js_multiple_fns_all_kebab_consistent` — multiple
//!    extern-js decls in a single program all canonicalise + agree
//!    pairwise (no per-fn drift).
//! 5. `extern_js_canonical_name_helper_round_trips` — sanity-test
//!    the public helper `extern_js_canonical_name` directly against
//!    a small fixture sweep.

use mty_ast::AstNode;
use mty_codegen_wasm::component::wrap_as_component;
use mty_codegen_wasm::emit::{compile_program_to_bytes, compile_program_to_bytes_with_preview};
use mty_codegen_wasm::target::WasmTarget;
use mty_codegen_wasm::wit::{emit_wit, extern_js_canonical_name};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, Const, ExternBinding, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
    LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
};
use mty_types::IntKind;
use wasmparser::{Imports, Operator, Parser, Payload, TypeRef};

/// Mirrors the helper in `extern_js_imports.rs::program_with_extern_js`
/// but kept private here so the two test files stay independent.
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

/// Walk the core wasm and return every `(module, name)` pair in the
/// import section that resolves to a Function.
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

/// Every `Call(idx)` instruction in every function body, in program order.
fn all_call_targets(wasm: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("parse") {
            let mut rdr = body.get_operators_reader().expect("ops");
            while !rdr.eof() {
                if let Operator::Call { function_index } = rdr.read().expect("op") {
                    out.push(function_index);
                }
            }
        }
    }
    out
}

#[test]
fn example_15_extern_js_compiles_to_component() {
    // The headline gap: load `examples/15_extern_js.mty`, run the full
    // frontend → IR → core-wasm → `wrap_as_component` pipeline. Before
    // v0.26 Track D this panicked with `failed to resolve import
    // "mty:web/js::_alert"` because the WIT-side `alert` (kebab) and
    // the wasm-side `_alert` (verbatim) didn't match.
    use std::path::PathBuf;

    let example_path: PathBuf = {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p.push("examples");
        p.push("15_extern_js.mty");
        p
    };
    let src = std::fs::read_to_string(&example_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", example_path.display()));

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

    // Core module + WIT.
    let core = compile_program_to_bytes_with_preview(
        &prog,
        WasmTarget::Web,
        mty_codegen_wasm::emit::EmitWasiPreview::P2,
    )
    .expect("compile example 15 core");
    let doc = emit_wit(&prog, "extern_js", WasmTarget::Web).expect("emit wit");

    // The headline: `wrap_as_component` must succeed.
    let component = wrap_as_component(&core, &doc).unwrap_or_else(|e| {
        panic!(
            "wrap_as_component failed (the v0.25 → v0.26 regression we \
             closed): {e:?}\n\nWIT text:\n{}",
            doc.text,
        )
    });

    // And the resulting component should pass the Component-Model
    // validator (any structural drift would surface here too).
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&component)
        .expect("component validates end-to-end");
}

#[test]
fn extern_js_underscore_name_canonical_in_wit_and_wasm() {
    // Both sides must produce the same canonical name. We exercise the
    // canonical underscore-prefixed shape (`_alert`) since that's the
    // exact drift `wrap_as_component` rejected pre-fix.
    let prog = program_with_extern_js("_alert", &[IrTy::Str], IrTy::Unit, false);
    let canonical = extern_js_canonical_name("_alert");
    assert_eq!(
        canonical, "alert",
        "canonical helper must strip the leading underscore (got {canonical:?})",
    );

    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    let imports = imported_funcs(&wasm);
    assert!(
        imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == &canonical),
        "wasm import name must match the canonical helper output \
         ({canonical:?}); got {imports:?}",
    );
    assert!(
        !imports
            .iter()
            .any(|(m, n)| m == "mty:web/js" && n == "_alert"),
        "verbatim `_alert` MUST NOT leak through (v0.25 Track F drift); \
         got {imports:?}",
    );

    let doc = emit_wit(&prog, "demo", WasmTarget::Web).expect("emit wit");
    assert!(
        doc.text.contains(&format!("{canonical}: func")),
        "WIT stub must declare the fn under the canonical name \
         ({canonical:?}); text:\n{}",
        doc.text,
    );
    // The two sides agreeing is what `wrap_as_component` checks.
    let component = wrap_as_component(&wasm, &doc).unwrap_or_else(|e| {
        panic!(
            "wrap_as_component failed; wit and wasm names must \
             agree.\nwit:\n{}\nerr: {e:?}",
            doc.text,
        )
    });
    assert!(
        component.starts_with(b"\0asm"),
        "component bytes should still start with asm preamble",
    );
}

#[test]
fn extern_js_call_routes_to_canonical_import() {
    // The dispatch index must point to the import declared under the
    // canonical name — no off-by-one from the verbatim-vs-canonical
    // pivot.
    let prog = program_with_extern_js("_foo", &[], IrTy::Unit, true);
    let wasm = compile_program_to_bytes(&prog, WasmTarget::Web).expect("compile");
    wasmparser::Validator::new()
        .validate_all(&wasm)
        .expect("valid wasm");

    let imports = imported_funcs(&wasm);
    let foo_idx = imports
        .iter()
        .position(|(m, n)| m == "mty:web/js" && n == "foo")
        .unwrap_or_else(|| panic!("missing canonical `mty:web/js#foo` import; got {imports:?}"))
        as u32;
    let calls = all_call_targets(&wasm);
    assert!(
        calls.contains(&foo_idx),
        "wasm body should `Call({foo_idx})` (the canonical-import slot); \
         calls = {calls:?}",
    );
}

#[test]
fn extern_js_multiple_fns_all_kebab_consistent() {
    // Three extern fns with varying shapes — each must canonicalise
    // independently AND each must satisfy `wrap_as_component`.
    let mut p = Program::default();
    // Three extern fns.
    let cases = [
        ("_alert", IrTy::Unit),
        ("_logMessage", IrTy::Unit),
        ("_get_score", IrTy::Int(IntKind::U32)),
    ];
    for (i, (name, ret)) in cases.iter().enumerate() {
        let fid = IrFnId(i as u32);
        p.fns.push(Function {
            id: fid,
            name: (*name).into(),
            params: vec![],
            locals: vec![LocalDecl {
                name: "_0".into(),
                ty: ret.clone(),
                mutable: false,
                source: LocalSource::Return,
            }],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![],
                terminator: Term::Return(Operand::Const(Const::Unit)),
            }],
            entry: BlockId(0),
            ret_ty: ret.clone(),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        });
        p.extern_bindings.insert(
            fid,
            ExternBinding {
                abi: "js".into(),
                name: (*name).into(),
            },
        );
    }
    // main fn at the end so the rest of the pipeline has an entry.
    let main_id = IrFnId(cases.len() as u32);
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
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    });

    let wasm = compile_program_to_bytes(&p, WasmTarget::Web).expect("compile");
    let doc = emit_wit(&p, "multi", WasmTarget::Web).expect("emit wit");
    let imports = imported_funcs(&wasm);

    for (name, _) in cases.iter() {
        let canonical = extern_js_canonical_name(name);
        assert!(
            imports
                .iter()
                .any(|(m, n)| m == "mty:web/js" && n == &canonical),
            "missing canonical import for {name:?} → {canonical:?}; \
             got {imports:?}",
        );
        assert!(
            doc.text.contains(&format!("{canonical}: func")),
            "WIT stub missing canonical entry for {name:?} → {canonical:?}; \
             wit:\n{}",
            doc.text,
        );
    }

    // And the full pipeline succeeds.
    let _component = wrap_as_component(&wasm, &doc)
        .unwrap_or_else(|e| panic!("wrap_as_component failed: {e:?}\nwit:\n{}", doc.text));
}

#[test]
fn extern_js_canonical_name_helper_round_trips() {
    // Sanity-test the helper directly. The contract: any
    // representative source name should canonicalise to a WIT-legal
    // kebab identifier with no leading `_`. (`wit_parser` rejects
    // bare leading `_` per the v0.26 Track D investigation logged in
    // `extern_js_canonical_name`'s doc comment.)
    let cases = [
        ("_alert", "alert"),
        ("_log_message", "log-message"),
        ("_getScore", "get-score"),
        ("alert", "alert"),
        ("snake_name", "snake-name"),
        // Hyphens in the source aren't legal Mighty identifiers, but
        // the `kebab()` helper drops them as it does any other ASCII
        // punctuation — pinning the actual behaviour rather than the
        // ideal.
        ("alreadyKebab", "already-kebab"),
    ];
    for (src, want) in cases {
        let got = extern_js_canonical_name(src);
        assert_eq!(
            got, want,
            "extern_js_canonical_name({src:?}) — wanted {want:?}, got {got:?}",
        );
        assert!(
            !got.starts_with('_'),
            "canonical name must NOT start with underscore (got {got:?})",
        );
    }
}
