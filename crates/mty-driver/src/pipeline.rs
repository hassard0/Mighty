use mty_ast::{AstNode, File};
use mty_diagnostics::{codes::DiagCode, Diagnostic, Label};
use mty_hir::Package;
use mty_syntax::parse;

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
            // v0.22 (Coverage Closure): preserve the parser-supplied
            // diagnostic code (default MT0001) instead of unconditionally
            // collapsing every parse error to UNEXPECTED_TOKEN. This is
            // how MT0004 (unknown duration unit) and MT0030 (depth limit
            // exceeded) reach the diagnostic surface.
            Diagnostic::error(
                DiagCode::new(e.code),
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
    let file = File::cast(mty_syntax::SyntaxNode::new_root(p.green.clone())).expect("FILE root");
    let (pkg, diag) = mty_hir::lower::LoweringCtx::new().lower_file(file);
    let mut all = p.diagnostics.clone();
    all.extend(diag);
    (pkg, all)
}

/// Type-check a lowered package. Returns the list of type-checker
/// diagnostics (errors + warnings). Callers typically concatenate these
/// with the result of [`lower`].
pub fn type_check(pkg: &Package) -> Vec<Diagnostic> {
    mty_types::check_package(pkg)
}

/// Type-check + borrow-check a lowered package. The borrow check runs
/// only if type-check produced no *errors* (warnings are tolerated).
/// Returns the union of both diagnostic lists.
pub fn type_and_borrow_check(pkg: &Package) -> Vec<Diagnostic> {
    let typed = mty_types::check_package_typed(pkg);
    let mut diags = typed.diagnostics.clone();
    let any_type_err = diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error));
    if !any_type_err {
        diags.extend(mty_borrow::check_package(&typed, pkg));
    }
    diags
}

/// Slice-6 helper: lower a typed package all the way to SIR. Caller
/// must ensure the package type-checks (and ideally borrow-checks)
/// cleanly before invoking — the SIR lowerer is tolerant but its
/// output isn't worth running on a type-broken program.
pub fn lower_to_sir(pkg: &Package) -> (mty_ir::Program, Vec<Diagnostic>) {
    let typed = mty_types::check_package_typed(pkg);
    let diags = typed.diagnostics.clone();
    let prog = mty_ir::lower_package(pkg, &typed);
    (prog, diags)
}

/// Slice-6 helper: parse → lower → type+borrow check → SIR-lower → run.
/// Returns the interpreter exit code. Stops on any error in earlier
/// phases.
pub fn run_file(src: String, source_id: String) -> i32 {
    use mty_diagnostics::render::ariadne::render_all;
    use mty_diagnostics::Severity;
    let parsed = parse_source(src.clone(), source_id.clone());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        eprint!("{}", render_all(&diags, &source_id, &src));
        return 1;
    }
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);
    // v0.3 Task 1: use StdHost (not RealHost) so EffectOp::GenericCall
    // routes through the dispatcher installed by `mty_stdlib::host::install()`.
    let mut host = mty_runtime::host_std::StdHost::new(std::sync::Arc::new(
        mty_runtime::BudgetTracker::new(mty_runtime::Budget::default()),
    ));
    let res = mty_ir::interp::run(&prog, &mut host);
    if let mty_ir::interp::RunResult::Trap { code, message } = &res {
        eprintln!("trap {}: {}", code, message);
    }
    res.exit_code()
}

/// Slice-7 helper: parse → lower → type+borrow check → SIR-lower → run
/// **on the runtime** (tokio executor + agents). Returns the exit
/// code from the runtime's outcome:
///
/// - 0 = `main` returned cleanly.
/// - 1 = parse / type-check / borrow error, or a trap during execution.
/// - 2 = no `main` defined.
/// - 3 = step budget exhausted by `main`.
///
/// If `main` is absent the runtime falls back to a 0 exit (this
/// matches example 07/08/10 which lack a `main` in their canonical
/// form). Agents that have been spawned but have not received any
/// messages are shut down cleanly.
pub fn run_file_with_runtime(src: String, source_id: String) -> i32 {
    use mty_diagnostics::render::ariadne::render_all;
    use mty_diagnostics::Severity;
    let parsed = parse_source(src.clone(), source_id.clone());
    let (pkg, mut diags) = lower(&parsed);
    if !diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        diags.extend(type_and_borrow_check(&pkg));
    }
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        eprint!("{}", render_all(&diags, &source_id, &src));
        return 1;
    }
    let typed = mty_types::check_package_typed(&pkg);
    let prog = std::sync::Arc::new(mty_ir::lower_package(&pkg, &typed));

    let runtime = mty_runtime::RuntimeBuilder::new().build(prog.clone());
    let exec = runtime.scheduler.rt.clone();
    exec.block_on(async {
        if prog.fn_by_name("main").is_some() {
            // Slice-7 MVP: run main on the slice-6 interpreter (so user
            // code like `log("hello")` and synchronous business logic
            // still works). When main spawns agents via the runtime
            // surface or via the existing AgentSpawn rvalue, those
            // spawn paths now route through mty-runtime; for slice 7
            // we accept that the embedded AgentSpawn still uses the
            // synchronous slice-6 path. Long-running services should
            // instead embed via the programmatic Runtime API.
            // v0.3 Task 1: use StdHost (not RealHost) so `std.*` calls
            // route through `mty_stdlib::host::install()`'s dispatcher.
            use mty_ir::interp::run::{run_fn_with_budget, RunResult};
            let mut host = mty_runtime::host_std::StdHost::new(std::sync::Arc::new(
                mty_runtime::BudgetTracker::new(mty_runtime::Budget::default()),
            ));
            let res = run_fn_with_budget(&prog, "main", vec![], &mut host, 5_000_000);
            let _ = runtime.shutdown().await;
            match res {
                Ok(_) => 0,
                Err(RunResult::Trap { code, message }) => {
                    eprintln!("trap {}: {}", code, message);
                    1
                }
                Err(RunResult::BudgetExceeded) => 3,
                Err(RunResult::MemBudgetExceeded { used, limit }) => {
                    eprintln!(
                        "trap MT5009: memory budget exceeded: {} B > {} B",
                        used, limit
                    );
                    4
                }
                Err(RunResult::NoMain) => 2,
                Err(RunResult::Ok { exit }) => exit,
            }
        } else {
            let _ = runtime.shutdown().await;
            0
        }
    })
}
