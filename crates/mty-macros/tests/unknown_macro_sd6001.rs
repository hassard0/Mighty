//! v0.5: SD6001 unknown_macro finally fires.
//!
//! Verifies the parse layer's view: a `Name!(args)` MACRO_CALL with no
//! matching MacroDef in the registry. Diagnostic emission itself lives
//! in mty-hir's lowering integration; here we assert the registry
//! correctly fails to resolve the name.

use mty_ast::{AstNode, File};
use mty_macros::MacroRegistry;
use mty_syntax::{SyntaxKind, SyntaxNode};

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn unknown_macro_name_does_not_resolve() {
    let src = "fn main() -> i32 { nonexistent!(x); 0 }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    // No `macro nonexistent(...)` decl, so the registry is empty.
    assert!(reg.is_empty());
    // The MACRO_CALL node is still present (parsed cleanly); resolution
    // is the next step.
    let mac_call = file
        .descendants()
        .find(|n| n.kind() == SyntaxKind::MACRO_CALL);
    assert!(mac_call.is_some(), "MACRO_CALL should still parse");
}

#[test]
fn known_macro_name_resolves() {
    let src = concat!(
        "macro shout(x) => { x + 1 }\n",
        "fn main() -> i32 { shout!(0); 0 }\n",
    );
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    assert!(reg.get("shout").is_some());
}

#[test]
fn typo_in_macro_name_does_not_match() {
    // Catches accidental shadowing / typo: `assert_eq` declared,
    // `assert_eq!` called (with typo `assert_qe!`).
    let src = concat!(
        "macro assert_eq(a, b) => { if a != b { panic(\"x\") } }\n",
        "fn main() -> i32 { assert_qe!(1, 1); 0 }\n",
    );
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    assert!(reg.get("assert_qe").is_none(), "typo must not resolve");
    assert!(reg.get("assert_eq").is_some());
}
