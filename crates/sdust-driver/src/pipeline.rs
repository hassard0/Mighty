use sdust_ast::{AstNode, File};
use sdust_diagnostics::{codes::UNEXPECTED_TOKEN, Diagnostic, Label};
use sdust_hir::Package;
use sdust_syntax::parse;

pub struct ParsedFile {
    pub source: String,
    pub source_id: String,
    pub green: rowan::GreenNode,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_source(source: String, source_id: String) -> ParsedFile {
    let r = parse(&source);
    let diagnostics = r
        .errors
        .iter()
        .map(|e| {
            Diagnostic::error(
                UNEXPECTED_TOKEN,
                Label {
                    start: e.start,
                    end: e.end,
                    message: e.message.clone(),
                },
            )
        })
        .collect();
    ParsedFile {
        source,
        source_id,
        green: r.green,
        diagnostics,
    }
}

pub fn lower(p: &ParsedFile) -> (Package, Vec<Diagnostic>) {
    let file = File::cast(sdust_syntax::SyntaxNode::new_root(p.green.clone())).expect("FILE root");
    let (pkg, diag) = sdust_hir::lower::LoweringCtx::new().lower_file(file);
    let mut all = p.diagnostics.clone();
    all.extend(diag);
    (pkg, all)
}

/// Type-check a lowered package. Returns the list of type-checker
/// diagnostics (errors + warnings). Callers typically concatenate these
/// with the result of [`lower`].
pub fn type_check(pkg: &Package) -> Vec<Diagnostic> {
    sdust_types::check_package(pkg)
}

/// Type-check + borrow-check a lowered package. The borrow check runs
/// only if type-check produced no *errors* (warnings are tolerated).
/// Returns the union of both diagnostic lists.
pub fn type_and_borrow_check(pkg: &Package) -> Vec<Diagnostic> {
    let typed = sdust_types::check_package_typed(pkg);
    let mut diags = typed.diagnostics.clone();
    let any_type_err = diags
        .iter()
        .any(|d| matches!(d.severity, sdust_diagnostics::Severity::Error));
    if !any_type_err {
        diags.extend(sdust_borrow::check_package(&typed, pkg));
    }
    diags
}
