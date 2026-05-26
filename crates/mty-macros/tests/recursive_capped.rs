//! A macro that recurses (directly or transitively) is capped at
//! [`MAX_EXPANSION_DEPTH`] = 32. The expander itself does NOT recurse
//! across calls — recursion accounting lives in the caller (HIR
//! lowering). This test simulates that loop and checks the limit.
//!
//! v0.15 migration: uses the set-of-scopes expander
//! (`expand_scoped_to_source`) — the legacy `expand_to_source` was
//! deleted in v0.15. The depth-cap behavior is identical; the only
//! shape change is that the per-invocation context comes from a
//! `ScopeGen` rather than an explicit `ctx` argument.

use mty_ast::{AstNode, File};
use mty_macros::{expand_scoped_to_source, MacroRegistry, ScopeGen, Scopes, MAX_EXPANSION_DEPTH};
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
    let mut gen = ScopeGen::new();
    while depth < limit {
        let (next, _exp) = expand_scoped_to_source(
            def,
            &[current.as_str()],
            &mut gen,
            Scopes::empty(),
            Scopes::empty(),
        )
        .unwrap();
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
    let mut gen = ScopeGen::new();
    let (out, _exp) =
        expand_scoped_to_source(def, &["41"], &mut gen, Scopes::empty(), Scopes::empty()).unwrap();
    assert!(out.contains("(41) + 1"), "got: {out}");
    // No further macro call: the lowering loop would stop after one step.
    assert!(!out.contains("flat("), "got: {out}");
}
