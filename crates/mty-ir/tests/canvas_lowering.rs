//! v0.25 Track A — IR-side routing for `std.web.Canvas` method calls.
//! Closes the v0.23 → v0.24 unfinished business documented in
//! `dev/history/notes/DEMO06_CANVAS_DIRECT_V0_24_NOTES.md` §A.
//!
//! v0.24 Track A landed `BuiltinId::CanvasOp(CanvasOpKind)` in the IR
//! plus matching emitter dispatch. This file pins the lowerer-side
//! piece that connects them: when a Mighty source method call resolves
//! to a `std.web.Canvas` receiver and a canonical canvas method name,
//! the IR lowerer emits `Rvalue::Call { func:
//! FnRef::Builtin(BuiltinId::CanvasOp(kind)) }` (not a generic
//! `Rvalue::MethodCall`). Without this routing the wasm32-web emitter
//! never sees the canvas op and the `mty:web/canvas@0.1` import never
//! lands in the core module — the regression that blocked
//! `demos/06_canvas_game` from owning its pixels Mighty-side.
//!
//! Detection key (per the v0.25 design): per-fn `canvas_locals` taint
//! propagated from `std.web.Canvas.new(...)` callsites through
//! let-binding hand-offs. We don't trust the typed receiver type
//! because the type-checker stamps `Error` on the canvas handle today
//! (no `std.web` module or `Canvas` ADT in the prelude). The
//! local-tagging approach keeps the routing fix self-contained.

mod common;

use common::compile;
use mty_ir::ir::{BuiltinId, CanvasOpKind, FnRef, Program, Rvalue, Stmt};

/// Collect every `CanvasOp(kind)` builtin call in program order
/// across all fns. Mirrors `dom_ops_in` in `dom_lowering.rs`.
fn canvas_ops_in(prog: &Program) -> Vec<CanvasOpKind> {
    let mut out = Vec::new();
    for f in &prog.fns {
        for blk in &f.blocks {
            for stmt in &blk.stmts {
                if let Stmt::Assign(
                    _,
                    Rvalue::Call {
                        func: FnRef::Builtin(BuiltinId::CanvasOp(kind)),
                        ..
                    },
                ) = stmt
                {
                    out.push(*kind);
                }
            }
        }
    }
    out
}

/// Collect every generic `Rvalue::MethodCall` in program order — used
/// as a negative control to assert that the canvas routing *consumed*
/// the call (instead of leaving it on the fallback path).
fn generic_method_calls_in(prog: &Program) -> Vec<String> {
    let mut out = Vec::new();
    for f in &prog.fns {
        for blk in &f.blocks {
            for stmt in &blk.stmts {
                if let Stmt::Assign(_, Rvalue::MethodCall { method, .. }) = stmt {
                    out.push(method.clone());
                }
            }
        }
    }
    out
}

#[test]
fn canvas_fill_rect_lowers_to_builtin_canvas_op() {
    // Smallest possible repro of the v0.23 unfinished business:
    // `let canvas = std.web.Canvas.new(...); canvas.fill_rect(...)`
    // must reach `BuiltinId::CanvasOp(FillRect)` so the wasm32-web
    // emitter's existing dispatch arm (`emit_canvas_call`) actually
    // fires.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.fill_rect(0, 0, 240, 480, 487724799)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    assert!(
        ops.contains(&CanvasOpKind::FillRect),
        "expected CanvasOp(FillRect); got {ops:?}\nSIR dump:\n{dump}"
    );
}

#[test]
fn all_canvas_methods_route_through_canvas_op() {
    // Sweep every CanvasOpKind variant: each must be reachable from
    // Mighty source via the canonical snake-case method name. This
    // is the IR-level analog of the codegen-wasm
    // `canvas_all_ops_emit_at_least_one_import` test.
    let cases = [
        (CanvasOpKind::Clear, "clear", ""),
        (CanvasOpKind::FillRect, "fill_rect", "0, 0, 10, 10, 0"),
        (CanvasOpKind::StrokeRect, "stroke_rect", "0, 0, 10, 10, 0"),
        (CanvasOpKind::FillText, "fill_text", r#""hi", 12, 34, 0"#),
        (CanvasOpKind::SetFillStyle, "set_fill_style", "0"),
        (CanvasOpKind::Width, "width", ""),
        (CanvasOpKind::Height, "height", ""),
        (
            CanvasOpKind::RequestAnimationFrame,
            "request_animation_frame",
            "",
        ),
    ];
    for (want, method, args) in cases {
        let src = format!(
            r##"
            fn main() {{
              let canvas = std.web.Canvas.new(240, 480)
              canvas.{method}({args})
            }}
        "##
        );
        let (_pkg, _typed, prog) = compile(&src);
        let ops = canvas_ops_in(&prog);
        assert!(
            ops.contains(&want),
            "method `{method}` did not route to CanvasOp({want:?}); got {ops:?}"
        );
    }
}

#[test]
fn canvas_method_with_unknown_name_falls_back_to_generic_method_call() {
    // `canvas.bogus_method(...)` — unknown method on a canvas
    // handle. Must fall through to `Rvalue::MethodCall` so the
    // type-checker / interpreter can surface a proper diagnostic.
    // The routing fix must NOT panic or misroute non-canonical
    // method names.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.bogus_method(1, 2, 3)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    assert!(
        ops.is_empty(),
        "bogus_method must not route through CanvasOp; got {ops:?}"
    );
    // Negative control: still emitted as a generic method call.
    let methods = generic_method_calls_in(&prog);
    assert!(
        methods.iter().any(|m| m == "bogus_method"),
        "bogus_method must fall through to generic MethodCall; got {methods:?}"
    );
}

#[test]
fn non_canvas_receiver_does_not_route_through_canvas_op() {
    // Negative control: a non-canvas local with a method named
    // `fill_rect` must NOT mis-route through `CanvasOp(FillRect)`.
    // The `is_canvas_handle_receiver` predicate guards against
    // name-only matching.
    let src = r##"
        fn paint() {
          let widget = make_widget()
          widget.fill_rect(0, 0, 10, 10, 0)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    assert!(
        ops.is_empty(),
        "non-canvas receiver must not produce CanvasOp; got {ops:?}"
    );
}

#[test]
fn canvas_local_taint_propagates_through_let_rebind() {
    // `let c2 = canvas` rebinding must preserve the canvas-handle
    // taint; subsequent `c2.fill_rect(...)` still routes through
    // `CanvasOp(FillRect)`. Without the bind-pat propagation in
    // `stmts::bind_pat_assign`, the taint dies at the rebind site
    // and the second call falls through to the generic path.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          let c2 = canvas
          c2.fill_rect(0, 0, 10, 10, 0)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    assert!(
        ops.contains(&CanvasOpKind::FillRect),
        "let-rebind must propagate canvas taint; got {ops:?}"
    );
}

#[test]
fn canvas_multiple_calls_each_route_through_canvas_op() {
    // Multiple method calls on the same canvas handle must each
    // independently lower to a `CanvasOp(...)` builtin call. The
    // wasm32-web emitter's `predeclare_canvas_imports` then dedupes
    // them at the import-section level.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.clear()
          canvas.fill_rect(0, 0, 10, 10, 0)
          canvas.fill_rect(0, 10, 10, 10, 0)
          canvas.fill_text("score", 4, 16, 0)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    assert!(
        ops.contains(&CanvasOpKind::Clear),
        "missing Clear; got {ops:?}"
    );
    let fill_rect_count = ops.iter().filter(|k| **k == CanvasOpKind::FillRect).count();
    assert_eq!(fill_rect_count, 2, "expected two FillRect ops; got {ops:?}");
    assert!(
        ops.contains(&CanvasOpKind::FillText),
        "missing FillText; got {ops:?}"
    );
}

#[test]
fn hir_canvas_call_lowers_to_ir_builtin_end_to_end() {
    // End-to-end sanity check matching the spec's "HIR canvas call
    // lowers to IR builtin" requirement: write Mighty source, walk
    // the lowered SIR, assert the BuiltinId::CanvasOp variant.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.clear()
          canvas.set_fill_style(305419896)
          canvas.fill_rect(0, 0, 240, 480, 487724799)
          canvas.request_animation_frame()
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    // Order matters — must appear in source order.
    assert_eq!(
        ops,
        vec![
            CanvasOpKind::Clear,
            CanvasOpKind::SetFillStyle,
            CanvasOpKind::FillRect,
            CanvasOpKind::RequestAnimationFrame
        ],
        "canvas op sequence drifted from source order"
    );
}
