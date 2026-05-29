//! v0.23 Track A — WIT generation: assert that the `wasm32-web` target
//! gets `mty:web/canvas@0.1` + `mty:web/input@0.1` interfaces wired
//! into the generated world, and that the host stubs declare the
//! method shapes Track D's host shim will bind against.
//!
//! These tests are the contract between Track A (this crate) and the
//! other three v0.23 tracks:
//!
//! - Track B (lowerer): pattern-matches on the canvas/input import
//!   names emitted here when lowering `std.web.Canvas::fill_rect` /
//!   `std.web.Input::subscribe_keydown` calls.
//! - Track C (CLI): defaults `--target=wasm32-web` for `mty run --web`
//!   demos so the demo agent picks up these imports.
//! - Track E (host glue): binds JS-side `HTMLCanvasElement` /
//!   `window.addEventListener` to the import names below.

mod common;

use mty_codegen_wasm::{emit_wit, WasmTarget};

#[test]
fn wit_world_includes_canvas_and_input() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("import mty:web/canvas;"),
        "expected canvas import in wit, was: {}",
        doc.text
    );
    assert!(
        doc.text.contains("import mty:web/input;"),
        "expected input import in wit, was: {}",
        doc.text
    );
    // Re-parse must succeed end-to-end including the new stubs.
    let (_resolve, _pkg, _world) = doc.resolve().expect("resolve roundtrip");
}

#[test]
fn canvas_fill_rect_lowers_to_wit_import() {
    // The host-stub block must declare the `fill-rect` method shape
    // so wit-parser accepts the world's `import mty:web/canvas;` and
    // wit-component can wire the import at link time.
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");
    assert!(
        doc.text
            .contains("fill-rect: func(x: s32, y: s32, w: u32, h: u32, color: u32);"),
        "expected fill-rect signature in stub, was: {}",
        doc.text
    );
    assert!(
        doc.text.contains("clear: func();"),
        "expected clear() in stub"
    );
    assert!(
        doc.text
            .contains("fill-text: func(text: string, x: s32, y: s32, color: u32);"),
        "expected fill-text signature"
    );
    assert!(
        doc.text.contains("request-animation-frame: func();"),
        "expected raf signature"
    );
}

#[test]
fn input_keydown_subscribes() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("subscribe-keydown: func();"),
        "expected subscribe-keydown in stub, was: {}",
        doc.text
    );
    assert!(
        doc.text.contains("subscribe-keyup: func();"),
        "expected subscribe-keyup in stub"
    );
    assert!(
        doc.text.contains("record key-event"),
        "expected key-event record in stub"
    );
}

#[test]
fn canvas_geometry_accessors_are_present() {
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");
    assert!(
        doc.text.contains("width: func() -> u32;"),
        "expected width getter, was: {}",
        doc.text
    );
    assert!(
        doc.text.contains("height: func() -> u32;"),
        "expected height getter"
    );
    assert!(
        doc.text.contains("set-fill-style: func(color: u32);"),
        "expected set-fill-style setter"
    );
}

#[test]
fn wasi_target_does_not_get_canvas_or_input() {
    // Canvas + input are a *web-target-only* surface. WASI builds
    // must not pull in the imports (they would never resolve against
    // a real WASI host).
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Wasi).expect("emit");
    assert!(
        !doc.text.contains("import mty:web/canvas;"),
        "wasi build must not import canvas, was: {}",
        doc.text
    );
    assert!(
        !doc.text.contains("import mty:web/input;"),
        "wasi build must not import input"
    );
    assert!(
        !doc.text.contains("interface canvas"),
        "wasi build must not declare canvas stub"
    );
}

#[test]
fn web_world_round_trips_with_canvas_and_input() {
    // Full end-to-end: emit, re-parse via `wit_parser::Resolve`,
    // resolve the world, all without errors.
    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");
    let (resolve, pkg, world) = doc.resolve().expect("resolve roundtrip");
    // World must exist by name.
    let world_data = &resolve.worlds[world];
    assert_eq!(world_data.name, "notetris-world");
    // Package id must match the generated header.
    let pkg_data = &resolve.packages[pkg];
    assert_eq!(pkg_data.name.namespace, "mty");
    assert_eq!(pkg_data.name.name, "notetris");
}

#[test]
fn stdlib_constants_match_emitted_wit() {
    // The Mighty-side bindings pin the WIT interface names as
    // public constants. If those drift away from the strings the
    // codegen layer emits, the whole `std.web.Canvas` surface
    // stops linking — this test catches the drift early.
    use mty_stdlib::web::canvas as cv;
    use mty_stdlib::web::input as inp;

    let prog = common::empty_main();
    let doc = emit_wit(&prog, "notetris", WasmTarget::Web).expect("emit");

    // Canvas-side: iface tag must be `mty:web/canvas@0.1` and every
    // method must show up in the emitted stub.
    for (iface, method) in [
        cv::WIT_IMPORT_CLEAR,
        cv::WIT_IMPORT_FILL_RECT,
        cv::WIT_IMPORT_STROKE_RECT,
        cv::WIT_IMPORT_FILL_TEXT,
        cv::WIT_IMPORT_SET_FILL_STYLE,
        cv::WIT_IMPORT_WIDTH,
        cv::WIT_IMPORT_HEIGHT,
        cv::WIT_IMPORT_REQUEST_ANIMATION_FRAME,
    ] {
        assert_eq!(
            iface, "mty:web/canvas@0.1",
            "canvas iface tag drifted (testing {method})"
        );
        // Method name must appear in the emitted wit text.
        let needle = format!("{method}: func");
        assert!(
            doc.text.contains(&needle),
            "canvas method {method} missing from emitted wit"
        );
    }
    // Input-side: iface tag must be `mty:web/input@0.1`.
    for (iface, method) in [
        inp::WIT_IMPORT_SUBSCRIBE_KEYDOWN,
        inp::WIT_IMPORT_SUBSCRIBE_KEYUP,
    ] {
        assert_eq!(
            iface, "mty:web/input@0.1",
            "input iface tag drifted (testing {method})"
        );
        let needle = format!("{method}: func");
        assert!(
            doc.text.contains(&needle),
            "input method {method} missing from emitted wit"
        );
    }
}

#[test]
fn key_decoder_round_trips() {
    // Track A also owns the `Key` enum used by the host-shim's
    // `keydown(k)` callback decode path. Sanity-check that the
    // round-trip is total for the cases the host emits.
    use mty_stdlib::web::Key;

    for raw in [
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        " ",
        "Enter",
        "Escape",
        "a",
        "Z",
        "F12",
    ] {
        let k = Key::from_dom_string(raw);
        let s = k.to_dom_string();
        // Round-trip via the decoder again must land on the same
        // variant (the string form may normalize, e.g. " " stays
        // " ").
        assert_eq!(
            Key::from_dom_string(&s),
            k,
            "round-trip failed for raw={raw}"
        );
    }
}
