use mty_ast::{AstNode, File};
use mty_diagnostics::{codes::DiagCode, Diagnostic, Label};
use mty_hir::Package;
use mty_syntax::parse;
use std::path::{Path, PathBuf};

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

/// v0.41 T2 — multi-file lowering. Walks every `ParsedFile` and folds
/// them into a *single* HIR `Package` by reusing one `LoweringCtx`
/// across all inputs. This is the path `mty test` + `mty check`
/// (package-anchored) take so that a tests/ file can `use lib.{fn}` a
/// sibling `src/lib.mty` module: both end up in the same top-level
/// namespace where the v0.4 type checker resolves bare `fn` references
/// via `defs.by_name`.
///
/// Files are lowered in the order supplied — caller is responsible for
/// any deterministic sort (the test runner sorts lex via
/// `discover_test_files`). Each file's parser-stage diagnostics are
/// concatenated in front of the lowering diagnostics so the rendered
/// error report can still attribute spans back to their owning file.
///
/// Returns `(Package, all_diagnostics)` — diagnostics carry
/// `Severity::Error` if any phase failed; callers should test for
/// errors before threading the package into `type_and_borrow_check`.
pub fn lower_files(files: &[ParsedFile]) -> (Package, Vec<Diagnostic>) {
    let (pkg, diags, _ownership) = lower_files_with_ownership(files);
    (pkg, diags)
}

/// v0.41 T2 — same as [`lower_files`] but also returns a per-file
/// ownership view of the resulting `Package`'s `fns` arena. The third
/// tuple slot is a `Vec` of `(source_id, fn_names)` in the SAME order
/// as the input — callers (the test runner) use this to figure out
/// which `test_*` fns belong to a given file even after the merge
/// flattened them all into one Package's namespace.
///
/// Returns names rather than `FnId`s because the arena's identifier
/// type is private to `mty-hir`; the test runner only needs the names
/// to filter what to dispatch.
#[allow(clippy::type_complexity)]
pub fn lower_files_with_ownership(
    files: &[ParsedFile],
) -> (Package, Vec<Diagnostic>, Vec<(String, Vec<String>)>) {
    let mut ctx = mty_hir::lower::LoweringCtx::new();
    let mut all: Vec<Diagnostic> = Vec::new();
    let mut ownership: Vec<(String, Vec<String>)> = Vec::with_capacity(files.len());
    let mut last_fn_count = 0usize;
    for p in files {
        // Surface parser-stage diagnostics first so the file order is
        // preserved in the rendered report.
        all.extend(p.diagnostics.clone());
        let file =
            File::cast(mty_syntax::SyntaxNode::new_root(p.green.clone())).expect("FILE root");
        let (pkg_so_far, diag) = ctx.lower_file(file);
        all.extend(diag);
        let fn_names: Vec<String> = pkg_so_far
            .fns
            .iter()
            .skip(last_fn_count)
            .map(|(_, f)| f.name.clone())
            .collect();
        last_fn_count = pkg_so_far.fns.len();
        ownership.push((p.source_id.clone(), fn_names));
        ctx = mty_hir::lower::LoweringCtx::from_partial(pkg_so_far);
    }
    (ctx.into_package(), all, ownership)
}

/// v0.41 T2 — discover the package source set rooted at a
/// `mighty.toml` directory. Returns every `.mty` file under `src/`
/// (recursive), lex-sorted for determinism. Missing `src/` is fine —
/// callers fall back to the test file alone.
///
/// The walker uses the same denylist as `mty test --eval` discovery
/// (target/build/dist/.git/node_modules/.venv) so it never wanders
/// into vendored / build artifact trees.
pub fn discover_package_sources(manifest_dir: &Path) -> Vec<PathBuf> {
    let src_root = manifest_dir.join("src");
    let mut out = Vec::new();
    walk_pkg(&src_root, &mut out);
    out.sort();
    out
}

fn walk_pkg(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            if matches!(
                name,
                "target" | ".git" | "node_modules" | "build" | "dist" | ".venv"
            ) {
                continue;
            }
        }
        if p.is_dir() {
            walk_pkg(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("mty") {
            out.push(p);
        }
    }
}

/// v0.41 T2 — walk upward from `path` looking for the package root
/// (a directory containing `mighty.toml`). Mirrors the behavior of
/// `cmd::build::find_manifest_root` but lives in the driver so
/// both `mty test` and `mty check` can share the lookup.
pub fn find_manifest_root(path: &Path) -> Option<PathBuf> {
    let abs = path.canonicalize().ok()?;
    let mut cur = if abs.is_file() {
        abs.parent()?.to_path_buf()
    } else {
        abs
    };
    loop {
        if cur.join("mighty.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// v0.41 T2 — synthesise MT2029 / MT2030 diagnostics for `use mod.{fn}`
/// declarations that don't match anything in the assembled package.
/// Runs after `lower_files` so it sees the complete top-level
/// namespace.
///
/// We deliberately don't try to repair the surface: leaves are
/// not (yet) populated by `lower_use`, so this pass walks the CST of
/// each `ParsedFile` to recover the `use lib.{a, b}` shape directly,
/// then checks `lib` against the merged set of file stems (the package
/// modules) and `a`/`b` against `defs.by_name` style top-level names.
///
/// `package_modules` is the set of module names — typically
/// `src/<name>.mty` → `<name>` — that should be considered "real" so
/// `use lib.{...}` knows whether `lib` is missing entirely (MT2029)
/// vs missing one of its symbols (MT2030).
pub fn check_use_resolution(
    files: &[ParsedFile],
    pkg: &Package,
    package_modules: &std::collections::BTreeSet<String>,
) -> Vec<Diagnostic> {
    use mty_diagnostics::codes::{SYMBOL_NOT_IN_MODULE, UNRESOLVED_MODULE};
    use mty_syntax::SyntaxKind;
    let mut out: Vec<Diagnostic> = Vec::new();
    // Build the set of all top-level names the package defines, by
    // simple name. `lower` puts everything fn/struct/enum/etc into
    // these arenas; we mirror the test-runner's "by simple name"
    // lookup so the check sees the same world the type-checker sees.
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for hf in pkg.fns.iter().map(|(_, f)| f) {
        if !hf.name.is_empty() {
            names.insert(hf.name.clone());
        }
    }
    for hs in pkg.structs.iter().map(|(_, s)| s) {
        if !hs.name.is_empty() {
            names.insert(hs.name.clone());
        }
    }
    for he in pkg.enums.iter().map(|(_, e)| e) {
        if !he.name.is_empty() {
            names.insert(he.name.clone());
        }
    }
    for ta in pkg.type_aliases.iter().map(|(_, t)| t) {
        if !ta.name.is_empty() {
            names.insert(ta.name.clone());
        }
    }
    for c in pkg.protocols.iter().map(|(_, p)| p) {
        if !c.name.is_empty() {
            names.insert(c.name.clone());
        }
    }

    for p in files {
        let root = mty_syntax::SyntaxNode::new_root(p.green.clone());
        for node in root.descendants() {
            if node.kind() != SyntaxKind::USE_DECL {
                continue;
            }
            // First NAME_REF chain inside the USE_DECL is the module
            // path; NAME children after the L_BRACE are leaves.
            let mut path_segs: Vec<(String, rowan::TextRange)> = Vec::new();
            for n in node.children() {
                if n.kind() == SyntaxKind::PATH {
                    for seg in n.descendants() {
                        if seg.kind() == SyntaxKind::NAME_REF {
                            if let Some(tok) = seg.first_token() {
                                path_segs.push((tok.text().to_string(), seg.text_range()));
                            }
                        }
                    }
                }
            }
            // Skip `use std....` and `use foo.bar.baz...` (only the
            // single-segment `use mod.{...}` shape is in scope for
            // L13 — multi-segment paths talk to the stdlib or future
            // dep graph and don't have local-package modules to verify
            // against).
            if path_segs.len() != 1 {
                continue;
            }
            let (mod_name, mod_span) = &path_segs[0];
            // Standard library prefix — never flag.
            if mod_name == "std" {
                continue;
            }
            // Collect leaves: NAMEs directly under USE_DECL between
            // L_BRACE and R_BRACE. We use simple-name semantics — no
            // alias handling needed for the v0.41 T2 surface; the
            // parser still accepts `as` but we don't model it here.
            let mut leaves: Vec<(String, rowan::TextRange)> = Vec::new();
            let mut after_brace = false;
            for ch in node.children_with_tokens() {
                if ch.kind() == SyntaxKind::L_BRACE {
                    after_brace = true;
                    continue;
                }
                if ch.kind() == SyntaxKind::R_BRACE {
                    after_brace = false;
                    continue;
                }
                if !after_brace {
                    continue;
                }
                if let Some(n) = ch.as_node() {
                    if n.kind() == SyntaxKind::NAME {
                        if let Some(tok) = n.first_token() {
                            leaves.push((tok.text().to_string(), n.text_range()));
                        }
                    }
                }
            }
            if leaves.is_empty() {
                // `use foo;` shape — current resolver already binds the
                // module name in scope; nothing to validate for L13.
                continue;
            }
            // MT2029 — module not in package.
            if !package_modules.contains(mod_name) {
                out.push(Diagnostic::error(
                    UNRESOLVED_MODULE,
                    Label {
                        start: usize::from(mod_span.start()),
                        end: usize::from(mod_span.end()),
                        message: format!(
                            "no module named `{mod_name}` in this package (looked in `src/`)"
                        ),
                    },
                ));
                continue;
            }
            // MT2030 — module exists, symbol missing.
            for (leaf, span) in &leaves {
                if !names.contains(leaf) {
                    out.push(Diagnostic::error(
                        SYMBOL_NOT_IN_MODULE,
                        Label {
                            start: usize::from(span.start()),
                            end: usize::from(span.end()),
                            message: format!("symbol `{leaf}` not found in module `{mod_name}`"),
                        },
                    ));
                }
            }
        }
    }
    out
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
#[cfg(feature = "host-toolchain")]
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
#[cfg(feature = "host-toolchain")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_files_merges_two_sources_into_one_package() {
        // Two trivial files, each declaring one fn. After lower_files,
        // both fns must live in the same Package.fns arena and the
        // ownership view must report them under their respective files.
        let a = parse_source(
            "pub fn answer() -> I32 { return 42; }".to_string(),
            "src/lib.mty".to_string(),
        );
        let b = parse_source(
            "fn test_answer() { if answer() != 42 { panic(\"wrong\"); } }".to_string(),
            "tests/t.mty".to_string(),
        );
        let (pkg, diags, ownership) = lower_files_with_ownership(&[a, b]);
        assert!(
            !diags
                .iter()
                .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error)),
            "unexpected lower errors: {diags:?}"
        );
        let names: Vec<String> = pkg.fns.iter().map(|(_, f)| f.name.clone()).collect();
        assert!(names.contains(&"answer".to_string()), "{names:?}");
        assert!(names.contains(&"test_answer".to_string()), "{names:?}");
        assert_eq!(ownership.len(), 2);
        assert_eq!(ownership[0].0, "src/lib.mty");
        assert!(ownership[0].1.contains(&"answer".to_string()));
        assert_eq!(ownership[1].0, "tests/t.mty");
        assert!(ownership[1].1.contains(&"test_answer".to_string()));
    }

    fn dup_parsed(p: &ParsedFile) -> ParsedFile {
        ParsedFile {
            source: p.source.clone(),
            source_id: p.source_id.clone(),
            green: p.green.clone(),
            diagnostics: p.diagnostics.clone(),
        }
    }

    #[test]
    fn check_use_resolution_flags_missing_module_and_symbol() {
        let lib_src = "pub fn answer() -> I32 { return 42; }".to_string();
        let test_src = "use ghost.{answer};\nuse lib.{answr};\nfn test_x() {}".to_string();
        let lib = parse_source(lib_src.clone(), "src/lib.mty".to_string());
        let test = parse_source(test_src.clone(), "tests/t.mty".to_string());
        let merged = vec![dup_parsed(&lib), dup_parsed(&test)];
        let (pkg, _diags) = lower_files(&merged);
        let mut mods = std::collections::BTreeSet::new();
        mods.insert("lib".to_string());
        let for_check = vec![dup_parsed(&lib), dup_parsed(&test)];
        let diags = check_use_resolution(&for_check, &pkg, &mods);
        use mty_diagnostics::codes::{SYMBOL_NOT_IN_MODULE, UNRESOLVED_MODULE};
        let mt2029 = diags.iter().filter(|d| d.code == UNRESOLVED_MODULE).count();
        let mt2030 = diags
            .iter()
            .filter(|d| d.code == SYMBOL_NOT_IN_MODULE)
            .count();
        assert_eq!(mt2029, 1, "expected one MT2029 — got {diags:?}");
        assert_eq!(mt2030, 1, "expected one MT2030 — got {diags:?}");
    }

    #[test]
    fn check_use_resolution_ignores_stdlib_paths() {
        let test = parse_source(
            "use std.http.{serve};\nfn test_x() {}".to_string(),
            "tests/t.mty".to_string(),
        );
        let (pkg, _) = lower_files(&[dup_parsed(&test)]);
        let mods = std::collections::BTreeSet::new();
        let diags = check_use_resolution(&[dup_parsed(&test)], &pkg, &mods);
        assert!(
            diags.is_empty(),
            "stdlib `use std.*` chains must not surface MT2029/MT2030: {diags:?}"
        );
    }

    #[test]
    fn discover_package_sources_walks_src_recursively() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(tmp.path().join("src/sub")).unwrap();
        std::fs::write(tmp.path().join("src/a.mty"), "").unwrap();
        std::fs::write(tmp.path().join("src/sub/b.mty"), "").unwrap();
        std::fs::write(tmp.path().join("src/skip.txt"), "").unwrap();
        let found = discover_package_sources(tmp.path());
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|p| p.ends_with("a.mty")));
        assert!(found.iter().any(|p| p.ends_with("b.mty")));
    }

    #[test]
    fn find_manifest_root_walks_upward() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pkg_root = tmp.path().join("pkg");
        std::fs::create_dir_all(pkg_root.join("src/deep")).unwrap();
        std::fs::write(pkg_root.join("mighty.toml"), "[package]\nname=\"x\"\n").unwrap();
        let nested = pkg_root.join("src/deep/file.mty");
        std::fs::write(&nested, "").unwrap();
        let root = find_manifest_root(&nested).expect("should locate package root");
        assert_eq!(
            root.canonicalize().unwrap(),
            pkg_root.canonicalize().unwrap()
        );
    }
}
