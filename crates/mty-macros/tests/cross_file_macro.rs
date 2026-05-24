//! v0.5: cross-file `pub macro` import.
//!
//! Verifies that `PackageMacros::register_use` lifts an exported macro
//! from the exporter's package into the importer's local registry.
//! Two-file fixture: exporter declares `pub macro greet()`, importer
//! gets it merged via `register_use`. Real package-aware resolution
//! lands when mty-pkg wires its symbol table into HIR lowering.

use mty_ast::{AstNode, File};
use mty_macros::{expand_to_source, MacroKind, PackageMacros};
use mty_syntax::SyntaxNode;

fn parse_file(src: &str) -> SyntaxNode {
    let p = mty_syntax::parse(src);
    let root = SyntaxNode::new_root(p.green);
    File::cast(root).unwrap().0
}

#[test]
fn exporter_isolates_pub_vs_private() {
    let src = concat!(
        "macro priv_helper() => { 1 }\n",
        "pub macro greet() => { print(\"hi\") }\n",
    );
    let file = parse_file(src);
    let pm = PackageMacros::from_file(&file);
    // Both visible locally.
    assert!(pm.local.contains("priv_helper"));
    assert!(pm.local.contains("greet"));
    // Only `greet` is exported.
    assert!(!pm.exported.contains("priv_helper"));
    assert!(pm.exported.contains("greet"));
}

#[test]
fn importer_pulls_in_exported_macros() {
    let exporter_src = "pub macro greet() => { print(\"hi\") }\n";
    let exporter = PackageMacros::from_file(&parse_file(exporter_src));

    let importer_src = "fn main() -> i32 { 0 }\n";
    let mut importer = PackageMacros::from_file(&parse_file(importer_src));
    assert!(!importer.local.contains("greet"));

    importer.register_use(&exporter, &[]);
    assert!(
        importer.local.contains("greet"),
        "register_use should have lifted `greet` into local"
    );

    // The imported macro is usable: expand it.
    let def = importer.local.get("greet").unwrap();
    let s = expand_to_source(def, &[], 1).unwrap();
    assert!(s.contains("print"), "expansion: {s}");
}

#[test]
fn private_macro_does_not_leak_across_files() {
    let exporter_src = "macro secret() => { 42 }\n";
    let exporter = PackageMacros::from_file(&parse_file(exporter_src));

    let mut importer = PackageMacros::new();
    importer.register_use(&exporter, &[]);
    assert!(
        !importer.local.contains("secret"),
        "private macro must NOT be exported"
    );
}

#[test]
fn import_with_alias_rebinds_name() {
    let exporter_src = "pub macro greet() => { 1 }\n";
    let exporter = PackageMacros::from_file(&parse_file(exporter_src));

    let mut importer = PackageMacros::new();
    assert!(importer.register_use_one(&exporter, "greet", "hello"));
    assert!(importer.local.contains("hello"));
    assert!(!importer.local.contains("greet"));
}

#[test]
fn alias_map_renames_during_bulk_use() {
    let exporter_src = concat!("pub macro a() => { 1 }\n", "pub macro b() => { 2 }\n",);
    let exporter = PackageMacros::from_file(&parse_file(exporter_src));

    let mut importer = PackageMacros::new();
    importer.register_use(&exporter, &[("a".to_string(), "alpha".to_string())]);
    assert!(importer.local.contains("alpha"));
    assert!(importer.local.contains("b")); // un-aliased: keeps original name
    assert!(!importer.local.contains("a"));
}

#[test]
fn exported_proc_macros_also_carry_through() {
    let exporter_src = "pub proc macro identity(input: TokenStream) -> TokenStream { input }\n";
    let exporter = PackageMacros::from_file(&parse_file(exporter_src));
    assert!(exporter.exported.contains("identity"));
    let def = exporter.exported.get("identity").unwrap();
    assert_eq!(def.kind, MacroKind::Procedural);

    let mut importer = PackageMacros::new();
    importer.register_use(&exporter, &[]);
    let imported = importer.local.get("identity").expect("imported");
    assert_eq!(imported.kind, MacroKind::Procedural);
}
