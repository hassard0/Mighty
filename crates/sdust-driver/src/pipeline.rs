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

/// Slice-6 helper: lower a typed package all the way to SIR. Caller
/// must ensure the package type-checks (and ideally borrow-checks)
/// cleanly before invoking — the SIR lowerer is tolerant but its
/// output isn't worth running on a type-broken program.
pub fn lower_to_sir(pkg: &Package) -> (sdust_sir::Program, Vec<Diagnostic>) {
    let typed = sdust_types::check_package_typed(pkg);
    let diags = typed.diagnostics.clone();
    let prog = sdust_sir::lower_package(pkg, &typed);
    (prog, diags)
}

/// Slice-6 helper: parse → lower → type+borrow check → SIR-lower → run.
/// Returns the interpreter exit code. Stops on any error in earlier
/// phases.
pub fn run_file(src: String, source_id: String) -> i32 {
    use sdust_diagnostics::render::ariadne::render_all;
    use sdust_diagnostics::Severity;
    let parsed = parse_source(src.clone(), source_id.clone());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        eprint!("{}", render_all(&diags, &source_id, &src));
        return 1;
    }
    let typed = sdust_types::check_package_typed(&pkg);
    let prog = sdust_sir::lower_package(&pkg, &typed);
    let mut host = sdust_sir::interp::RealHost;
    let res = sdust_sir::interp::run(&prog, &mut host);
    if let sdust_sir::interp::RunResult::Trap { code, message } = &res {
        eprintln!("trap {}: {}", code, message);
    }
    res.exit_code()
}
