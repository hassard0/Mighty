//! WASI Preview 2 backend integration tests.
//!
//! Verifies:
//! 1. P2 builds round-trip and produce a Component Model component,
//! 2. The component's import section references `wasi:*@0.2.3`,
//! 3. User-supplied WIT (a custom `[wit]` package) is merged in and
//!    the named world is selected.
//!
//! When `wasmtime-wasi` is on the dev-dep path AND the host can spin
//! up a P2 context, an additional smoke test instantiates the
//! component and checks that `main` runs without trapping. That test
//! is gated by the `wasmtime_p2_smoke` feature (off by default for
//! v0.13 — see WASI_P2_V0_13_NOTES.md for the rationale).

mod common;

use mty_codegen_wasm::{
    build_direct_p2_probe_module, compile_program_to_bytes_p2, compile_program_to_file_p2,
    emit_wit_p2, is_component, AdapterEmbed, AdapterKind, P2DirectImport, Preview2Options, UserWit,
};

#[test]
fn p2_component_is_a_component() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");
    assert!(is_component(&bytes), "expected component preamble");
}

#[test]
fn p2_component_validates_with_full_features() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes).expect("p2 component validates");
}

#[test]
fn p2_wit_doc_references_versioned_wasi() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let doc = emit_wit_p2(&prog, &opts).expect("emit p2 wit");
    // P2-spec versioned imports must be present.
    assert!(
        doc.text.contains("wasi:io/streams@0.2.3"),
        "missing wasi:io. text:\n{}",
        doc.text
    );
    assert!(doc.text.contains("wasi:cli/stdout@0.2.3"));
    assert!(doc.text.contains("wasi:cli/exit@0.2.3"));
    assert!(doc.text.contains("wasi:http/outgoing-handler@0.2.3"));
    assert!(doc.text.contains("wasi:filesystem/types@0.2.3"));
    assert!(doc.text.contains("wasi:clocks/monotonic-clock@0.2.3"));
}

#[test]
fn p2_component_no_wasi_cli_log_shim_in_v017() {
    // v0.17 dropped the unversioned `wasi:cli/log` shim. The empty
    // main here doesn't call `log()`, so the component shouldn't
    // reference any cli/log-shaped import — neither the shim nor
    // the v0.17 direct-lowering trio (those only land when a
    // `log()` call is actually emitted).
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");
    assert!(
        !component_has_versioned_wasi_import(&bytes, "wasi:cli/log"),
        "v0.17: wasi:cli/log shim must be gone from the component",
    );
}

#[test]
fn p2_wit_text_declares_full_p2_surface() {
    // Even though wit-component prunes unused imports from the
    // wire-level component, the *WIT document we hand to the encoder*
    // declares the full versioned P2 surface. This test pins that
    // surface so a regression in `preview2::emit_wit_p2` is caught
    // immediately.
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let doc = emit_wit_p2(&prog, &opts).expect("wit");
    for needle in [
        "wasi:io/streams@0.2.3",
        "wasi:io/poll@0.2.3",
        "wasi:cli/stdout@0.2.3",
        "wasi:cli/stderr@0.2.3",
        "wasi:cli/exit@0.2.3",
        "wasi:cli/environment@0.2.3",
        "wasi:clocks/monotonic-clock@0.2.3",
        "wasi:clocks/wall-clock@0.2.3",
        "wasi:random/random@0.2.3",
        "wasi:filesystem/preopens@0.2.3",
        "wasi:filesystem/types@0.2.3",
        "wasi:http/types@0.2.3",
        "wasi:http/outgoing-handler@0.2.3",
    ] {
        assert!(
            doc.text.contains(needle),
            "missing {needle} in synthesized P2 WIT"
        );
    }
}

#[test]
fn p2_component_no_p1_snapshot_import() {
    // The core module that lives *inside* a P2 component might still
    // declare its log import as `wasi:cli/log#log` (P1-style) until the
    // v0.14 lowering work lands; but the *outer* component must not
    // reference `wasi_snapshot_preview1` anywhere, which would be the
    // tell-tale sign of pure-P1 wrapping. Check both the live
    // component imports (likely empty of wasi_snapshot_preview1) and
    // the embedded WIT.
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");
    assert!(
        !component_has_versioned_wasi_import(&bytes, "wasi_snapshot_preview1"),
        "P2 component should not declare wasi_snapshot_preview1 in imports"
    );
    assert!(
        !component_type_section_contains(&bytes, "wasi_snapshot_preview1"),
        "P2 component-type WIT should not reference wasi_snapshot_preview1"
    );
}

#[test]
fn p2_with_user_wit_includes_user_package() {
    let prog = common::empty_main();
    let user_text = r#"
package demo:user-pkg;

interface greeter {
  greet: func(who: string) -> string;
}

world custom-world {
  import wasi:cli/stdout@0.2.3;
  export greeter;
}
"#;
    let opts = Preview2Options::new("smoke").with_user_wit(UserWit {
        text: user_text.into(),
        world: Some("custom-world".into()),
        source_label: "test-user-wit".into(),
    });
    let doc = emit_wit_p2(&prog, &opts).expect("emit p2 wit + user");
    assert!(doc.text.contains("demo:user-pkg"));
    assert_eq!(doc.world_name, "custom-world");
    // `doc.text` is a *display* serialization concatenating multiple
    // top-level packages — the v0.14 architecture pushes each
    // package into a separate `Resolve::push_str` call so they can
    // cross-reference (a single multi-package blob is not a legal
    // .wit file). The end-to-end validation that the merge succeeded
    // is `emit_wit_p2`'s round-trip parse (which has already run by
    // the time `doc` is returned here).
    assert!(
        doc.text.contains("custom-world"),
        "expected user world name in display text"
    );
    assert!(
        doc.text.contains("greet"),
        "expected user interface fn in display text"
    );
}

#[test]
fn p2_with_user_wit_missing_world_errors() {
    let prog = common::empty_main();
    let user_text = "package demo:empty;\n";
    let opts = Preview2Options::new("smoke").with_user_wit(UserWit {
        text: user_text.into(),
        world: Some("does-not-exist".into()),
        source_label: "missing-world".into(),
    });
    let err = emit_wit_p2(&prog, &opts).expect_err("should error");
    let msg = format!("{err}");
    assert!(
        msg.contains("does-not-exist"),
        "expected world name in error, got: {msg}"
    );
}

#[test]
fn p2_file_emission_writes_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("hello-p2.wasm");
    let prog = common::empty_main();
    let opts = Preview2Options::new("hello");
    let (bytes, doc) = compile_program_to_file_p2(&prog, &opts, &out).expect("file emission");
    assert!(out.exists(), "output file missing");
    let disk_bytes = std::fs::read(&out).expect("read");
    assert_eq!(bytes, disk_bytes);
    assert!(doc.text.contains("wasi:cli/stdout@0.2.3"));
}

/// Walk the wasmparser component-imports stream and return true iff
/// any import name's "namespace/name" prefix starts with `prefix`.
///
/// Used to assert that the produced component declares the versioned
/// P2 interface set (and not the legacy P1 snapshot import).
fn component_has_versioned_wasi_import(bytes: &[u8], prefix: &str) -> bool {
    use wasmparser::{Parser, Payload};
    let parser = Parser::new(0);
    for payload in parser.parse_all(bytes) {
        let Ok(payload) = payload else {
            continue;
        };
        if let Payload::ComponentImportSection(reader) = payload {
            for imp in reader.into_iter().flatten() {
                if imp.name.name.starts_with(prefix) {
                    return true;
                }
            }
        }
    }
    false
}

/// Scan the component's custom sections for the embedded WIT
/// `component-type` blob and return true iff `needle` appears anywhere
/// in its bytes (as raw substring). Currently used in
/// `p2_component_no_p1_snapshot_import` to assert the absence of the
/// legacy snapshot interface across both the wire-level imports and
/// the embedded WIT contract.
fn component_type_section_contains(bytes: &[u8], needle: &str) -> bool {
    use wasmparser::{Parser, Payload};
    let needle_bytes = needle.as_bytes();
    let parser = Parser::new(0);
    for payload in parser.parse_all(bytes) {
        let Ok(payload) = payload else {
            continue;
        };
        if let Payload::CustomSection(reader) = payload {
            if reader.name().starts_with("component-type") {
                let data = reader.data();
                if data.windows(needle_bytes.len()).any(|w| w == needle_bytes) {
                    return true;
                }
            }
        }
    }
    false
}

// ============================================================
// v0.14 — adapter + direct-import lowering coverage.
//
// v0.19 deleted the vendored `wasi_snapshot_preview1.*.wasm`
// bytes from the crate. The default P2 build path doesn't import
// `wasi_snapshot_preview1` at all (every stdlib call has a direct
// P2 lowering since v0.17), so the adapter is now purely opt-in
// and the bytes are caller-supplied via `AdapterEmbed`. Tests
// below exercise the default (no-adapter) path end-to-end; the
// opt-in path is exercised at the API-shape level (constructing
// an `AdapterEmbed` and confirming it lands in `embed_adapter`)
// because invoking `wit-component::ComponentEncoder::adapter`
// requires a *real* adapter, which is no longer in-tree.
// ============================================================

/// When the P2 build path runs with the v0.17 default
/// (`embed_adapter = None`) the resulting component must
/// validate AND its import section must not reference
/// `wasi_snapshot_preview1` at the component-imports layer
/// (the adapter is gone entirely; there's nothing to wrap).
#[test]
fn adapter_default_none_for_p2() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke"); // v0.17 default = None
    assert!(
        opts.embed_adapter.is_none(),
        "v0.17 default: embed_adapter starts at None"
    );
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");

    // Component must validate.
    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes)
        .expect("p2 component validates without adapter");

    // No outer-component `wasi_snapshot_preview1` import — the
    // adapter is gone entirely under the v0.17 default.
    assert!(
        !component_has_versioned_wasi_import(&bytes, "wasi_snapshot_preview1"),
        "wasi_snapshot_preview1 must not appear as a component-level import"
    );
}

/// With `embed_adapter = None`, the produced component must still
/// validate (since the slice-8 core module doesn't import any
/// `wasi_snapshot_preview1` calls under the v0.17 direct-lowering
/// path). This pins the opt-out path.
#[test]
fn adapter_can_be_opted_out() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke").with_adapter(None);
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2 no-adapter");

    let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    v.validate_all(&bytes)
        .expect("p2 no-adapter component still validates");

    assert!(is_component(&bytes));
}

/// API-shape coverage for the opt-in path. `with_adapter(Some(_))`
/// must round-trip a caller-supplied `AdapterEmbed` into
/// `embed_adapter` field-for-field. v0.19 no longer vendors the
/// adapter bytes in-tree, so we don't try to drive
/// `compile_program_to_bytes_p2` through this path (it requires a
/// *real* P1→P2 adapter for wit-component to accept).
#[test]
fn adapter_opt_in_roundtrips_bytes_and_kind() {
    // Cheap synthetic bytes — wasm magic + version, nothing else.
    // We never hand them to wit-component; the test only checks
    // that the API plumbs them into `embed_adapter`.
    let mock_bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for kind in [
        AdapterKind::Command,
        AdapterKind::Reactor,
        AdapterKind::Proxy,
    ] {
        let opts = Preview2Options::new("opt-in")
            .with_adapter(Some(AdapterEmbed::new(kind, mock_bytes.clone())));
        let stored = opts.embed_adapter.as_ref().expect("embed_adapter set");
        assert_eq!(stored.kind, kind);
        assert_eq!(stored.bytes, mock_bytes);
        // The kind tag still drives the legacy module name the core
        // module's P1 imports reference.
        assert_eq!(kind.import_module_name(), "wasi_snapshot_preview1");
    }
}

/// Validate that a probe module declaring a direct
/// `wasi:random/random@0.2.3#get-random-bytes` import produces a
/// component whose import section references that exact versioned
/// interface — no `wasi_snapshot_preview1` hop. This pins the
/// direct-lowering path the v0.15 std.random work will use.
#[test]
fn p2_random_uses_direct_import() {
    let core = build_direct_p2_probe_module(P2DirectImport::RandomBytes);
    // Wrap the probe via the P2 path with the adapter ON (the
    // adapter shouldn't interfere with direct imports that don't
    // collide with adapter exports).
    let bytes = wrap_probe_module(&core, "wasi:random/random@0.2.3");

    assert!(is_component(&bytes), "probe wraps as a component");
    // The component's IMPORTS must include wasi:random/random@0.2.3.
    assert!(
        component_has_versioned_wasi_import(&bytes, "wasi:random/random@0.2.3"),
        "expected direct wasi:random import in component"
    );
}

/// Same as `p2_random_uses_direct_import` for `wasi:clocks/wall-clock`.
#[test]
fn p2_wall_clock_uses_direct_import() {
    let core = build_direct_p2_probe_module(P2DirectImport::WallClockNow);
    let bytes = wrap_probe_module(&core, "wasi:clocks/wall-clock@0.2.3");

    assert!(is_component(&bytes));
    assert!(
        component_has_versioned_wasi_import(&bytes, "wasi:clocks/wall-clock@0.2.3"),
        "expected direct wasi:clocks/wall-clock import in component"
    );
}

/// And for `wasi:clocks/monotonic-clock`.
#[test]
fn p2_monotonic_clock_uses_direct_import() {
    let core = build_direct_p2_probe_module(P2DirectImport::MonotonicNow);
    let bytes = wrap_probe_module(&core, "wasi:clocks/monotonic-clock@0.2.3");

    assert!(is_component(&bytes));
    assert!(
        component_has_versioned_wasi_import(&bytes, "wasi:clocks/monotonic-clock@0.2.3"),
        "expected direct wasi:clocks/monotonic-clock import in component"
    );
}

/// `P2DirectImport::import_pair` and the corresponding stdlib
/// `P2_DIRECT_IMPORT_*` constants must agree on the import names.
/// This pins the single source of truth across crates and catches
/// drift if someone bumps one but not the other.
#[test]
fn p2_direct_import_names_match_stdlib_constants() {
    use mty_stdlib::{fs as stdfs, http as stdhttp, random as stdrand, time as stdtime};

    assert_eq!(
        P2DirectImport::RandomBytes.import_pair(),
        stdrand::P2_DIRECT_IMPORT_RANDOM_BYTES
    );
    assert_eq!(
        P2DirectImport::MonotonicNow.import_pair(),
        stdtime::P2_DIRECT_IMPORT_MONOTONIC_NOW
    );
    assert_eq!(
        P2DirectImport::WallClockNow.import_pair(),
        stdtime::P2_DIRECT_IMPORT_WALL_CLOCK_NOW
    );
    assert_eq!(
        P2DirectImport::MonotonicResolution.import_pair(),
        stdtime::P2_DIRECT_IMPORT_MONOTONIC_RESOLUTION
    );
    // v0.16 — filesystem.
    assert_eq!(
        P2DirectImport::FsOpenAt.import_pair(),
        stdfs::P2_DIRECT_IMPORT_OPEN_AT
    );
    assert_eq!(
        P2DirectImport::FsReadViaStream.import_pair(),
        stdfs::P2_DIRECT_IMPORT_READ_VIA_STREAM
    );
    assert_eq!(
        P2DirectImport::FsWriteViaStream.import_pair(),
        stdfs::P2_DIRECT_IMPORT_WRITE_VIA_STREAM
    );
    assert_eq!(
        P2DirectImport::FsStat.import_pair(),
        stdfs::P2_DIRECT_IMPORT_STAT
    );
    assert_eq!(
        P2DirectImport::FsClose.import_pair(),
        stdfs::P2_DIRECT_IMPORT_CLOSE
    );
    // v0.16 — http.
    assert_eq!(
        P2DirectImport::HttpNewRequest.import_pair(),
        stdhttp::P2_DIRECT_IMPORT_NEW_OUTGOING_REQUEST
    );
    assert_eq!(
        P2DirectImport::HttpHandleRequest.import_pair(),
        stdhttp::P2_DIRECT_IMPORT_OUTGOING_HANDLE
    );
    assert_eq!(
        P2DirectImport::HttpResponseStatus.import_pair(),
        stdhttp::P2_DIRECT_IMPORT_RESPONSE_STATUS
    );
    assert_eq!(
        P2DirectImport::HttpResponseBody.import_pair(),
        stdhttp::P2_DIRECT_IMPORT_RESPONSE_CONSUME
    );
}

/// `AdapterKind::import_module_name()` is the only piece of the
/// enum that survives v0.19 (the `bytes()` accessor is gone with
/// the vendored adapter modules). Each kind must report the same
/// legacy module name — that's the name the core module's P1
/// imports reference and what wit-component matches on.
#[test]
fn adapter_kind_import_module_name_is_stable() {
    for kind in [
        AdapterKind::Command,
        AdapterKind::Reactor,
        AdapterKind::Proxy,
    ] {
        assert_eq!(kind.import_module_name(), "wasi_snapshot_preview1");
    }
}

// ============================================================
// v0.15 — wired-in direct-import dispatch coverage.
// ============================================================

use mty_codegen_wasm::{compile_program_to_bytes_with_preview, EmitWasiPreview};

/// When the emitter is built with [`EmitWasiPreview::P2`] AND the
/// SIR program calls `std.random.bytes(n)`, the produced *core*
/// module must declare a direct
/// `wasi:random/random@0.2.3#get-random-bytes` import — no
/// `wasi_snapshot_preview1` hop. Pins the v0.15 dispatch path.
#[test]
fn random_bytes_emits_direct_p2_import() {
    let prog = std_call_program("std.random.bytes", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(&core, "wasi:random/random@0.2.3", "get-random-bytes"),
        "expected versioned wasi:random direct import in core module"
    );
    assert!(
        !core_module_imports(&core, "wasi_snapshot_preview1", "fd_random_get"),
        "core module must not fall back to wasi_snapshot_preview1 for random"
    );
}

/// Same idea for `std.time.now` → wasi:clocks/wall-clock.now.
#[test]
fn time_now_emits_direct_p2_import() {
    let prog = std_call_program("std.time.now", /*pass_arg=*/ false);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(&core, "wasi:clocks/wall-clock@0.2.3", "now"),
        "expected versioned wasi:clocks/wall-clock direct import"
    );
}

/// Same idea for `std.time.monotonic_now` →
/// wasi:clocks/monotonic-clock.now.
#[test]
fn time_monotonic_now_emits_direct_p2_import() {
    let prog = std_call_program("std.time.monotonic_now", /*pass_arg=*/ false);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(&core, "wasi:clocks/monotonic-clock@0.2.3", "now"),
        "expected versioned wasi:clocks/monotonic-clock direct import"
    );
}

/// And `std.time.resolution` →
/// wasi:clocks/monotonic-clock.resolution.
#[test]
fn time_resolution_emits_direct_p2_import() {
    let prog = std_call_program("std.time.resolution", /*pass_arg=*/ false);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P2,
    )
    .expect("compile p2");
    assert!(
        core_module_imports(&core, "wasi:clocks/monotonic-clock@0.2.3", "resolution"),
        "expected versioned wasi:clocks/monotonic-clock.resolution direct import"
    );
}

/// On the **P1** dispatch path the same SIR program must NOT splice
/// in any versioned `wasi:*@0.2.3` import — we'd regress back-compat
/// otherwise. The body falls through to the legacy
/// `WasmError::Unsupported` → single-`unreachable` placeholder, which
/// still validates and still doesn't reference the versioned
/// interfaces.
#[test]
fn random_bytes_under_p1_skips_direct_import() {
    let prog = std_call_program("std.random.bytes", /*pass_arg=*/ true);
    let core = compile_program_to_bytes_with_preview(
        &prog,
        mty_codegen_wasm::WasmTarget::Wasi,
        EmitWasiPreview::P1,
    )
    .expect("compile p1");
    assert!(
        !core_module_imports(&core, "wasi:random/random@0.2.3", "get-random-bytes"),
        "P1 build must not declare the direct P2 import for std.random.bytes"
    );
}

/// v0.17 — the `wasi:cli/log` shim has been retired entirely.
/// The P2 WIT document no longer declares the unversioned
/// `wasi:cli` package; log() lowers directly to
/// `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3`.
#[test]
fn log_shim_removed_in_v017() {
    let prog = common::empty_main();
    let opts = Preview2Options::new("shim-doc");
    let doc = emit_wit_p2(&prog, &opts).expect("p2 wit");
    // The WIT doc text should NOT contain a "package wasi:cli;"
    // declaration (the v0.13–v0.16 unversioned shim) anymore.
    // The versioned `package wasi:cli@0.2.3 {` block is fine —
    // that's the upstream wasi:cli interface set.
    assert!(
        !doc.text.contains("package wasi:cli;"),
        "v0.17: the unversioned wasi:cli/log shim package must be gone"
    );
    // The migration breadcrumb is left in the WIT comments so
    // anyone reading the doc sees the rationale.
    assert!(
        doc.text.contains("v0.17"),
        "expected a v0.17 migration breadcrumb in the WIT doc"
    );
}

/// Build a Program that calls one of the std.* extern paths. When
/// `pass_arg` is true, the call gets a single i32 arg (matches the
/// `std.random.bytes(n)` shape); otherwise it's a no-arg call.
///
/// The body discards the call result (sinks via Drop on a fresh
/// temp). This is enough for the emitter to splice in the import;
/// what we assert in the test is the import section's contents.
fn std_call_program(extern_name: &str, pass_arg: bool) -> mty_ir::ir::Program {
    use mty_hir::SourceSpan;
    use mty_ir::ir::{
        Block, BlockId, BuiltinId, Const, FnRef, Function, IrFnId, IrTy, Local, LocalDecl,
        LocalSource, Operand, Place, Program, Rvalue, Stmt, Term,
    };
    use mty_types::IntKind;

    let mut p = Program::default();
    // Local 0 = return slot (Unit).
    // Local 1 = call-result sink (Error-typed so emit drops it).
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
        // Local 2 = constant `n`.
        locals.push(LocalDecl {
            name: "n".into(),
            ty: IrTy::Int(IntKind::I32),
            mutable: false,
            source: LocalSource::Temp,
        });
        args.push(Operand::Const(Const::Int(16, IntKind::I32)));
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

/// Walk the core module's import section and return true iff any
/// import matches the `(module, name)` pair exactly.
///
/// wasmparser's [`wasmparser::Imports`] groups imports by module
/// name when the encoder packs them together; we handle all three
/// shapes (`Single`, `Compact1`, `Compact2`) to match what
/// `wasm-encoder` produces today.
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

// ---- helpers ----------------------------------------------------------------

/// Wrap a hand-crafted probe core-module as a P2 component by
/// hand-rolling a WIT document that declares the single versioned
/// interface the probe imports. This is the integration-test
/// equivalent of `mty build` for a direct-lowering core.
///
/// `import_id` is the WIT-form id (e.g. `"wasi:random/random@0.2.3"`)
/// the probe expects on the host side.
fn wrap_probe_module(core: &[u8], import_id: &str) -> Vec<u8> {
    use mty_codegen_wasm::VENDORED_WASI_P2_WIT;

    // Build a tiny WIT world that imports the one interface the
    // probe needs, then run the standard wit-component encode path.
    let world_name = "probe-world";
    // No `export _start` in WIT — WIT identifiers are kebab-case and
    // can't start with `_`. The probe core-module exports `_start`
    // as its `wasm-exec` entry; wit-component picks it up via the
    // `wasi:cli/run.run` shape if it's present, or simply leaves it
    // as a free export otherwise.
    let mighty_pkg =
        format!("package mighty:probe;\n\nworld {world_name} {{\n  import {import_id};\n}}\n");
    let mut resolve = wit_parser::Resolve::default();
    // Push the upstream WIT packages in topological order so
    // cross-package use's resolve.
    for (label, text) in split_vendored_packages(VENDORED_WASI_P2_WIT) {
        resolve.push_str(&label, &text).expect("vendored push_str");
    }
    let pkg_id = resolve
        .push_str("probe.wit", &mighty_pkg)
        .expect("probe pkg push_str");
    let world_id = resolve
        .select_world(pkg_id, Some(world_name))
        .expect("select probe world");

    let mut module_bytes = core.to_vec();
    wit_component::embed_component_metadata(
        &mut module_bytes,
        &resolve,
        world_id,
        wit_component::StringEncoding::UTF8,
    )
    .expect("embed metadata");
    let mut enc = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module_bytes)
        .expect("encoder.module");
    enc.encode().expect("encode probe component")
}

/// Test-side mirror of `preview2.rs::split_nested_into_packages`.
/// Reimplemented here (rather than pub-exporting the helper) so the
/// integration test surface stays narrow.
fn split_vendored_packages(text: &str) -> Vec<(String, String)> {
    const ORDER: &[&str] = &[
        "wasi:io@0.2.3",
        "wasi:clocks@0.2.3",
        "wasi:random@0.2.3",
        "wasi:sockets@0.2.3",
        "wasi:filesystem@0.2.3",
        "wasi:cli@0.2.3",
        "wasi:http@0.2.3",
    ];
    let mut chunks: Vec<(String, String, String)> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    'outer: while i < text.len() {
        let rest = &text[i..];
        let Some(pos) = rest.find("package ") else {
            break;
        };
        let abs = i + pos;
        // Reject `package ` matches that aren't at line-start
        // (whitespace-only prefix on the current line). This filters
        // out occurrences inside `//` comment lines.
        let line_start = text[..abs].rfind('\n').map(|p| p + 1).unwrap_or(0);
        if !text[line_start..abs].chars().all(|c| c == ' ' || c == '\t') {
            i = abs + "package ".len();
            continue 'outer;
        }
        let after = &text[abs..];
        let Some(brace) = after.find('{') else {
            break;
        };
        let semi = after.find(';');
        if let Some(s) = semi {
            if s < brace {
                i = abs + s + 1;
                continue;
            }
        }
        let name = after["package ".len()..brace].trim().to_string();
        let body_start = abs + brace + 1;
        let mut depth = 1;
        let mut j = body_start;
        while j < bytes.len() && depth > 0 {
            match bytes[j] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let body = &text[body_start..(j - 1)];
        let label = format!("probe-{}.wit", name.replace(':', "-").replace('@', "_"));
        let chunk_text = format!("package {name};\n{body}\n");
        chunks.push((label, name, chunk_text));
        i = j;
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for target in ORDER {
        if let Some(pos) = chunks.iter().position(|(_, n, _)| n == target) {
            let (l, _, c) = chunks.swap_remove(pos);
            out.push((l, c));
        }
    }
    for (l, _, c) in chunks {
        out.push((l, c));
    }
    out
}
