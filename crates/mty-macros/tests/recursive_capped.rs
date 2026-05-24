//! A macro that recurses (directly or transitively) is capped at
//! [`MAX_EXPANSION_DEPTH`] = 32. The expander itself does NOT recurse
//! across calls — recursion accounting lives in the caller (HIR
//! lowering). This test simulates that loop and checks the limit.

use mty_ast::{AstNode, File};
use mty_macros::{expand_to_source, MacroRegistry, MAX_EXPANSION_DEPTH};
use mty_syntax::SyntaxNode;

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

#[test]
fn depth_constant_is_exactly_32() {
    // Spec lock — changing this requires a v0.4 amendment.
    assert_eq!(MAX_EXPANSION_DEPTH, 32);
}

#[test]
fn recursive_expansion_terminates_at_limit() {
    // `r(x) => r(x) + 1` would explode without a cap. We simulate the
    // outer lowering loop here: each iteration that detects another
    // macro call should re-expand, and we stop at MAX_EXPANSION_DEPTH.
    let reg = registry("macro r(x) => { r(x) + 1 }\n");
    let def = reg.get("r").unwrap();
    let mut current = String::from("x");
    let mut depth = 0u32;
    let limit = MAX_EXPANSION_DEPTH;
    while depth < limit {
        let next = expand_to_source(def, &[&current], depth).unwrap();
        // crude detect: presence of `r(` indicates another macro call.
        if !next.contains("r(") {
            break;
        }
        current = next;
        depth += 1;
    }
    assert!(
        depth >= limit,
        "loop ended before hitting depth cap: depth={depth}"
    );
}

#[test]
fn non_recursive_expansion_terminates_immediately() {
    let reg = registry("macro flat(x) => { x + 1 }\n");
    let def = reg.get("flat").unwrap();
    let out = expand_to_source(def, &["41"], 0).unwrap();
    assert!(out.contains("(41) + 1"), "got: {out}");
    // No further macro call: the lowering loop would stop after one step.
    assert!(!out.contains("flat("), "got: {out}");
}
