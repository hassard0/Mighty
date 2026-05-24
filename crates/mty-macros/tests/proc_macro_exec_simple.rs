//! v0.8 Task 1 — Proc macro that doubles its token-stream input via the
//! sandbox's `concat(input, input)` DSL primitive.

use mty_ast::{AstNode, File};
use mty_macros::{expand_proc, MacroRegistry, ProcMacroResult};
use mty_macros::token::{lex_fragment, tokens_to_source};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn identity_proc_macro_returns_input() {
    let src = "proc macro identity(input: TokenStream) -> TokenStream { input }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("identity").unwrap();
    let input = lex_fragment("hello").unwrap();
    match expand_proc(def, &input) {
        ProcMacroResult::Ok(out) => {
            let s = tokens_to_source(&out);
            assert!(s.contains("hello"), "expected identity to emit `hello`, got: {s:?}");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn double_proc_macro_via_concat() {
    let src = "proc macro double(input: TokenStream) -> TokenStream { concat(input, input) }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("double").unwrap();
    let input = lex_fragment("X").unwrap();
    match expand_proc(def, &input) {
        ProcMacroResult::Ok(out) => {
            let s = tokens_to_source(&out);
            // Two `X` tokens back to back.
            assert!(s.matches('X').count() >= 2, "expected `X` twice in: {s:?}");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}

#[test]
fn repeat_proc_macro_emits_n_copies() {
    let src = "proc macro three(input: TokenStream) -> TokenStream { repeat(input, 3) }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("three").unwrap();
    let input = lex_fragment("Y").unwrap();
    match expand_proc(def, &input) {
        ProcMacroResult::Ok(out) => {
            let s = tokens_to_source(&out);
            assert_eq!(s.matches('Y').count(), 3, "expected 3 Y's, got: {s:?}");
        }
        other => panic!("expected Ok, got {other:?}"),
    }
}
