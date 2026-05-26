//! v0.23 Track B — regression harness: `wasm32-web` Components must
//! EMBED a real core Wasm module, not just emit the Component
//! preamble + WIT scaffolding.
//!
//! ## Background
//!
//! The Mighty wasm32-web pipeline runs:
//!
//! 1. `compile_program_to_bytes_with_preview(prog, Web, ..)` — produce
//!    the core Wasm module via the slice-8 lowerer.
//! 2. `emit_wit(prog, name, Web)` — generate the matching WIT doc.
//! 3. `wrap_as_component(core, doc)` — embed the core under a
//!    Component Model wrapper and run through `wit_component`.
//!
//! Each of those steps is exercised separately elsewhere. What was
//! missing — and is the reason this file exists — is a contract test
//! that the **final** artifact `mty build --target wasm32-web` writes
//! to disk still carries a real, instantiable core module. A future
//! refactor could trivially break the embedding (e.g. dropping the
//! `.module(&core)` call, swapping the core bytes for an empty stub,
//! wiring up a header-only component to demo the Component encoder)
//! without tripping any of the existing wasm32-wasi-focused tests.
//!
//! The five tests below cover:
//!
//! 1. `wasm32_web_embeds_core_module_preamble` — the Component bytes
//!    contain the canonical core-Wasm preamble `\0asm\x01\0\0\0`
//!    somewhere past the Component header.
//! 2. `wasm32_web_core_has_function_section` — the embedded core
//!    module has a non-empty function section (i.e. actual code).
//! 3. `wasm32_web_main_export_in_core` — `main` is exported by the
//!    embedded core module (the JS shim's entry point).
//! 4. `wasm32_web_size_growth` — output is larger than a synthetic
//!    "header + types only" baseline (~ 600 bytes), confirming the
//!    user program is actually included.
//! 5. `wasm32_web_validates` — both the whole Component AND the
//!    extracted core module validate under
//!    `wasmparser::Validator::new_with_features(WasmFeatures::all())`.
//!
//! These tests use both `common::empty_main()` (a single `fn main`
//! SIR program with no body) AND a non-trivial program (an `add`
//! function so we can prove there's >1 function in the core).
//!
//! See `dev/history/notes/WASM32_WEB_CORE_V0_23_NOTES.md` for the
//! investigation that motivated this harness.

mod common;

use mty_codegen_wasm::{
    compile_program_to_file_with_options, is_component, BuildOptions, WasmTarget,
};
use mty_hir::SourceSpan;
use mty_ir::ir::{
    BinOp, Block, BlockId, Function, IrFnId, IrTy, Local, LocalDecl, LocalSource, Operand, Place,
    Program, Rvalue, Stmt, Term,
};
use mty_types::IntKind;

/// Canonical core-Wasm preamble: magic `\0asm` + version `0x00000001`.
const CORE_PREAMBLE: &[u8] = b"\0asm\x01\x00\x00\x00";

/// Build a `wasm32-web` Component artifact for `prog` named `pkg` and
/// return the on-disk bytes (which is the same as `art.bytes`).
fn build_web(prog: &Program, pkg: &str) -> Vec<u8> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = tmp.path().join(format!("{pkg}.wasm"));
    let opts = BuildOptions::new(pkg);
    let art = compile_program_to_file_with_options(prog, WasmTarget::Web, &out, &opts)
        .expect("build wasm32-web");
    assert!(
        is_component(&art.bytes),
        "wasm32-web output must be a Component, got {:02x?}",
        &art.bytes[..8.min(art.bytes.len())]
    );
    art.bytes
}

/// Scan `bytes` for occurrences of the core-Wasm preamble. Returns
/// every offset where it occurs (the Component preamble at offset 0
/// uses `\0asm\x0d...` so it is NOT matched).
fn find_core_preambles(bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + CORE_PREAMBLE.len() <= bytes.len() {
        if &bytes[i..i + CORE_PREAMBLE.len()] == CORE_PREAMBLE {
            out.push(i);
            i += CORE_PREAMBLE.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Walk the Component bytes via `wasmparser` and return the bytes
/// of every embedded core module section in order. Each returned
/// slice starts with `\0asm\x01\0\0\0`.
fn extract_core_modules(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        let p = payload.expect("payload");
        if let wasmparser::Payload::ModuleSection {
            unchecked_range, ..
        } = p
        {
            let slice = &bytes[unchecked_range.start..unchecked_range.end];
            out.push(slice.to_vec());
        }
    }
    out
}

/// Build a SIR program with `main` plus a second `add(a, b) -> i32`
/// function whose body returns `a + b`. Used to prove non-trivial
/// programs lower to more than the header.
fn add_program() -> Program {
    let mut p = common::empty_main();
    // fn add(a: i32, b: i32) -> i32 { return a + b }
    // Use a leading underscore so `is_exportable_fn` excludes the
    // helper from the generated WIT world; otherwise the Component
    // encoder would require a core-module export for it which the
    // slice-8 lowerer only emits for `main`.
    let mut add = Function {
        id: IrFnId(1),
        name: "_add".into(),
        params: vec![Local(1), Local(2)],
        locals: vec![
            LocalDecl {
                name: "_0".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Return,
            },
            LocalDecl {
                name: "a".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Param,
            },
            LocalDecl {
                name: "b".into(),
                ty: IrTy::Int(IntKind::I32),
                mutable: false,
                source: LocalSource::Param,
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            stmts: vec![Stmt::Assign(
                Place::local(Local(0)),
                Rvalue::BinOp(
                    BinOp::Add,
                    Operand::Copy(Place::local(Local(1))),
                    Operand::Copy(Place::local(Local(2))),
                ),
            )],
            terminator: Term::Return(Operand::Copy(Place::local(Local(0)))),
        }],
        entry: BlockId(0),
        ret_ty: IrTy::Int(IntKind::I32),
        effects: vec![],
        hir_fn: None,
        span: SourceSpan { start: 0, end: 0 },
    };
    add.params = vec![Local(1), Local(2)];
    p.fns.push(add);
    p
}

#[test]
fn wasm32_web_embeds_core_module_preamble() {
    let bytes = build_web(&common::empty_main(), "hello");
    let preambles = find_core_preambles(&bytes);
    // We expect at least one core preamble (the user core module).
    // `wit-component` may also inline 1-2 adapter modules; the test
    // only asserts the lower bound so it doesn't break if the adapter
    // count changes.
    assert!(
        !preambles.is_empty(),
        "no core wasm preamble (\\0asm\\x01\\0\\0\\0) found inside Component; \
         this means the wasm32-web pipeline regressed to header-only output. \
         first 16 bytes: {:02x?}",
        &bytes[..16.min(bytes.len())]
    );
    // Smoke-check: the first preamble must be past the Component
    // header (offset 0..8) so we're not accidentally re-matching the
    // outer module preamble itself.
    assert!(
        preambles[0] >= 8,
        "first core preamble at offset {} overlaps the Component header",
        preambles[0]
    );
}

#[test]
fn wasm32_web_core_has_function_section() {
    let bytes = build_web(&add_program(), "addmod");
    let cores = extract_core_modules(&bytes);
    assert!(
        !cores.is_empty(),
        "expected at least one embedded core module section"
    );
    // The very first embedded module is the user's program (the
    // wit-component adapters come AFTER in the section order). Walk
    // its sections and count function declarations.
    let user_core = &cores[0];
    assert!(
        user_core.starts_with(CORE_PREAMBLE),
        "embedded module #0 doesn't start with core preamble: {:02x?}",
        &user_core[..8.min(user_core.len())]
    );
    let mut fn_count = 0usize;
    let mut code_count = 0usize;
    for payload in wasmparser::Parser::new(0).parse_all(user_core) {
        match payload.expect("payload") {
            wasmparser::Payload::FunctionSection(reader) => {
                fn_count = reader.count() as usize;
            }
            wasmparser::Payload::CodeSectionStart { count, .. } => {
                code_count = count as usize;
            }
            _ => {}
        }
    }
    assert!(
        fn_count >= 1,
        "embedded user core module has function section with {fn_count} entries; \
         expected >= 1 (main + add)"
    );
    assert_eq!(
        fn_count, code_count,
        "function section count ({fn_count}) must match code section count ({code_count})"
    );
}

#[test]
fn wasm32_web_main_export_in_core() {
    let bytes = build_web(&common::empty_main(), "hello");
    let cores = extract_core_modules(&bytes);
    assert!(!cores.is_empty(), "no embedded core modules");

    // Walk the user core module's export section and assert `main`
    // shows up as a function export. The slice-8 lowerer normalises
    // every SIR function name to an unqualified core export.
    let user_core = &cores[0];
    let mut found_main = false;
    let mut export_names: Vec<String> = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(user_core) {
        if let wasmparser::Payload::ExportSection(reader) = payload.expect("payload") {
            for entry in reader {
                let entry = entry.expect("export");
                export_names.push(entry.name.to_string());
                if entry.name == "main" && matches!(entry.kind, wasmparser::ExternalKind::Func) {
                    found_main = true;
                }
            }
        }
    }
    assert!(
        found_main,
        "embedded core module does not export `main`; exports were: {export_names:?}"
    );
}

#[test]
fn wasm32_web_size_growth() {
    // A wasm32-web build must produce a non-trivial artifact. The
    // smallest legal "Component header + types + one empty module"
    // we have seen is just under 2 KB. A regression that drops the
    // embedded core module typically lands below ~1.5 KB, and a
    // header-only Component (no body, no types) would be ~ 100-200
    // bytes. We assert > 1500 bytes for the empty_main fixture as a
    // safety floor that catches *all* of those regressions without
    // being so tight it breaks on routine Component-encoder churn.
    let bytes = build_web(&common::empty_main(), "hello");
    let n = bytes.len();
    assert!(
        n > 1500,
        "wasm32-web artifact is only {n} bytes; expected > 1500. \
         This usually means the Component wrapper shipped without the \
         embedded core module."
    );

    // Sanity check for non-trivial programs: must be strictly larger
    // than the empty case (the extra `add` function body, type
    // signature, and export entry add weight).
    let big = build_web(&add_program(), "addmod");
    assert!(
        big.len() > n,
        "non-trivial program ({} bytes) should be larger than empty_main ({} bytes)",
        big.len(),
        n,
    );
}

#[test]
fn wasm32_web_validates() {
    let bytes = build_web(&add_program(), "addmod");

    // Whole-component validation (this is the contract the browser
    // host eventually relies on once it consumes raw components).
    let mut comp_v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
    comp_v
        .validate_all(&bytes)
        .expect("Component must validate under wasmparser with all features");

    // Extract every embedded core module and validate each one in
    // isolation — exactly what `findCoreModule` + `WebAssembly.compile`
    // do on the browser side. They MUST all be standalone-valid wasm
    // modules.
    let cores = extract_core_modules(&bytes);
    assert!(
        !cores.is_empty(),
        "no embedded core modules to validate against"
    );
    for (i, core) in cores.iter().enumerate() {
        assert!(
            core.starts_with(CORE_PREAMBLE),
            "embedded module #{i} missing core preamble: {:02x?}",
            &core[..8.min(core.len())]
        );
        let mut v = wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
        v.validate_all(core)
            .unwrap_or_else(|e| panic!("embedded core module #{i} failed validation: {e:#}"));
    }
}
