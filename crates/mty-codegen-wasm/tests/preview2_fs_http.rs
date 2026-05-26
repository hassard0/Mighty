//! WASI Preview 2 direct-import coverage for `std.fs.*` + `std.http.*`.
//!
//! v0.15 wired direct P2 lowerings for `std.random` + `std.time` into
//! the core-module emitter. v0.16 extends that to filesystem and HTTP
//! — both surfaces previously had to route through the vendored
//! `wasi_snapshot_preview1` adapter because their canonical-ABI
//! shapes carry resource handles (`descriptor`, `outgoing-request`,
//! `incoming-response`, …).
//!
//! This test file pins the v0.16 dispatch path:
//!
//! 1. SIR programs that call `std.fs.{open,read_file,write_file,stat,close}`
//!    under `EmitWasiPreview::P2` produce a core module whose import
//!    section references the *versioned* P2 interface verbatim
//!    (`wasi:filesystem/types@0.2.3.[method]descriptor.*`).
//! 2. SIR programs that call `std.http.{get,post,send}` produce a
//!    core module whose import section references
//!    `wasi:http/types@0.2.3.[constructor]outgoing-request` /
//!    `wasi:http/outgoing-handler@0.2.3.handle`.
//! 3. End-to-end, an `fs.read_file`-shaped program builds into a
//!    valid Component Model component.
//!
//! Test count: 7 (above the spec's 6+).
//!
//! ## v0.17 follow-ups
//!
//! - `std.fs.read_file` currently emits only the
//!   `read-via-stream` import; the open + close scaffold around it
//!   (which would also drag in `[resource-drop]descriptor`) is
//!   deferred until the SIR layer carries the preopen-descriptor
//!   handle explicitly.
//! - `std.http.get` only splices the constructor today; the full
//!   spine (handle → status → consume) is exercised via the
//!   `std.http.send` lowering.

mod common;

use mty_codegen_wasm::{
    compile_program_to_bytes_p2, compile_program_to_bytes_with_preview, is_component,
    EmitWasiPreview, P2DirectImport, Preview2Options,
};

/// Build a Program that calls one of the std.* extern paths. Mirrors
/// the helper inside `tests/preview2.rs` (test-private over there;
/// duplicated here so this file stays self-contained — pulling the
/// helper into a shared `common.rs` would make every other consumer
/// of `common::empty_main` recompile).
fn std_call_program(extern_name: &str, pass_arg: bool) -> mty_ir::ir::Program {
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
        LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
    };
    use mty_types::IntKind;

    let mut p = Program::default();
    let mut locals = vec![
        LocalDecl {
            name: "_0".into(),
            ty: IrTy::Unit,
            mutable: false,
            source: LocalSource::Return,
        },
        LocalDecl {
            name: "_1".into(),
            ty: IrTy::Error,
            mutable: false,
            source: LocalSource::Temp,
        },
    ];
    let mut args: Vec<Operand> = Vec::new();
    if pass_arg {
        locals.push(LocalDecl {
            name: "n".into(),
            ty: IrTy::Int(IntKind::I32),
            mutable: false,
            source: LocalSource::Temp,
        });
        // Pass a string literal — fs paths + http URLs both want
        // strings, and the emitter interns Str consts into the data
        // section before pushing (ptr, len).
        args.push(Operand::Const(Const::Str("./test.txt".into())));
    }
    let stmts = vec![Stmt::Assign(
        Place::local(Local(1)),
        Rvalue::Call {
            func: FnRef::Builtin(BuiltinId::Extern(extern_name.into())),
            args,
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

/// Scan the core module's import section for an exact `(module, name)`
/// pair. Copied from `tests/preview2.rs` to keep this file
/// self-contained. Handles every wasm-encoder-produced `Imports::*`
/// shape (Single / Compact1 / Compact2).
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

// ============================================================
// v0.16 — filesystem direct-import dispatch coverage.
// ============================================================

#[test]
fn fs_open_emits_direct_p2_import() {
    let prog = std_call_program("std.fs.open", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.open-at"
        ),
        "expected versioned wasi:filesystem.open-at direct import"
    );
    assert!(
        !core_module_imports(&core, "wasi_snapshot_preview1", "path_open"),
        "core module must not fall back to wasi_snapshot_preview1 for fs.open"
    );
}

#[test]
fn fs_read_emits_direct_p2_import() {
    let prog = std_call_program("std.fs.read_file", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.read-via-stream"
        ),
        "expected versioned wasi:filesystem.read-via-stream direct import"
    );
    assert!(
        !core_module_imports(&core, "wasi_snapshot_preview1", "fd_read"),
        "core module must not fall back to wasi_snapshot_preview1 for fs.read_file"
    );
}

#[test]
fn fs_write_emits_direct_p2() {
    let prog = std_call_program("std.fs.write_file", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.write-via-stream"
        ),
        "expected versioned wasi:filesystem.write-via-stream direct import"
    );
    assert!(
        !core_module_imports(&core, "wasi_snapshot_preview1", "fd_write"),
        "core module must not fall back to wasi_snapshot_preview1 for fs.write_file"
    );
}

#[test]
fn fs_stat_emits_direct_p2() {
    let prog = std_call_program("std.fs.stat", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.stat"
        ),
        "expected versioned wasi:filesystem.stat direct import"
    );
}

#[test]
fn fs_close_emits_resource_drop_import() {
    let prog = std_call_program("std.fs.close", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[resource-drop]descriptor"
        ),
        "expected wasi:filesystem descriptor resource-drop import"
    );
}

// ============================================================
// v0.16 — http direct-import dispatch coverage.
// ============================================================

#[test]
fn http_get_emits_direct_p2_imports() {
    let prog = std_call_program("std.http.get", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    // GET lowers to the outgoing-request constructor entry point —
    // the full spine (handle → status → consume) is exercised via
    // `std.http.send`. What we pin here is that the versioned
    // wasi:http/types import lands in the import section.
    assert!(
        core_module_imports(
            &core,
            "wasi:http/types@0.2.3",
            "[constructor]outgoing-request"
        ),
        "expected versioned wasi:http outgoing-request constructor import"
    );
    assert!(
        !core_module_imports(&core, "wasi_snapshot_preview1", "sock_send"),
        "core module must not fall back to wasi_snapshot_preview1 for http.get"
    );
}

#[test]
fn http_post_emits_direct_p2() {
    let prog = std_call_program("std.http.post", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(
            &core,
            "wasi:http/types@0.2.3",
            "[constructor]outgoing-request"
        ),
        "expected versioned wasi:http outgoing-request constructor import"
    );
}

#[test]
fn http_send_emits_outgoing_handler() {
    // `std.http.send` is the lower-level entry that calls
    // `outgoing-handler.handle` directly. Pin that the versioned
    // import is spliced.
    let prog = std_call_program("std.http.send", /*pass_arg=*/ false);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(&core, "wasi:http/outgoing-handler@0.2.3", "handle"),
        "expected versioned wasi:http outgoing-handler.handle direct import"
    );
}

// ============================================================
// v0.16 — end-to-end component validation.
// ============================================================

/// A small program that calls `std.fs.read_file` must build all the
/// way through the P2 wrap path and produce a valid Component Model
/// component. This catches regressions in the canonical-ABI types we
/// declare for the splice — if the signature is wrong, `wit-component`
/// rejects the encode.
#[test]
fn fs_program_compiles_to_valid_component() {
    let prog = std_call_program("std.fs.read_file", /*pass_arg=*/ true);
    let opts = Preview2Options::new("fs-demo");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("compile p2 component");
    assert!(is_component(&bytes), "expected component preamble");

    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes).expect("component validates");
}

// ============================================================
// v0.16 — P1 back-compat (direct lowerings stay off on P1).
// ============================================================

/// Under `EmitWasiPreview::P1` the v0.16 fs direct lowering must
/// remain inactive — we'd regress back-compat with downstream
/// tooling otherwise. Mirrors the v0.15 random-bytes back-compat
/// guard.
#[test]
fn fs_read_under_p1_skips_direct_import() {
    let prog = std_call_program("std.fs.read_file", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P1,
    )
    .expect("compile p1");
    assert!(
        !core_module_imports(
            &core,
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.read-via-stream"
        ),
        "P1 build must not declare the v0.16 direct P2 import for fs.read_file"
    );
}

/// Same back-compat assertion for the http surface.
#[test]
fn http_get_under_p1_skips_direct_import() {
    let prog = std_call_program("std.http.get", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P1,
    )
    .expect("compile p1");
    assert!(
        !core_module_imports(
            &core,
            "wasi:http/types@0.2.3",
            "[constructor]outgoing-request"
        ),
        "P1 build must not declare the v0.16 direct P2 import for http.get"
    );
}

// ============================================================
// v0.16 — P2DirectImport enum coverage of the new variants.
// ============================================================

/// Pin every new variant's `(module, name)` pair so a refactor in
/// `preview2::P2DirectImport::import_pair` can't silently change
/// what the emitter splices.
#[test]
fn p2_direct_import_pairs_for_fs_http_match_spec() {
    assert_eq!(
        P2DirectImport::FsOpenAt.import_pair(),
        ("wasi:filesystem/types@0.2.3", "[method]descriptor.open-at")
    );
    assert_eq!(
        P2DirectImport::FsReadViaStream.import_pair(),
        (
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.read-via-stream"
        )
    );
    assert_eq!(
        P2DirectImport::FsWriteViaStream.import_pair(),
        (
            "wasi:filesystem/types@0.2.3",
            "[method]descriptor.write-via-stream"
        )
    );
    assert_eq!(
        P2DirectImport::FsStat.import_pair(),
        ("wasi:filesystem/types@0.2.3", "[method]descriptor.stat")
    );
    assert_eq!(
        P2DirectImport::FsClose.import_pair(),
        ("wasi:filesystem/types@0.2.3", "[resource-drop]descriptor")
    );
    assert_eq!(
        P2DirectImport::HttpNewRequest.import_pair(),
        ("wasi:http/types@0.2.3", "[constructor]outgoing-request")
    );
    assert_eq!(
        P2DirectImport::HttpHandleRequest.import_pair(),
        ("wasi:http/outgoing-handler@0.2.3", "handle")
    );
    assert_eq!(
        P2DirectImport::HttpResponseStatus.import_pair(),
        ("wasi:http/types@0.2.3", "[method]incoming-response.status")
    );
    assert_eq!(
        P2DirectImport::HttpResponseBody.import_pair(),
        ("wasi:http/types@0.2.3", "[method]incoming-response.consume")
    );
    // Predicates stay in sync with the variants.
    assert!(P2DirectImport::FsOpenAt.is_filesystem());
    assert!(P2DirectImport::FsClose.is_filesystem());
    assert!(!P2DirectImport::HttpNewRequest.is_filesystem());
    assert!(P2DirectImport::HttpNewRequest.is_http());
    assert!(P2DirectImport::HttpResponseBody.is_http());
    assert!(!P2DirectImport::FsStat.is_http());
}
