//! v0.8 Task 1 — Runaway proc macro hits the wall-clock cap and is
//! reported as MT6008 (resource-exceeded).

use mty_ast::{AstNode, File};
use mty_macros::{expand_proc, MacroRegistry, ProcMacroResult, ResourceBreach};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn runaway_proc_macro_breaches_wall_or_steps() {
    // `while true { … }` triggers the spin-loop branch in the
    // interpreter; the worker is cancelled by either the step or the
    // wall budget.
    let src = "proc macro spin(input: TokenStream) -> TokenStream { while true { } }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("spin").unwrap();
    let started = std::time::Instant::now();
    let r = expand_proc(def, &[]);
    let elapsed = started.elapsed();
    // The wall-clock watcher allows a tiny grace window; in practice the
    // hard cap is ~150ms (100ms timeout + 50ms grace).
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "runaway sandbox didn't return inside 500ms: took {elapsed:?}"
    );
    match r {
        ProcMacroResult::ResourceExceeded(ResourceBreach::Wall)
        | ProcMacroResult::ResourceExceeded(ResourceBreach::Steps) => {}
        other => panic!("expected wall/step breach, got {other:?}"),
    }
}
