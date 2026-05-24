//! v0.8 Task 1 — A proc-macro body that performs an effect call is
//! rejected. The static check catches the obvious case (MT6005); a
//! constructed shape that slips past the static check is caught at
//! runtime as MT6007.

use mty_ast::{AstNode, File};
use mty_macros::{expand_proc, ImpurityReason, MacroRegistry, ProcMacroResult};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn proc_macro_with_effect_call_is_rejected_statically() {
    let src = concat!(
        "proc macro leak(input: TokenStream) -> TokenStream {\n",
        "  effect.io(\"hi\")\n",
        "  input\n",
        "}\n",
    );
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("leak").unwrap();
    match expand_proc(def, &[]) {
        ProcMacroResult::Impure(ImpurityReason::EffectCall(name)) => assert_eq!(name, "io"),
        other => panic!("expected static Impure(EffectCall), got {other:?}"),
    }
}

#[test]
fn proc_macro_with_bare_impure_call_is_rejected() {
    let src = concat!(
        "proc macro tstamp(input: TokenStream) -> TokenStream {\n",
        "  time.now()\n",
        "  input\n",
        "}\n",
    );
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("tstamp").unwrap();
    match expand_proc(def, &[]) {
        ProcMacroResult::Impure(ImpurityReason::BareImpureCall(name)) => assert_eq!(name, "time"),
        other => panic!("expected static Impure(BareImpureCall), got {other:?}"),
    }
}
