//! Zero-arg macro that returns a literal expands to that literal.

#![allow(deprecated)] // exercises legacy `expand_to_source` (removal scheduled for v0.15)

use mty_ast::{AstNode, File};
use mty_macros::{expand_to_source, MacroRegistry};
use mty_syntax::SyntaxNode;

fn registry(src: &str) -> MacroRegistry {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    let file = File::cast(root).expect("FILE root");
    MacroRegistry::from_file(&file.0)
}

#[test]
fn zero_arg_literal_macro() {
    let reg = registry("macro forty_two() => { 42 }\n");
    let def = reg.get("forty_two").unwrap();
    let src = expand_to_source(def, &[], 0).unwrap();
    let stripped: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        stripped.contains("42"),
        "expansion did not include 42: {src}"
    );
}

#[test]
fn zero_arg_block_macro() {
    let reg = registry("macro greet() => { print(\"hi\") }\n");
    let def = reg.get("greet").unwrap();
    let src = expand_to_source(def, &[], 1).unwrap();
    assert!(src.contains("print"), "expansion missing print: {src}");
    assert!(src.contains("\"hi\""), "expansion missing string: {src}");
}
