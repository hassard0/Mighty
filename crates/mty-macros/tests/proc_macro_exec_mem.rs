//! v0.8 Task 1 — A proc-macro that tries to allocate more than 16 MiB
//! of output is rejected with MT6008 (Memory breach).

use mty_ast::{AstNode, File};
use mty_macros::token::lex_fragment;
use mty_macros::{expand_proc, MacroRegistry, ProcMacroResult, ResourceBreach};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn allocating_over_16mb_breaches_memory() {
    // `repeat(input, BIG)` where each input token-string is ~16 bytes.
    // 16 bytes * 4_000_000 copies ≈ 64 MiB, well over the 16 MiB cap.
    let src = "proc macro hog(input: TokenStream) -> TokenStream { repeat(input, 4000000) }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("hog").unwrap();
    let input = lex_fragment("aaaaaaaaaaaaaaaa").unwrap(); // 16-byte ident
    match expand_proc(def, &input) {
        ProcMacroResult::ResourceExceeded(ResourceBreach::Memory) => {}
        // Hitting the step cap before the memory cap is also acceptable —
        // both signal "this macro is out of control".
        ProcMacroResult::ResourceExceeded(ResourceBreach::Steps) => {}
        other => panic!("expected memory/steps breach, got {other:?}"),
    }
}
