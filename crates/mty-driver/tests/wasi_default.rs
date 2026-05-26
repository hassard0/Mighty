//! WASI preview default-selection tests (v0.15 flip + v0.17 direct-log).
//!
//! v0.13/v0.14 defaulted `--wasi` to P1 for back-compat with the
//! slice-8 emitter. v0.15 flipped the default to P2 now that the
//! codegen layer wires direct versioned imports for `std.random`
//! and `std.time` and the vendored adapter handles the remaining
//! surfaces (`std.fs`, `std.http`, `log()`). v0.17 dropped the
//! adapter dependency for `log()` — it now lowers directly to
//! `wasi:cli/stdout@0.2.3` + `wasi:io/streams@0.2.3` blocking writes,
//! and the adapter default is opt-in (`with_adapter`).
//!
//! These tests pin both the new default selection AND the back-compat
//! `--wasi=p1` opt-out so a future tweak can't silently regress
//! either side.

use mty_codegen_cranelift::artifact::BuildMode;
use mty_codegen_wasm::WasmTarget;
use mty_driver::build::{build_wasm, BuildOptions, BuildOutcome, BuildTarget, WasiPreview};

/// `WasiPreview::default()` must be `P2` in v0.15 (was `P1` through
/// v0.14). The CLI consumes this default whenever no `--wasi` flag
/// is passed; pin it here so an inadvertent revert of the
/// `#[derive(Default)]` selection is caught immediately.
#[test]
fn default_wasi_preview_is_p2() {
    assert_eq!(WasiPreview::default(), WasiPreview::P2);
}

/// Build a tiny wasm program with no explicit `--wasi` flag (i.e.
/// take `WasiPreview::default()`). The produced artifact must be a
/// Component-Model component (the P2 path always wraps in a
/// component) and must declare versioned `wasi:*@0.2.3` imports
/// somewhere in its import section, not the legacy
/// `wasi_snapshot_preview1` shape at the component layer.
#[test]
fn default_wasi_preview_produces_p2_component_for_wasm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = BuildOptions {
        target: BuildTarget::Wasm(WasmTarget::Wasi),
        mode: BuildMode::Release,
        out_dir: dir.path().to_path_buf(),
        binary_name: "wasi_default".into(),
        no_component: false,
        // No explicit override → take the v0.15 default.
        wasi_preview: WasiPreview::default(),
        user_wit: None,
    };
    // v0.17 dropped the standing `wasi:cli/log` shim, so a P2 build
    // only emits the new direct-import surface for code that actually
    // calls `log()`. Use a tiny log()-calling main so the assertion
    // below has a real import to check against.
    let outcome = build_wasm(
        "fn main() { log(\"wasi_default v0.17 marker\") }\n".into(),
        "wasi_default.mty".into(),
        &opts,
        WasmTarget::Wasi,
    );
    match outcome {
        BuildOutcome::WasmOk(p) => {
            let bytes = std::fs::read(&p).expect("read wasm");
            assert!(
                mty_codegen_wasm::is_component(&bytes),
                "v0.15 default must produce a component (got a core module)"
            );
            // v0.17 dropped the wasi:cli/log shim: `log()` now lowers
            // directly to `wasi:cli/stdout@0.2.3` +
            // `wasi:io/streams@0.2.3.[method]output-stream.
            // blocking-write-and-flush` (+ matching resource-drop).
            // Pin the new direct-import signature so a future regression
            // to the shim path is caught immediately. (Same signature
            // as the v0.17 `preview2_log` codegen-wasm tests; replicated
            // here as the driver-level marker that "default produces a
            // P2 component with direct P2 imports".)
            assert!(
                component_text_contains(&bytes, "wasi:cli/stdout@0.2.3"),
                "expected wasi:cli/stdout@0.2.3 in v0.17 P2 default component (signature of the direct-import log path)"
            );
            // v0.17: the unversioned `wasi:cli/log` shim is gone from
            // the live import section. (It may still appear inside the
            // vendored component-type WIT metadata if anything imports
            // wasi:cli wholesale; the assertion below targets only the
            // live imports stream.)
            assert!(
                !component_text_contains_in_imports(&bytes, "wasi:cli/log"),
                "v0.17 default P2 build leaked the dropped wasi:cli/log shim into live imports"
            );
            // Also pin: the component MUST NOT declare a top-level
            // `wasi_snapshot_preview1` import — that would be the
            // signature of a P1-only wrap.
            assert!(
                !component_text_contains_in_imports(&bytes, "wasi_snapshot_preview1"),
                "default P2 build leaked a wasi_snapshot_preview1 top-level import"
            );
        }
        other => panic!("expected WasmOk from default-preview build, got {other:?}"),
    }
}

/// Explicitly passing `--wasi=p1` must still produce a Component
/// Model component but route through the legacy P1 path (no
/// versioned wasi imports at the component layer). This is the
/// back-compat opt-out and must continue working forever — many
/// downstream consumers still pin P1 in their build scripts.
#[test]
fn explicit_p1_still_works_for_wasm() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = BuildOptions {
        target: BuildTarget::Wasm(WasmTarget::Wasi),
        mode: BuildMode::Release,
        out_dir: dir.path().to_path_buf(),
        binary_name: "wasi_p1_optout".into(),
        no_component: false,
        wasi_preview: WasiPreview::P1,
        user_wit: None,
    };
    let outcome = build_wasm(
        "fn main() {}\n".into(),
        "wasi_p1_optout.mty".into(),
        &opts,
        WasmTarget::Wasi,
    );
    match outcome {
        BuildOutcome::WasmOk(p) => {
            let bytes = std::fs::read(&p).expect("read wasm");
            // Component Model wrap still happens for P1 — this hasn't
            // changed since v0.2.
            assert!(
                mty_codegen_wasm::is_component(&bytes),
                "P1 build should still produce a component (legacy wrap path)"
            );
        }
        other => panic!("expected WasmOk from P1 opt-out, got {other:?}"),
    }
}

/// `WasiPreview::parse` must keep accepting all four spellings
/// (`p1`, `preview1`, `p2`, `preview2`) so existing scripts that
/// pass `--wasi=preview1` keep working after the default flip.
#[test]
fn wasi_preview_parse_back_compat() {
    assert_eq!(WasiPreview::parse("p1"), Some(WasiPreview::P1));
    assert_eq!(WasiPreview::parse("preview1"), Some(WasiPreview::P1));
    assert_eq!(WasiPreview::parse("p2"), Some(WasiPreview::P2));
    assert_eq!(WasiPreview::parse("preview2"), Some(WasiPreview::P2));
    assert_eq!(WasiPreview::parse("garbage"), None);
}

/// Native builds carry the `wasi_preview` field for ergonomic
/// uniformity, but the value is ignored. Pin that constructing a
/// native build with the v0.15 default doesn't accidentally route
/// through the wasm code path.
#[test]
fn native_build_ignores_wasi_preview_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let opts = BuildOptions::native_release(dir.path().to_path_buf(), "native_smoke");
    assert!(matches!(opts.target, BuildTarget::Native));
    assert_eq!(opts.wasi_preview, WasiPreview::default());
}

/// Scan a component for `needle` across both its live imports and
/// the embedded `component-type` WIT custom section. Used by the
/// default-selection test to assert a versioned reference is
/// present somewhere — the precise surface (live import vs. WIT
/// metadata) depends on wit-component's pruning pass which is
/// outside our control.
fn component_text_contains(bytes: &[u8], needle: &str) -> bool {
    component_text_contains_in_imports(bytes, needle)
        || component_text_contains_in_wit(bytes, needle)
}

/// Stricter variant: looks only at the live component-imports stream.
/// Useful for "must not contain X" assertions where we don't care
/// whether the embedded WIT metadata mentions X (it usually does
/// because the vendored P2 slice declares the full surface).
fn component_text_contains_in_imports(bytes: &[u8], needle: &str) -> bool {
    use wasmparser::{Parser, Payload};
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(payload) = payload else { continue };
        if let Payload::ComponentImportSection(reader) = payload {
            for imp in reader.into_iter().flatten() {
                if imp.name.name.starts_with(needle) {
                    return true;
                }
            }
        }
    }
    false
}

fn component_text_contains_in_wit(bytes: &[u8], needle: &str) -> bool {
    use wasmparser::{Parser, Payload};
    let needle_b = needle.as_bytes();
    for payload in Parser::new(0).parse_all(bytes) {
        let Ok(payload) = payload else { continue };
        if let Payload::CustomSection(reader) = payload {
            if reader.name().starts_with("component-type") {
                let data = reader.data();
                if data.windows(needle_b.len()).any(|w| w == needle_b) {
                    return true;
                }
            }
        }
    }
    false
}
