//! v0.27 Track A: HIR-side tests for `@tool(...)` attribute lowering.
//!
//! These exercise the integration between the parser (TOOL_ATTR CST
//! node), the AST accessor (`mty_ast::ToolAttr`), and the HIR
//! preprocessor (which calls `mty_macros::expand_builtin_attribute`
//! and splices the `__tool_*` companion fns into the source).

use mty_ast::{AstNode, File};
use mty_hir::lower::macros::preprocess;
use mty_syntax::{parse, SyntaxNode};

fn lower(
    src: &str,
) -> (
    mty_hir::Package,
    Vec<mty_diagnostics::diagnostic::Diagnostic>,
) {
    let r = parse(src);
    let f = File::cast(SyntaxNode::new_root(r.green)).unwrap();
    mty_hir::lower::LoweringCtx::new().lower_file(f)
}

#[test]
fn tool_attr_attaches_to_hirfn() {
    let src = "\
@tool(\"Read a text file\", cap: fs.read)
fn read_doc(path: String) -> String { path }
";
    let (pkg, _diags) = lower(src);
    // Find the read_doc fn in the lowered package.
    let read_doc = pkg
        .fns
        .iter()
        .find(|(_, f)| f.name == "read_doc")
        .expect("read_doc lowered")
        .1;
    let attr = read_doc
        .tool_attr
        .as_ref()
        .expect("HirFn.tool_attr populated for @tool-decorated fn");
    assert_eq!(attr.description, "Read a text file");
    assert_eq!(attr.capability.as_deref(), Some("fs.read"));
}

#[test]
fn tool_attr_lacking_desc_errors() {
    // Empty arg list should produce MT6012 via the macro expander.
    let src = "@tool() fn x() {}";
    let pp = preprocess(src);
    let has_mt6012 = pp.diagnostics.iter().any(|d| d.code.0 == 6012);
    assert!(
        has_mt6012,
        "expected MT6012 in diagnostics, got: {:?}",
        pp.diagnostics
    );
}

#[test]
fn tool_attr_with_invalid_cap_expr_errors() {
    // `cap: "literal"` parses as a valid expression but isn't a
    // dotted path → MT6014 from parse_cap_arg in the macro expander.
    let src = "@tool(\"d\", cap: \"not_a_path\") fn x() {}";
    let pp = preprocess(src);
    let has_mt6014 = pp.diagnostics.iter().any(|d| d.code.0 == 6014);
    assert!(
        has_mt6014,
        "expected MT6014 in diagnostics, got: {:?}",
        pp.diagnostics
    );
}

#[test]
fn tool_attr_synthesises_companion_fns() {
    // The preprocessor must splice the descriptor + invoke + register
    // companions into the source after the user's fn.
    let src = "\
@tool(\"Read a file\", cap: fs.read)
fn read_doc(path: String) -> String { path }
";
    let pp = preprocess(src);
    assert!(
        pp.source.contains("__tool_descriptor_read_doc"),
        "missing descriptor fn in: {}",
        pp.source
    );
    assert!(
        pp.source.contains("__tool_invoke_read_doc"),
        "missing invoke fn in: {}",
        pp.source
    );
    assert!(
        pp.source.contains("__tool_register_read_doc"),
        "missing register fn in: {}",
        pp.source
    );
    // The original `fn read_doc(...)` must STILL be present (the
    // attribute is non-destructive — the user can still call the fn
    // directly).
    assert!(
        pp.source.contains("fn read_doc(path: String)"),
        "original fn missing in: {}",
        pp.source
    );
    // v0.27: the `@tool(...)` prefix stays in the source so the typed
    // AST surface (`mty_ast::ToolAttr::for_fn_decl`) still sees it at
    // HIR lowering time — the parser accepts the prefix; only the
    // companion fns are spliced.
    assert!(
        pp.source.contains("@tool("),
        "attr prefix should be preserved: {}",
        pp.source
    );
}

#[test]
fn plain_fn_without_attr_unchanged() {
    let src = "fn plain() -> String { \"hi\" }";
    let pp = preprocess(src);
    assert_eq!(pp.source, src);
    assert!(pp.diagnostics.is_empty(), "diags: {:?}", pp.diagnostics);
}
