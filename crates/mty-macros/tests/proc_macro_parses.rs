//! v0.5: procedural macros parse + register, even though execution is
//! gated behind MT6006.
//!
//! These tests assert that:
//!   * `proc macro Name(input: TokenStream) -> TokenStream { body }`
//!     parses without parser errors.
//!   * The registry records it as `MacroKind::Procedural`.
//!   * Calling `expand_proc` on a pure body returns `Unsupported`
//!     (MT6006 territory).
//!   * Calling `expand_proc` on an impure body returns `Impure`
//!     (MT6005 territory).
//!   * The `pub` modifier puts a proc macro into the exported set.

use mty_ast::{AstNode, File};
use mty_macros::{
    expand_proc, ImpurityReason, MacroKind, MacroRegistry, PackageMacros, ProcMacroResult,
};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn pure_proc_macro_decl_parses_and_registers() {
    let src = "proc macro identity(input: TokenStream) -> TokenStream { input }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("identity").expect("identity registered");
    assert_eq!(def.kind, MacroKind::Procedural);
    // Param name should be captured as the input.
    assert_eq!(def.params, vec!["input".to_string()]);
}

#[test]
fn pub_proc_macro_lands_in_exported_set() {
    let src = "pub proc macro upcase(input: TokenStream) -> TokenStream { input }\n";
    let file = parse_file(src);
    let pm = PackageMacros::from_file(&file);
    assert!(pm.local.contains("upcase"));
    assert!(pm.exported.contains("upcase"));
}

#[test]
fn expand_proc_pure_returns_ok_v0_8() {
    // v0.8: the sandboxed interpreter is enabled, so a pure
    // identity-shaped body successfully expands to a (possibly empty)
    // token stream. The legacy v0.5 `Unsupported` arm is retained as a
    // back-compat enum variant but is no longer returned for runnable
    // bodies.
    let src = "proc macro identity(input: TokenStream) -> TokenStream { input }\n";
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("identity").unwrap();
    match expand_proc(def, &[]) {
        ProcMacroResult::Ok(_) => {}
        other => panic!("expected Ok, got: {:?}", other),
    }
}

#[test]
fn expand_proc_impure_returns_impure_with_reason() {
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
        ProcMacroResult::Impure(ImpurityReason::EffectCall(name)) => {
            assert_eq!(name, "io");
        }
        other => panic!("expected EffectCall impurity, got: {:?}", other),
    }
}

#[test]
fn expand_proc_detects_bare_time_call() {
    let src = concat!(
        "proc macro stamp(input: TokenStream) -> TokenStream {\n",
        "  let t = time.now()\n",
        "  input\n",
        "}\n",
    );
    let file = parse_file(src);
    let reg = MacroRegistry::from_file(&file);
    let def = reg.get("stamp").unwrap();
    match expand_proc(def, &[]) {
        ProcMacroResult::Impure(ImpurityReason::BareImpureCall(name)) => {
            assert_eq!(name, "time");
        }
        other => panic!("expected BareImpureCall impurity, got: {:?}", other),
    }
}

#[test]
fn proc_macro_call_site_parses_as_macro_call() {
    // Even though the proc macro body can't run yet, the call site
    // (`id!(42)`) is still a syntactically valid MACRO_CALL.
    use mty_syntax::SyntaxKind;
    let src = concat!(
        "proc macro id(input: TokenStream) -> TokenStream { input }\n",
        "fn main() -> i32 { id!(42); 0 }\n",
    );
    let file = parse_file(src);
    let has_macro_call = file
        .descendants()
        .any(|n| n.kind() == SyntaxKind::MACRO_CALL);
    assert!(has_macro_call);
}
