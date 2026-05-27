//! v0.25 Track A — HIR-side guarantees the IR canvas-routing fix
//! depends on.
//!
//! The IR layer (`crates/mty-ir/src/lower/exprs.rs`) routes
//! `canvas.fill_rect(...)` to `BuiltinId::CanvasOp(FillRect)`. The
//! HIR layer doesn't carry builtin info, but the IR pattern-match
//! depends on a stable HIR call shape: when source code says
//! `canvas.fill_rect(...)` the parser+HIR produce
//! `HirExpr::Call { callee: HirExpr::Path(["canvas", "fill_rect"]), args }`
//! (the "local.method(args)" parse shape) — NOT a `MethodCall` node,
//! despite the dot syntax. The IR side handles both shapes (the
//! `MethodCall` arm for chained receivers like `(expr).method(...)`
//! and the `lower_call` arm for `local.method(...)`); this file pins
//! the dominant shape so drift is loud.
//!
//! Mirrors the pattern of the existing DOM tests in
//! `crates/mty-ir/tests/dom_lowering.rs` but at the HIR tier (no
//! type info needed). Closes the v0.23 → v0.24 unfinished business
//! documented in
//! `dev/history/notes/CANVAS_HIR_ROUTING_V0_25_NOTES.md`.

use mty_ast::{AstNode, File};
use mty_hir::{HirExpr, HirStmt, Package};
use mty_syntax::{parse, SyntaxNode};

/// The eight canonical `mty:web/canvas@0.1` method names (snake_case
/// as they appear in Mighty source). Pinned by
/// `crates/mty-ir/src/ir.rs::CanvasOpKind::as_snake` and consumed by
/// `crates/mty-ir/src/lower/exprs.rs::canvas_op_for_method`.
const CANVAS_METHODS: &[&str] = &[
    "clear",
    "fill_rect",
    "stroke_rect",
    "fill_text",
    "set_fill_style",
    "width",
    "height",
    "request_animation_frame",
];

fn lower(src: &str) -> Package {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).expect("parse");
    let (pkg, _diags) = mty_hir::lower::LoweringCtx::new().lower_file(f);
    pkg
}

/// Collect every `(local, method)` pair from `Call(Path([local, method]))`
/// shaped HIR calls AND every `MethodCall { method, .. }` chained
/// receiver. Returns the trailing method name from each pattern.
fn dot_call_method_names(pkg: &Package) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in pkg.fns.values() {
        let Some(body_id) = f.body else { continue };
        let body = &pkg.blocks[body_id];
        collect_block(pkg, body, &mut out);
    }
    out
}

fn collect_block(pkg: &Package, b: &mty_hir::HirBlock, out: &mut Vec<String>) {
    for s in &b.stmts {
        match s {
            HirStmt::Let { init: Some(e), .. } => collect_expr(pkg, *e, out),
            HirStmt::Expr(e) => collect_expr(pkg, *e, out),
            _ => {}
        }
    }
    if let Some(t) = b.tail {
        collect_expr(pkg, t, out);
    }
}

fn collect_expr(pkg: &Package, eid: mty_hir::ExprId, out: &mut Vec<String>) {
    match &pkg.exprs[eid] {
        // Chained-receiver shape: `(some.expr).method(args)`.
        HirExpr::MethodCall {
            receiver,
            method,
            args,
        } => {
            out.push(method.clone());
            collect_expr(pkg, *receiver, out);
            for a in args {
                collect_expr(pkg, a.value, out);
            }
        }
        // `local.method(args)` parse shape: lowers to
        // `Call(Path([local, method]))`. The IR's `lower_call`
        // detects this and dispatches.
        HirExpr::Call { callee, args } => {
            if let HirExpr::Path(segs) = &pkg.exprs[*callee] {
                if segs.len() >= 2 {
                    out.push(segs.last().unwrap().clone());
                }
            }
            collect_expr(pkg, *callee, out);
            for a in args {
                collect_expr(pkg, a.value, out);
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            collect_expr(pkg, *lhs, out);
            collect_expr(pkg, *rhs, out);
        }
        HirExpr::Unary { rhs, .. } => collect_expr(pkg, *rhs, out),
        HirExpr::Field { receiver, .. } => collect_expr(pkg, *receiver, out),
        HirExpr::Borrow { inner, .. } => collect_expr(pkg, *inner, out),
        _ => {}
    }
}

#[test]
fn canvas_fill_rect_lowers_as_dot_call() {
    // `canvas.fill_rect(...)` must surface in HIR as one of the two
    // canonical dot-call shapes (the parser today produces
    // `Call(Path([canvas, fill_rect]))` for this exact text; for
    // chained receivers like `(expr).fill_rect(...)` it produces
    // `MethodCall`). Either way the trailing method name must be
    // `fill_rect` literally — the IR-side `canvas_op_for_method`
    // lookup pattern-matches on this string.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.fill_rect(0, 0, 240, 480, 487724799)
        }
    "##;
    let pkg = lower(src);
    let names = dot_call_method_names(&pkg);
    assert!(
        names.iter().any(|n| n == "fill_rect"),
        "expected `fill_rect` dot-call in HIR; got methods: {names:?}"
    );
}

#[test]
fn all_canvas_methods_lower_as_dot_calls() {
    // Sweep every canonical canvas op. Each must surface in HIR via
    // the dot-call shape with the snake-case method name verbatim.
    for m in CANVAS_METHODS {
        let args = match *m {
            "clear" | "request_animation_frame" | "width" | "height" => "".to_string(),
            "fill_rect" | "stroke_rect" => "0, 0, 240, 480, 0".to_string(),
            "fill_text" => r#""hi", 12, 34, 0"#.to_string(),
            "set_fill_style" => "0".to_string(),
            _ => "".to_string(),
        };
        let src = format!(
            r##"
            fn main() {{
              let canvas = std.web.Canvas.new(240, 480)
              canvas.{m}({args})
            }}
        "##
        );
        let pkg = lower(&src);
        let names = dot_call_method_names(&pkg);
        assert!(
            names.iter().any(|n| n == m),
            "expected canvas method `{m}` in HIR dot-call; got {names:?}"
        );
    }
}

#[test]
fn canvas_method_unknown_does_not_panic_in_hir() {
    // A typo / unsupported method name must NOT crash the HIR
    // lowerer. The dot-call shape still gets emitted with the bogus
    // method name — the IR layer's `canvas_op_for_method` returns
    // `None`, the call falls through to the generic
    // `Rvalue::MethodCall` dispatch, and the type-checker /
    // interpreter surface a sensible "method not found" downstream.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(240, 480)
          canvas.bogus_method(1, 2, 3)
        }
    "##;
    let pkg = lower(src);
    let names = dot_call_method_names(&pkg);
    assert!(
        names.iter().any(|n| n == "bogus_method"),
        "expected `bogus_method` dot-call in HIR; got {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "fill_rect"),
        "bogus_method must not be aliased onto fill_rect"
    );
}

#[test]
fn non_canvas_receiver_with_canvas_method_name_still_dot_call() {
    // Negative control: a dot-call to a method named `fill_rect` on
    // something that isn't a canvas must still lower the same way —
    // the HIR layer can't (and shouldn't) distinguish. The IR
    // layer's `is_canvas_handle_receiver` check is what prevents the
    // mis-routing; here we just pin that the HIR shape is the same
    // either way.
    let src = r##"
        fn paint(x: I32) {
          let widget = make_widget()
          widget.fill_rect(0, 0, 10, 10, 0)
        }
    "##;
    let pkg = lower(src);
    let names = dot_call_method_names(&pkg);
    assert!(
        names.iter().any(|n| n == "fill_rect"),
        "expected `fill_rect` dot-call in HIR; got {names:?}"
    );
}

#[test]
fn canvas_constructor_path_is_preserved() {
    // The IR-side constructor detection (`CANVAS_CONSTRUCTOR_PATH`)
    // matches against `std.web.Canvas.new` as a dotted path. The HIR
    // must preserve the full path so the IR's `lower_call` arm sees
    // every segment. Without this, the canvas-handle tagging never
    // fires and the routing fix is dead.
    let src = r##"
        fn main() {
          let canvas = std.web.Canvas.new(640, 480)
        }
    "##;
    let pkg = lower(src);
    // Walk the package and look for a Call whose callee is a
    // 4-segment path ending in `new`.
    let mut found = false;
    for f in pkg.fns.values() {
        let Some(body_id) = f.body else { continue };
        let body = &pkg.blocks[body_id];
        for s in &body.stmts {
            if let HirStmt::Let { init: Some(e), .. } = s {
                let e = *e;
                if let HirExpr::Call { callee, .. } = pkg.exprs[e].clone() {
                    if let HirExpr::Path(segs) = pkg.exprs[callee].clone() {
                        let want = ["std", "web", "Canvas", "new"];
                        if segs.len() == want.len()
                            && segs.iter().zip(want.iter()).all(|(a, b)| a == *b)
                        {
                            found = true;
                        }
                    }
                }
            }
        }
    }
    assert!(
        found,
        "expected callee path [std,web,Canvas,new] in HIR; package fn count: {}",
        pkg.fns.len()
    );
}
