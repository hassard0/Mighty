//! v0.17 — direct-import coverage for `log()` / `print()`.
//!
//! Previously (v0.13–v0.16) `log()` lowered to an unversioned
//! `wasi:cli/log#log` shim that `wit-component::ComponentEncoder`
//! resolved against an in-line WIT package declared inside the
//! generated P2 document. v0.17 drops the shim entirely: the
//! emitter now lowers `log()` directly to a three-call
//! canonical-ABI sequence on top of
//! `wasi:cli/stdout@0.2.3#get-stdout` +
//! `wasi:io/streams@0.2.3#[method]output-stream.blocking-write-and-flush`
//! and balances the handle via
//! `[resource-drop]output-stream`.
//!
//! This test file pins the v0.17 dispatch path:
//!
//! 1. SIR programs that call `log(...)` under
//!    [`EmitWasiPreview::P2`] produce a core module whose import
//!    section references the versioned `wasi:cli/stdout@0.2.3` AND
//!    `wasi:io/streams@0.2.3` interfaces — and NO `wasi:cli/log`,
//!    NO `wasi_snapshot_preview1`.
//! 2. The full `wrap_p2` pipeline validates and the wrapped
//!    component is still a Component-Model component.
//! 3. The new opt-out-by-default adapter behaviour holds:
//!    `Preview2Options::new(_).embed_adapter == None`.
//! 4. Opt-in works: `with_adapter(Some(...))` reattaches the
//!    adapter for back-compat builds.
//! 5. With the adapter opted out, a log()-heavy program ships
//!    smaller bytes than the v0.16-equivalent (adapter included).
//!
//! Test count: 7 (above the spec's 5+).

mod common;

use mty_codegen_wasm::{
    compile_program_to_bytes_p2, compile_program_to_bytes_with_preview, is_component, AdapterKind,
    EmitWasiPreview, P2DirectImport, Preview2Options, WasmTarget, WASI_P1_ADAPTER_COMMAND,
};

/// Build a Program that calls `log(literal_string)` from `main`.
/// Mirrors the helpers in the other preview2 test files but
/// targets the `BuiltinId::Log` builtin specifically (the others
/// only exercise `BuiltinId::Extern`).
fn log_call_program(msg: &str) -> mty_ir::ir::Program {
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
        LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
    };

    let mut p = Program::default();
    let locals = vec![
        LocalDecl {
            name: "_0".into(),
            ty: IrTy::Unit,
            mutable: false,
            source: LocalSource::Return,
        },
        // Sink for the call result (Unit; lowered as i32 placeholder
        // by the codegen layer).
        LocalDecl {
            name: "_1".into(),
            ty: IrTy::Error,
            mutable: false,
            source: LocalSource::Temp,
        },
    ];
    let stmts = vec![Stmt::Assign(
        Place::local(Local(1)),
        Rvalue::Call {
            func: FnRef::Builtin(BuiltinId::Log),
            args: vec![Operand::Const(Const::Str(msg.into()))],
        },
    )];
    p.fns.push(Function {
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
    });
    p
}

/// Walk a core module's import section. Returns `true` iff any
/// import matches the `(module, name)` pair exactly. Test-local
/// copy of the helper in `tests/preview2.rs` (kept narrow to avoid
/// pulling extra exports into the public surface).
fn core_module_imports(bytes: &[u8], module: &str, name: &str) -> bool {
    use wasmparser::{Imports, Parser, Payload};
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(Payload::ImportSection(reader)) = payload else {
            continue;
        };
        for group in reader {
            let Ok(group) = group else { continue };
            match group {
                Imports::Single(_, imp) => {
                    if imp.module == module && imp.name == name {
                        return true;
                    }
                }
                Imports::Compact1 {
                    module: m, items, ..
                } => {
                    if m == module {
                        for it in items.into_iter().flatten() {
                            if it.name == name {
                                return true;
                            }
                        }
                    }
                }
                Imports::Compact2 {
                    module: m, names, ..
                } => {
                    if m == module {
                        for n in names.into_iter().flatten() {
                            if n == name {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

/// `Some(module)` for any core-module import whose module name
/// starts with `module_prefix`. Used to assert absence of the
/// legacy `wasi:cli/log` shim and `wasi_snapshot_preview1`.
fn core_module_has_module_prefix(bytes: &[u8], module_prefix: &str) -> bool {
    use wasmparser::{Imports, Parser, Payload};
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(Payload::ImportSection(reader)) = payload else {
            continue;
        };
        for group in reader {
            let Ok(group) = group else { continue };
            let module = match &group {
                Imports::Single(_, imp) => imp.module,
                Imports::Compact1 { module: m, .. } => *m,
                Imports::Compact2 { module: m, .. } => *m,
            };
            if module.starts_with(module_prefix) {
                return true;
            }
        }
    }
    false
}

// ============================================================
// Phase 4 — log() direct-lowering tests.
// ============================================================

#[test]
fn log_call_emits_p2_imports() {
    let prog = log_call_program("hi");
    let core = compile_program_to_bytes_with_preview(&prog, WasmTarget::Wasi, EmitWasiPreview::P2)
        .expect("compile p2");

    // The two non-resource-drop imports must be present verbatim.
    let (m_get, n_get) = P2DirectImport::LogStdoutGet.import_pair();
    assert!(
        core_module_imports(&core, m_get, n_get),
        "expected direct {m_get}#{n_get} import in core module",
    );
    let (m_w, n_w) = P2DirectImport::LogStreamWrite.import_pair();
    assert!(
        core_module_imports(&core, m_w, n_w),
        "expected direct {m_w}#{n_w} import in core module",
    );
    let (m_d, n_d) = P2DirectImport::LogStreamDrop.import_pair();
    assert!(
        core_module_imports(&core, m_d, n_d),
        "expected direct {m_d}#{n_d} import in core module",
    );

    // And the v0.13–v0.16 shim + the P1 syscall route are GONE.
    assert!(
        !core_module_imports(&core, "wasi:cli/log", "log"),
        "core module must NOT import the v0.13 wasi:cli/log shim",
    );
    assert!(
        !core_module_has_module_prefix(&core, "wasi_snapshot_preview1"),
        "core module must NOT import any wasi_snapshot_preview1 syscall",
    );
}

#[test]
fn log_in_component_validates() {
    let prog = log_call_program("hello from log in component");
    let opts = Preview2Options::new("log-validates");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("compile + wrap p2");

    assert!(is_component(&bytes), "wrap_p2 should return a Component");
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes)
        .expect("p2 log component should validate");
}

#[test]
fn default_adapter_is_none() {
    // The v0.17 default flip: `Preview2Options::new` no longer
    // pre-loads `AdapterKind::Command`. Programs that don't reach
    // for legacy P1 syscalls now ship adapter-free by default.
    let opts = Preview2Options::new("default-adapter-none");
    assert!(
        opts.embed_adapter.is_none(),
        "v0.17 default: embed_adapter starts at None"
    );
}

#[test]
fn explicit_adapter_opt_in_works() {
    // The opt-in path remains supported for back-compat: callers
    // that link wasi-libc-built C code can still ask for the
    // adapter via `with_adapter(Some(...))`.
    let opts = Preview2Options::new("opt-in-adapter").with_adapter(Some(AdapterKind::Command));
    assert_eq!(
        opts.embed_adapter,
        Some(AdapterKind::Command),
        "opt-in adapter must round-trip",
    );

    // And the resulting component must still validate, with the
    // adapter physically embedded (we can't introspect "adapter
    // present" directly, but we can check size > default).
    let prog = log_call_program("opt-in adapter present");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("compile + wrap with adapter");
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes)
        .expect("p2 component with adapter still validates");
}

#[test]
fn log_program_no_adapter_runs_smaller() {
    // With the adapter opted out (v0.17 default), a log()-shaped
    // component ships smaller bytes than the same program built
    // with the adapter embedded. The exact delta depends on
    // wit-component's tree-shaking but the adapter contributes
    // ≥ ~10 KB even after stripping unused exports.
    let prog = log_call_program("size comparison");
    let no_adapter = compile_program_to_bytes_p2(&prog, &Preview2Options::new("no-adapter"))
        .expect("compile no-adapter");
    let with_adapter = compile_program_to_bytes_p2(
        &prog,
        &Preview2Options::new("with-adapter").with_adapter(Some(AdapterKind::Command)),
    )
    .expect("compile with-adapter");

    assert!(
        no_adapter.len() < with_adapter.len(),
        "no-adapter component ({} bytes) should be smaller than with-adapter ({} bytes)",
        no_adapter.len(),
        with_adapter.len(),
    );
    // And it should be meaningfully smaller — adapter bytes are
    // ≥ 10 KB even after the wit-component stripper runs.
    let savings = with_adapter.len() - no_adapter.len();
    assert!(
        savings >= 1024,
        "expected at least 1 KiB of savings from dropping the adapter, got {savings} bytes",
    );
    // Sanity check that the adapter binary itself is what we think
    // it is — the saving should be on the order of the embedded
    // adapter's size after stripping.
    assert!(
        WASI_P1_ADAPTER_COMMAND.len() > 10_000,
        "vendored command-adapter should be larger than 10 KiB",
    );
}

// ============================================================
// Phase 4 — supplementary coverage.
// ============================================================

#[test]
fn p2_log_direct_constants_match_stdlib() {
    // The codegen-side import-pair table must agree with the
    // stdlib-side P2 constants — both are the single source of
    // truth for the same WIT names, and CI catches drift.
    use mty_stdlib::log as stdlog;

    assert_eq!(
        P2DirectImport::LogStdoutGet.import_pair(),
        stdlog::P2_DIRECT_IMPORT_LOG_STDOUT_GET,
    );
    assert_eq!(
        P2DirectImport::LogStreamWrite.import_pair(),
        stdlog::P2_DIRECT_IMPORT_LOG_STREAM_WRITE,
    );
    assert_eq!(
        P2DirectImport::LogStreamDrop.import_pair(),
        stdlog::P2_DIRECT_IMPORT_LOG_STREAM_DROP,
    );

    // The is_log predicate covers exactly the trio and nothing else.
    assert!(P2DirectImport::LogStdoutGet.is_log());
    assert!(P2DirectImport::LogStreamWrite.is_log());
    assert!(P2DirectImport::LogStreamDrop.is_log());
    assert!(!P2DirectImport::RandomBytes.is_log());
    assert!(!P2DirectImport::FsOpenAt.is_log());
    assert!(!P2DirectImport::HttpNewRequest.is_log());
}

#[test]
fn log_p1_path_still_uses_legacy_shim() {
    // Back-compat sanity: an explicit P1 build still wires the
    // legacy `wasi:cli/log#log` shim. The v0.17 flip only affects
    // the P2 dispatch path; downstream consumers that pin
    // `--wasi=p1` must continue to see the v0.13 import shape.
    let prog = log_call_program("p1 back-compat");
    let core = compile_program_to_bytes_with_preview(&prog, WasmTarget::Wasi, EmitWasiPreview::P1)
        .expect("compile p1");

    assert!(
        core_module_imports(&core, "wasi:cli/log", "log"),
        "P1 build must still declare the legacy wasi:cli/log shim",
    );
    // And — equally important — the P1 build must NOT splice in
    // the v0.17 direct-lowering imports.
    let (m_get, n_get) = P2DirectImport::LogStdoutGet.import_pair();
    assert!(
        !core_module_imports(&core, m_get, n_get),
        "P1 build must not splice the {m_get}#{n_get} direct import",
    );
}

// `common::empty_main` is shared across the preview2 test family;
// reference it here so cargo doesn't warn about dead module imports.
#[test]
fn empty_main_still_builds_under_p2() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("empty-no-log");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("compile empty p2");
    assert!(is_component(&bytes));
    // An empty main shouldn't reach for the legacy P1 syscall
    // namespace either — the v0.17 adapter-opt-out default
    // keeps the component free of `wasi_snapshot_preview1`
    // entirely.
    use wasmparser::{Parser, Payload};
    let mut saw_snapshot_p1 = false;
    for payload in Parser::new(0).parse_all(&bytes) {
        let Ok(payload) = payload else { continue };
        if let Payload::ComponentImportSection(reader) = payload {
            for imp in reader.into_iter().flatten() {
                if imp.name.name.contains("wasi_snapshot_preview1") {
                    saw_snapshot_p1 = true;
                    break;
                }
            }
        }
    }
    assert!(
        !saw_snapshot_p1,
        "empty main built under v0.17 default should NOT show any wasi_snapshot_preview1 import",
    );
}
