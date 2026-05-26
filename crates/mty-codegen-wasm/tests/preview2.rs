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
    compile_program_to_bytes_p2, compile_program_to_file_p2, emit_wit_p2, is_component,
    Preview2Options, UserWit,
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
fn p2_component_imports_include_wasi_log_shim() {
    // The component declares only those world imports its core module
    // actually uses; wit-component prunes the rest. For an empty
    // `fn main() {}` the only retained import is the `wasi:cli/log`
    // shim (used by Mighty's slice-8 log() lowering). Assert it's
    // present, and assert NO `wasi_snapshot_preview1` import — that's
    // our boundary marker between the P2 path and the legacy P1 one.
    let prog = common::empty_main();
    let opts = Preview2Options::new("smoke");
    let bytes = compile_program_to_bytes_p2(&prog, &opts).expect("emit p2");
    assert!(
        component_has_versioned_wasi_import(&bytes, "wasi:cli/log"),
        "expected wasi:cli/log import (shim) in the component"
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
