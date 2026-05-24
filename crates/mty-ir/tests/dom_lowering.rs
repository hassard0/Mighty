//! v0.6 easy-win 1 — verify that a method call on a `Dom` capability
//! receiver lowers to a `Call { func: BuiltinId::DomOp(name) }` rather
//! than the generic `Rvalue::MethodCall`.
//!
//! This closes the v0.5 deferral that left the wasm-side `emit_dom_call`
//! gated behind `#[allow(dead_code)]`. With the SIR variant in place
//! the wasm32-web backend can actually reach `emit_dom_call` from real
//! Mighty source; the interpreter routes the same builtin through
//! `host.extern_call("dom.<op>", ...)` for headless test runs.

mod common;

use common::compile;
use mty_ir::ir::{BuiltinId, FnRef, Rvalue, Stmt};

/// Returns every `BuiltinId::DomOp(name)` call's method-name string in
/// program order across all fns. Used to assert the lowerer emits a
/// `dom.<op>` builtin call at exactly the right place(s).
fn dom_ops_in(prog: &mty_ir::ir::Program) -> Vec<String> {
    let mut out = Vec::new();
    for f in &prog.fns {
        for blk in &f.blocks {
            for stmt in &blk.stmts {
                if let Stmt::Assign(
                    _,
                    Rvalue::Call {
                        func: FnRef::Builtin(BuiltinId::DomOp(name)),
                        ..
                    },
                ) = stmt
                {
                    out.push(name.clone());
                }
            }
        }
    }
    out
}

#[test]
fn dom_set_text_lowers_to_builtin_dom_op() {
    // A trivial program where `d: Dom` is a function parameter so the
    // type-checker resolves the receiver to `Cap { family: Dom, .. }`.
    // `d.set_text(...)` must lower to `BuiltinId::DomOp("set_text")`.
    let src = r##"
        fn paint(d: Dom) {
            d.set_text("#id", "hello")
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = dom_ops_in(&prog);
    let dump = mty_ir::dump::dump_program(&prog);
    assert!(
        ops.iter().any(|n| n == "set_text"),
        "expected `set_text` DomOp; got {ops:?}\nSIR dump:\n{dump}"
    );
}

#[test]
fn dom_multiple_methods_all_route_through_dom_op() {
    let src = r##"
        fn paint(d: Dom) {
            d.set_text("#a", "x")
            d.get_text("#b")
            d.on_click("#c", "tag")
            d.query("#d")
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = dom_ops_in(&prog);
    for want in ["set_text", "get_text", "on_click", "query"] {
        assert!(
            ops.iter().any(|n| n == want),
            "expected `{want}` DomOp; got {ops:?}"
        );
    }
}

#[test]
fn non_dom_method_call_does_not_become_dom_op() {
    // `String::push_str` (or any non-Dom receiver method) must not be
    // mis-routed through `BuiltinId::DomOp`. This is the negative
    // control: if the receiver-type detection over-fires, every test
    // that calls a string method would silently become a `dom.push_str`
    // call and the SIR would diverge from what the wasm/native backends
    // expect.
    let src = r##"
        fn main() -> I32 {
            let s = "abc"
            if s.contains("b") { 1 } else { 0 }
        }
    "##;
    let (_pkg, _typed, prog) = compile(src);
    let ops = dom_ops_in(&prog);
    assert!(
        ops.is_empty(),
        "non-Dom receiver should not produce DomOp; got {ops:?}"
    );
}
