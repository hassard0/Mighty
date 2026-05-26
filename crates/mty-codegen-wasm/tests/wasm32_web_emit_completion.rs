//! v0.24 Track A — emitter-completion regression suite.
//!
//! Two related fixes in `crates/mty-codegen-wasm/src/emit.rs`:
//!
//! 1. **`BuiltinId::CanvasOp(kind)` lowering** — Mighty source like
//!    `canvas.fill_rect(x, y, w, h, color)` lowers (post-Track-B) to
//!    `Rvalue::Call { func: FnRef::Builtin(BuiltinId::CanvasOp(...)) }`.
//!    The emitter must declare a matching `mty:web/canvas@0.1` import
//!    and emit a direct `call` instruction.
//!
//! 2. **`export fn` → core module export section** — agents declare
//!    `frame(dt)`, `keydown(k)`, `keyup(k)` handlers. Prior to v0.24
//!    these surfaced only in the generated WIT; the embedded core
//!    module's export section listed only `main` + `cabi_realloc` +
//!    `memory`. The JS host's `inst.exports.frame(t)` therefore
//!    trapped with "frame is not a function". This file pins the
//!    fix: the export section must now contain `frame` / `keydown` /
//!    `keyup` whenever the source declares a fn with that name.
//!
//! Tests use `wasmparser` to walk the embedded core module directly —
//! the same path the v0.23 `wasm32_web_core` harness uses — so the
//! checks survive Component-Model wrapper churn.
//!
//! See `dev/history/notes/WEB_EMIT_COMPLETION_V0_24_NOTES.md` for the
//! design background.

mod common;

use mty_codegen_wasm::{
    compile_program_to_bytes_with_preview, compile_program_to_file_with_options, BuildOptions,
    EmitWasiPreview, WasmTarget,
};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    Block, BlockId, BuiltinId, CanvasOpKind, Const, FnRef, Function, IrFnId, IrTy, Local,
    LocalDecl, LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
};
use mty_types::IntKind;

/// Canonical core-Wasm preamble: magic `\0asm` + version `0x00000001`.
const CORE_PREAMBLE: &[u8] = b"\0asm\x01\x00\x00\x00";

/// Walk the Component bytes and return the bytes of the first
/// embedded core module section (this is the user's program; the
/// wit-component adapters come AFTER in section order).
fn extract_user_core(bytes: &[u8]) -> Vec<u8> {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ModuleSection {
            unchecked_range, ..
        } = payload.expect("payload")
        {
            return bytes[unchecked_range.start..unchecked_range.end].to_vec();
        }
    }
    panic!("no embedded core module found");
}

/// Build a `wasm32-web` Component artifact for `prog` named `pkg` and
/// return the embedded user core module bytes.
fn build_web_core(prog: &Program, pkg: &str) -> Vec<u8> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join(format!("{pkg}.wasm"));
    let opts = BuildOptions::new(pkg);
    let art = compile_program_to_file_with_options(prog, WasmTarget::Web, &out, &opts)
        .expect("build wasm32-web");
    let core = extract_user_core(&art.bytes);
    assert!(
        core.starts_with(CORE_PREAMBLE),
        "extracted user core does not start with core preamble"
    );
    core
}

/// (module, name) pairs of every function import declared by `core`,
/// in declaration order. wasmparser groups imports by module so we
/// have to flatten the three possible group shapes
/// (`Imports::Single`, `Imports::Compact1`, `Imports::Compact2`).
fn extract_function_imports(core: &[u8]) -> Vec<(String, String)> {
    use wasmparser::Imports;
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(core) {
        if let wasmparser::Payload::ImportSection(reader) = payload.expect("payload") {
            for group in reader {
                match group.expect("import group") {
                    Imports::Single(_, imp) => {
                        if matches!(imp.ty, wasmparser::TypeRef::Func(_)) {
                            out.push((imp.module.to_string(), imp.name.to_string()));
                        }
                    }
                    Imports::Compact1 { module, items } => {
                        for item in items {
                            let item = item.expect("compact1 item");
                            if matches!(item.ty, wasmparser::TypeRef::Func(_)) {
                                out.push((module.to_string(), item.name.to_string()));
                            }
                        }
                    }
                    Imports::Compact2 { module, names, ty } => {
                        if matches!(ty, wasmparser::TypeRef::Func(_)) {
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

/// Names of every function export declared by `core`, in declaration
/// order.
fn extract_function_exports(core: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(core) {
        if let wasmparser::Payload::ExportSection(reader) = payload.expect("payload") {
            for entry in reader {
                let entry = entry.expect("export entry");
                if matches!(entry.kind, wasmparser::ExternalKind::Func) {
                    out.push(entry.name.to_string());
                }
            }
        }
    }
    out
}

/// Build a program containing `main` that calls every canvas op in
/// `kinds`. Each call sinks its return into a fresh local so the
/// emitter's assign-path picks it up.
fn program_calling_canvas_ops(kinds: &[CanvasOpKind]) -> Program {
    let mut p = common::empty_main();
    // Replace the `main` body with one that calls each op in turn.
    let mut locals: Vec<LocalDecl> = vec![LocalDecl {
        name: "_0".into(),
        ty: IrTy::Unit,
        mutable: false,
        source: LocalSource::Return,
    }];
    let mut stmts: Vec<Stmt> = Vec::new();
    for (i, kind) in kinds.iter().enumerate() {
        let sink_idx = (locals.len()) as u32;
        locals.push(LocalDecl {
            name: format!("_canvas_sink_{i}"),
            ty: IrTy::Int(IntKind::I32),
            mutable: false,
            source: LocalSource::Temp,
        });
        // Pick a plausible arg tuple for each op so we always exercise
        // the canonical signature; the emitter only cares that the
        // stack arity matches.
        let args: Vec<Operand> = match kind {
            CanvasOpKind::Clear | CanvasOpKind::RequestAnimationFrame => vec![],
            CanvasOpKind::FillRect | CanvasOpKind::StrokeRect => vec![
                Operand::Const(Const::Int(0, IntKind::I32)),
                Operand::Const(Const::Int(0, IntKind::I32)),
                Operand::Const(Const::Int(40, IntKind::I32)),
                Operand::Const(Const::Int(40, IntKind::I32)),
                Operand::Const(Const::Int(0x1d2230ff, IntKind::U32)),
            ],
            CanvasOpKind::FillText => vec![
                Operand::Const(Const::Str("hi".into())),
                Operand::Const(Const::Int(8, IntKind::I32)),
                Operand::Const(Const::Int(16, IntKind::I32)),
                Operand::Const(Const::Int(0xffffffff_u32 as i128, IntKind::U32)),
            ],
            CanvasOpKind::SetFillStyle => {
                vec![Operand::Const(Const::Int(0x12345678, IntKind::U32))]
            }
            CanvasOpKind::Width | CanvasOpKind::Height => vec![],
        };
        stmts.push(Stmt::Assign(
            Place::local(Local(sink_idx)),
            Rvalue::Call {
                func: FnRef::Builtin(BuiltinId::CanvasOp(*kind)),
                args,
            },
        ));
    }
    p.fns[0] = Function {
        id: IrFnId(0),
        name: "main".into(),
        params: vec![],
        locals,
        blocks: vec![Block {
            id: BlockId(0),
            stmts,
            terminator: Term::Return(Operand::Const(Const::Unit)),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Unit,
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    };
    p
}

/// Build an `export fn <name>(<param_ty>) { return }` SIR function.
/// The lone parameter takes type `param_ty`; an unparameterized
/// signature is achievable by passing `IrTy::Unit` (the wasm signature
/// is then `() -> ()`).
fn make_named_fn(name: &str, fn_id: u32, param_ty: IrTy) -> Function {
    let mut locals = vec![LocalDecl {
        name: "_0".into(),
        ty: IrTy::Unit,
        mutable: false,
        source: LocalSource::Return,
    }];
    let mut params: Vec<Local> = Vec::new();
    if !matches!(param_ty, IrTy::Unit) {
        locals.push(LocalDecl {
            name: "p".into(),
            ty: param_ty,
            mutable: false,
            source: LocalSource::Param,
        });
        params.push(Local(1));
    }
    Function {
        id: IrFnId(fn_id),
        name: name.into(),
        params,
        locals,
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
    }
}

#[test]
fn canvas_fill_rect_lowers_to_wit_import() {
    let prog = program_calling_canvas_ops(&[CanvasOpKind::FillRect]);
    let core = build_web_core(&prog, "fillrect");
    let imports = extract_function_imports(&core);
    assert!(
        imports
            .iter()
            .any(|(m, n)| (m == "mty:web/canvas" || m == "mty:web/canvas@0.1") && n == "fill-rect"),
        "fill-rect import missing — imports were: {imports:?}"
    );
}

#[test]
fn canvas_set_fill_style_lowers() {
    let prog = program_calling_canvas_ops(&[CanvasOpKind::SetFillStyle]);
    let core = build_web_core(&prog, "setfillstyle");
    let imports = extract_function_imports(&core);
    assert!(
        imports.iter().any(
            |(m, n)| (m == "mty:web/canvas" || m == "mty:web/canvas@0.1") && n == "set-fill-style"
        ),
        "set-fill-style import missing — imports were: {imports:?}"
    );
}

#[test]
fn canvas_all_ops_emit_at_least_one_import() {
    // Sweep every CanvasOpKind variant; each one used in a program
    // must show up as a function import on the embedded core module.
    for kind in CanvasOpKind::all() {
        let prog = program_calling_canvas_ops(&[*kind]);
        let core = build_web_core(&prog, "sweep");
        let imports = extract_function_imports(&core);
        let expected_method = kind.as_wit_method();
        assert!(
            imports.iter().any(
                |(m, n)| (m == "mty:web/canvas" || m == "mty:web/canvas@0.1")
                    && n == expected_method
            ),
            "canvas op {kind:?} ({expected_method}) missing from imports — \
             imports were: {imports:?}"
        );
    }
}

#[test]
fn export_fn_frame_in_core_exports() {
    // SIR program with `main` + `frame(dt: U32) { }`; the embedded core
    // module's export section must contain `frame` as a fn export.
    let mut prog = common::empty_main();
    prog.fns
        .push(make_named_fn("frame", 1, IrTy::Int(IntKind::U32)));
    let core = build_web_core(&prog, "framemod");
    let exports = extract_function_exports(&core);
    assert!(
        exports.iter().any(|n| n == "frame"),
        "core module missing `frame` export — exports were: {exports:?}"
    );
}

#[test]
fn export_fn_keydown_keyup_in_core_exports() {
    let mut prog = common::empty_main();
    prog.fns
        .push(make_named_fn("keydown", 1, IrTy::Int(IntKind::U32)));
    prog.fns
        .push(make_named_fn("keyup", 2, IrTy::Int(IntKind::U32)));
    let core = build_web_core(&prog, "inputmod");
    let exports = extract_function_exports(&core);
    assert!(
        exports.iter().any(|n| n == "keydown"),
        "core module missing `keydown` export — exports were: {exports:?}"
    );
    assert!(
        exports.iter().any(|n| n == "keyup"),
        "core module missing `keyup` export — exports were: {exports:?}"
    );
}

#[test]
fn export_fn_without_use_still_exported() {
    // The exported callback fn doesn't reference any other user fn
    // (no call instructions at all); it must still surface in the
    // core export section. This is the regression that broke v0.23
    // Track D — the host's `inst.exports.frame(t)` was trapping
    // even though Mighty source contained a `fn frame(dt)`.
    let mut prog = common::empty_main();
    prog.fns
        .push(make_named_fn("frame", 1, IrTy::Int(IntKind::U32)));
    let core = build_web_core(&prog, "unused");
    let exports = extract_function_exports(&core);
    assert!(
        exports.iter().any(|n| n == "frame"),
        "`frame` must be exported even if no other user fn references it; \
         exports were: {exports:?}"
    );
}

#[test]
fn wasi_target_unchanged() {
    // The export-section change is web-only. A `wasm32-wasi` build
    // with the same SIR shape must NOT export `frame` — the WASI
    // host has no concept of an animation-frame callback.
    let mut prog = common::empty_main();
    prog.fns
        .push(make_named_fn("frame", 1, IrTy::Int(IntKind::U32)));
    let bytes = compile_program_to_bytes_with_preview(&prog, WasmTarget::Wasi, EmitWasiPreview::P1)
        .expect("wasi build");
    let exports = extract_function_exports(&bytes);
    assert!(
        !exports.iter().any(|n| n == "frame"),
        "wasi build must not export `frame`; exports were: {exports:?}"
    );
    // `main` is still there.
    assert!(
        exports.iter().any(|n| n == "main"),
        "wasi build must still export `main`; exports were: {exports:?}"
    );
}

#[test]
fn existing_main_export_preserved() {
    // Sanity: the v0.23 `wasm32_web_core` invariant — `main` is
    // always exported from the embedded core — must still hold even
    // after we layer additional callback exports on top.
    let mut prog = common::empty_main();
    prog.fns
        .push(make_named_fn("frame", 1, IrTy::Int(IntKind::U32)));
    let core = build_web_core(&prog, "preserve");
    let exports = extract_function_exports(&core);
    assert!(
        exports.iter().any(|n| n == "main"),
        "core module lost `main` export — exports were: {exports:?}"
    );
    assert!(
        exports.iter().any(|n| n == "cabi_realloc"),
        "core module lost `cabi_realloc` export — exports were: {exports:?}"
    );
}

#[test]
fn canvas_repeated_call_reuses_import() {
    // Two `fill_rect` calls in the same program must share one
    // import — the Emitter caches the fn-index in
    // `CanvasImports::fill_rect` after the first declaration.
    let prog = program_calling_canvas_ops(&[CanvasOpKind::FillRect, CanvasOpKind::FillRect]);
    let core = build_web_core(&prog, "repeat");
    let imports = extract_function_imports(&core);
    let fill_rect_count = imports
        .iter()
        .filter(|(m, n)| (m == "mty:web/canvas" || m == "mty:web/canvas@0.1") && n == "fill-rect")
        .count();
    assert_eq!(
        fill_rect_count, 1,
        "fill-rect should be declared exactly once even with multiple call sites; \
         imports were: {imports:?}"
    );
}

#[test]
fn canvas_mix_of_ops_each_get_one_import() {
    // Mixed call sequence — every distinct op gets its own import
    // line; total count matches the distinct kinds list.
    let kinds = [
        CanvasOpKind::Clear,
        CanvasOpKind::SetFillStyle,
        CanvasOpKind::FillRect,
        CanvasOpKind::Clear, // duplicate — should still only declare once
        CanvasOpKind::RequestAnimationFrame,
    ];
    let prog = program_calling_canvas_ops(&kinds);
    let core = build_web_core(&prog, "mixed");
    let imports = extract_function_imports(&core);
    let canvas_imports: Vec<&(String, String)> = imports
        .iter()
        .filter(|(m, _)| m == "mty:web/canvas" || m == "mty:web/canvas@0.1")
        .collect();
    // 4 distinct ops (Clear, SetFillStyle, FillRect, RAF) — not 5.
    assert_eq!(
        canvas_imports.len(),
        4,
        "expected 4 distinct canvas imports for the mixed call set, got: {canvas_imports:?}"
    );
    for needle in [
        "clear",
        "set-fill-style",
        "fill-rect",
        "request-animation-frame",
    ] {
        assert!(
            canvas_imports.iter().any(|(_, n)| n == needle),
            "missing {needle} from canvas imports: {canvas_imports:?}"
        );
    }
}
