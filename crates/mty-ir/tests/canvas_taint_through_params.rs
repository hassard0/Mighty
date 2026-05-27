//! v0.26 Track D — `std.web.Canvas` handle routing through fn params.
//!
//! v0.25 Track A pinned the per-fn `canvas_locals` taint set: any
//! local initialised from `std.web.Canvas.new(...)` (or rebound from
//! another marked local via `let canvas = c`) routes
//! `canvas.fill_rect(...)` to `BuiltinId::CanvasOp(FillRect)`. The
//! "canvas handle taint" predicate
//! (`is_canvas_handle_receiver` in `crates/mty-ir/src/lower/exprs.rs`)
//! only checked the per-fn set, which never gained a fn-parameter
//! entry: passing a Canvas handle as a fn argument silently dropped
//! the routing and the call fell through to a generic
//! `Rvalue::MethodCall` (eventually emitting an empty user-fn body
//! on the wasm target). The v0.25 Track F notes documented this as
//! the headline v0.26 gap §A.
//!
//! v0.26 Track D closes the gap at the fn-decl boundary: when the
//! source-level param type is `std.web.Canvas`, `lower_one_fn` marks
//! the new SIR `Local` in `canvas_locals` so the existing routing
//! predicate keeps working transparently. The shape detection lives
//! in `lower::items::is_std_web_canvas_type` and accepts both the
//! canonical multi-segment path (`std.web.Canvas`) AND a single-
//! segment `Canvas` shorthand (so a future `use std::web::Canvas`
//! lands without a follow-up regression).
//!
//! Tests below pin:
//!
//! 1. `canvas_param_handle_routes_to_builtin` — the headline:
//!    `fn helper(c: std.web.Canvas) { c.fill_rect(...) }` lowers to
//!    `BuiltinId::CanvasOp(FillRect)`.
//! 2. `canvas_param_in_chain` — passing canvas through TWO fn
//!    boundaries (`main → outer(canvas) → inner(canvas)`) still
//!    routes the deepest `fill_rect` call through CanvasOp.
//! 3. `nested_method_call_on_canvas_param` — using `c.width()` /
//!    `c.height()` as arguments to a nested `c.fill_rect(...)` (the
//!    pattern the canvas-game's `render_grid` helper uses) routes
//!    every method through CanvasOp.
//! 4. `inline_canvas_local_still_works` — backstop: the original
//!    v0.25 surface (`let canvas = std.web.Canvas.new(W, H);
//!    canvas.fill_rect(...)`) keeps routing through CanvasOp. The
//!    v0.26 param-marking must not regress the inline-local path.
//! 5. `canvas_borrow_param_routes_too` — `fn helper(c: &std.web.Canvas)`
//!    also routes through CanvasOp (the `Borrow` HIR wrapper unwraps
//!    in `is_std_web_canvas_type`).
//!
//! See `dev/history/notes/DEMO06_V2_V0_25_NOTES.md` §A for the v0.25
//! workaround (re-acquire the handle inline in every callback) and
//! `dev/history/notes/V025_CLEANUP_V0_26_NOTES.md` for the v0.26
//! closing slice.

mod common;

use common::compile;
use mty_ir::ir::{BuiltinId, CanvasOpKind, FnRef, Program, Rvalue, Stmt};

/// Collect every `BuiltinId::CanvasOp(kind)` builtin call in program
/// order across all fns. Mirrors `canvas_ops_in` in
/// `canvas_lowering.rs` so the two tests can grow side-by-side.
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

/// Count every generic `Rvalue::MethodCall` in `prog`. Used as a
/// negative control: a successful canvas-routing fix should EMIT
/// CanvasOp builtins AND avoid leaving a parallel generic MethodCall
/// on the fallback path.
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
fn canvas_param_handle_routes_to_builtin() {
    // The headline gap: a helper that takes `c: std.web.Canvas` as a
    // parameter and immediately calls `c.fill_rect(...)`. Pre-v0.26
    // Track D the call fell through to `Rvalue::MethodCall` because
    // `canvas_locals` only ever held inline-constructed Canvas
    // handles; the param-marking pass closes the loop.
    let src = r##"
        fn helper(c: std.web.Canvas) {
          c.fill_rect(0, 0, 100, 100, 487724799)
        }

        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          helper(canvas)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    assert!(
        ops.contains(&CanvasOpKind::FillRect),
        "expected CanvasOp(FillRect) from helper(c: std.web.Canvas); \
         got {ops:?}\n\nSIR dump:\n{dump}"
    );
    let generic = generic_method_calls_in(&prog);
    assert!(
        !generic.iter().any(|m| m == "fill_rect"),
        "fill_rect must not leak through the generic MethodCall path; \
         got {generic:?}\n\nSIR dump:\n{dump}"
    );
}

#[test]
fn canvas_param_in_chain() {
    // Chain of fn calls: main -> outer(canvas) -> inner(canvas). The
    // deepest fill_rect call must still resolve through CanvasOp.
    // Demonstrates the closure works at any fn-call nesting depth.
    let src = r##"
        fn inner(c: std.web.Canvas) {
          c.fill_rect(10, 10, 50, 50, 487724799)
        }

        fn outer(c: std.web.Canvas) {
          inner(c)
          c.fill_rect(20, 20, 30, 30, 487724799)
        }

        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          outer(canvas)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    // Both fill_rect calls must land in the CanvasOp list (one from
    // inner, one from outer).
    let fill_rect_count = ops
        .iter()
        .filter(|k| matches!(k, CanvasOpKind::FillRect))
        .count();
    assert!(
        fill_rect_count >= 2,
        "expected at least 2 CanvasOp(FillRect) calls (one per helper); \
         got {fill_rect_count} in {ops:?}\n\nSIR dump:\n{dump}"
    );
}

#[test]
fn nested_method_call_on_canvas_param() {
    // The pattern the canvas-game `render_grid(canvas)` helper uses:
    // `c.fill_rect(c.width(), 0, 100, 100, color)`. Both the outer
    // fill_rect AND the inner width() call must route through
    // CanvasOp (width takes no args; the issue is the receiver
    // matching).
    let src = r##"
        fn render_grid(c: std.web.Canvas) {
          let w = c.width()
          c.fill_rect(0, 0, w, 100, 487724799)
        }

        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          render_grid(canvas)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    assert!(
        ops.contains(&CanvasOpKind::FillRect),
        "fill_rect on canvas param must route through CanvasOp; \
         ops = {ops:?}\n\nSIR dump:\n{dump}"
    );
    assert!(
        ops.contains(&CanvasOpKind::Width),
        "width() on canvas param must route through CanvasOp; \
         ops = {ops:?}\n\nSIR dump:\n{dump}"
    );
}

#[test]
fn inline_canvas_local_still_works() {
    // Regression backstop: the v0.25 inline-construction path
    // (`let canvas = std.web.Canvas.new(...); canvas.fill_rect(...)`)
    // must keep routing through CanvasOp. The v0.26 param-marking
    // must NOT regress the v0.25 surface.
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
        "v0.25 inline-local path regressed; ops = {ops:?}\n\nSIR dump:\n{dump}"
    );
}

#[test]
fn canvas_borrow_param_routes_too() {
    // `fn helper(c: &std.web.Canvas)` — the borrow wrapper around
    // the canvas type must NOT defeat the routing. `Borrow` is
    // unwrapped by `is_std_web_canvas_type`. The Mighty source for
    // this shape is uncommon today (most canvas usages are by-value
    // because the host treats it as a handle), but pinning the borrow
    // path keeps the helper future-proof for the canvas-cap design
    // the v0.27 borrow-checker integration will need.
    let src = r##"
        fn helper(c: &std.web.Canvas) {
          c.fill_rect(0, 0, 100, 100, 487724799)
        }

        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          helper(&canvas)
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = canvas_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    assert!(
        ops.contains(&CanvasOpKind::FillRect),
        "fill_rect on &std.web.Canvas param must route through CanvasOp; \
         ops = {ops:?}\n\nSIR dump:\n{dump}"
    );
}
