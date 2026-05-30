//! `std.test` — Mighty-native test runner.
//!
//! Discovery rule (v0.2): every `fn` declared in any `.mty` file under
//! the package's `tests/` directory whose name begins with `test_` is a
//! test. Each test is invoked through the slice-6 SIR interpreter,
//! which runs deterministically and respects per-fn step budgets.
//!
//! Reporting: prints `ok` / `FAILED` per test, then a summary line. A
//! single failing test makes the runner exit nonzero.
//!
//! v0.3 will lift the `test_` prefix convention to a proper `test fn`
//! syntax + `#[test]` attribute (parser change deferred to keep this
//! slice scoped — see `STDLIB_V0_2_NOTES.md`).

#[cfg(feature = "runner")]
use mty_diagnostics::Severity;
#[cfg(feature = "runner")]
use mty_driver::{
    check_use_resolution, discover_package_sources, lower, lower_files_with_ownership,
    parse_source, type_and_borrow_check, ParsedFile,
};
#[cfg(feature = "runner")]
use mty_ir::interp::host::RealHost;
#[cfg(feature = "runner")]
use mty_ir::interp::run::{run_fn_with_budget, RunResult};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct DiscoveredTest {
    pub file: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum TestOutcome {
    Pass,
    Fail(String),
}

#[derive(Debug, Clone)]
pub struct TestReport {
    pub name: String,
    pub file: PathBuf,
    pub outcome: TestOutcome,
}

#[derive(Debug, Default)]
pub struct TestRunSummary {
    pub reports: Vec<TestReport>,
    pub passed: usize,
    pub failed: usize,
    pub output: String,
}

impl TestRunSummary {
    pub fn exit_code(&self) -> i32 {
        if self.failed == 0 {
            0
        } else {
            1
        }
    }

    /// Finalize counts + the summary line. Call after pushing all
    /// individual `TestReport`s.
    pub fn finalize(&mut self) {
        self.passed = self
            .reports
            .iter()
            .filter(|r| matches!(r.outcome, TestOutcome::Pass))
            .count();
        self.failed = self.reports.len() - self.passed;
        self.output.push_str(&format!(
            "\ntest result: {} passed; {} failed; {} total\n",
            self.passed,
            self.failed,
            self.reports.len()
        ));
    }
}

/// Walk `dir` recursively and return all `.mty` files in lexicographic
/// order. Missing dir → empty vec (matches `cargo test` ergonomics).
pub fn discover_test_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    if !dir.exists() {
        return out;
    }
    walk(dir, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("mty") {
            out.push(p);
        }
    }
}

/// Compose a `crate::file_stem::test_name` identifier for the reporter.
pub fn qualified_name(file: &Path, name: &str) -> String {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("test");
    format!("{stem}::{name}")
}

/// Run every `test_*` fn in every `.mty` file under `dir`. Returns a
/// summary suitable for printing + exit-code use. Requires the
/// `runner` feature (default-on) which pulls the driver + diagnostics
/// stack.
///
/// **v0.41 T2**: if `dir` looks like a package's `tests/` directory
/// (i.e. its parent contains `mighty.toml`), this delegates to
/// [`run_package`] so each test file is assembled with every
/// `src/**/*.mty` module under the manifest into a single HIR
/// `Package` before lower/typecheck/run. This is what makes
/// `use lib.{fn}` in `tests/foo.mty` resolve to the real symbol in
/// `src/lib.mty` instead of silently returning a default. The
/// pre-v0.41 behavior (each file parsed in isolation) is preserved
/// when no manifest is found — the unit-tests in
/// `crates/mty-stdlib/tests/test_runner.rs` rely on that shape.
#[cfg(feature = "runner")]
pub fn run_dir(dir: &Path) -> TestRunSummary {
    // v0.41 T2 — if this is the `tests/` subdir of a Mighty package,
    // route through the package-aware runner so sibling `src/`
    // modules are visible.
    if let Some(parent) = dir.parent() {
        if parent.join("mighty.toml").is_file() {
            return run_package(parent);
        }
    }
    run_dir_legacy(dir)
}

/// v0.41 T2 — package-aware test runner. Walks `manifest_dir/src/**`
/// to build the package source set, then for each `tests/*.mty` file
/// folds that file + every src module into one HIR `Package`. The
/// test file's `test_*` fns then execute against a world where bare
/// references to top-level symbols defined in `src/**` resolve
/// correctly. Each test file gets its own assembled package so a
/// failure in one file's typecheck doesn't poison the others.
#[cfg(feature = "runner")]
pub fn run_package(manifest_dir: &Path) -> TestRunSummary {
    let tests_dir = manifest_dir.join("tests");
    let test_files = discover_test_files(&tests_dir);
    let src_files = discover_package_sources(manifest_dir);
    // Eagerly parse `src/` once — the parsed greens are cheap to clone
    // when we fold them into each test's package.
    let src_parsed: Vec<ParsedFile> = src_files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok().map(|s| (p.clone(), s)))
        .map(|(p, s)| parse_source(s, p.display().to_string()))
        .collect();
    // Pre-compute the set of module names — used by `check_use_resolution`
    // to distinguish "no such module" (MT2029) from "module exists but
    // missing symbol" (MT2030).
    let mut package_modules: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for p in &src_files {
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            package_modules.insert(stem.to_string());
        }
    }
    let mut summary = TestRunSummary::default();
    for file in &test_files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                summary
                    .output
                    .push_str(&format!("error reading {}: {e}\n", file.display()));
                continue;
            }
        };
        let parsed = parse_source(src, file.display().to_string());
        let mut all_parsed: Vec<ParsedFile> = src_parsed
            .iter()
            .map(|p| ParsedFile {
                source: p.source.clone(),
                source_id: p.source_id.clone(),
                green: p.green.clone(),
                diagnostics: p.diagnostics.clone(),
            })
            .collect();
        all_parsed.push(parsed);
        run_package_file(file, &all_parsed, &package_modules, &mut summary);
    }
    summary.finalize();
    summary
}

/// Helper for [`run_package`] — wires one test file's assembled
/// package through lower → use-resolution → type/borrow check →
/// SIR-lower → run, and pushes per-test reports into `summary`.
///
/// Each per-file diagnostic that escalates to a hard failure adds a
/// single `<parse>` / `<typecheck>` placeholder report (mirroring the
/// pre-v0.41 surface) so callers can count failures uniformly.
#[cfg(feature = "runner")]
fn run_package_file(
    file: &Path,
    all_parsed: &[ParsedFile],
    package_modules: &std::collections::BTreeSet<String>,
    summary: &mut TestRunSummary,
) {
    let (pkg, mut diags, ownership) = lower_files_with_ownership(all_parsed);
    // v0.41 T2 — surface MT2029/MT2030 before typecheck so the
    // failure mode is "use lib.{xyz} — symbol not found" (a clear,
    // actionable error) instead of "answer() returned a fresh-var
    // default" (the silent failure mode the lesson reported).
    diags.extend(check_use_resolution(all_parsed, &pkg, package_modules));
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        summary
            .output
            .push_str(&format!("FAILED to parse/lower {}\n", file.display()));
        for d in &diags {
            if matches!(d.severity, Severity::Error) {
                summary
                    .output
                    .push_str(&format!("  - {}\n", d.primary.message));
            }
        }
        summary.reports.push(TestReport {
            name: "<parse>".into(),
            file: file.to_path_buf(),
            outcome: TestOutcome::Fail(format!("parse/lower failed in {}", file.display())),
        });
        return;
    }
    diags.extend(type_and_borrow_check(&pkg));
    if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
        summary
            .output
            .push_str(&format!("FAILED to typecheck {}\n", file.display()));
        for d in &diags {
            if matches!(d.severity, Severity::Error) {
                summary
                    .output
                    .push_str(&format!("  - {}\n", d.primary.message));
            }
        }
        summary.reports.push(TestReport {
            name: "<typecheck>".into(),
            file: file.to_path_buf(),
            outcome: TestOutcome::Fail(format!("typecheck failed in {}", file.display())),
        });
        return;
    }
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);

    // Use the per-file ownership view returned by `lower_files_with_ownership`
    // to identify which fns were lowered out of THIS file. A `src/`
    // module that defines `pub fn test_helper(...)` is part of the
    // package but not a test — only `test_*` fns declared in the test
    // file itself get dispatched.
    let test_file_id = file.display().to_string();
    let test_fn_names: std::collections::BTreeSet<String> = ownership
        .iter()
        .find(|(id, _)| id == &test_file_id)
        .map(|(_, names)| {
            names
                .iter()
                .filter(|n| n.starts_with("test_"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    for (_id, test_fn) in pkg
        .fns
        .iter()
        .filter(|(_, f)| test_fn_names.contains(&f.name))
    {
        let mut host = RealHost;
        let res = run_fn_with_budget(&prog, &test_fn.name, vec![], &mut host, 5_000_000);
        let (label, outcome) = match res {
            Ok(_) => ("ok", TestOutcome::Pass),
            Err(RunResult::Trap { code, message }) => (
                "FAILED",
                TestOutcome::Fail(format!("trap {code}: {message}")),
            ),
            Err(RunResult::BudgetExceeded) => {
                ("FAILED", TestOutcome::Fail("step budget exceeded".into()))
            }
            Err(RunResult::MemBudgetExceeded { used, limit }) => (
                "FAILED",
                TestOutcome::Fail(format!("mem budget exceeded: {used} B > {limit} B")),
            ),
            Err(RunResult::NoMain) => (
                "FAILED",
                TestOutcome::Fail(format!("test fn {} not found", test_fn.name)),
            ),
            Err(RunResult::Ok { exit }) if exit != 0 => {
                ("FAILED", TestOutcome::Fail(format!("test exited {exit}")))
            }
            Err(RunResult::Ok { .. }) => ("ok", TestOutcome::Pass),
        };
        summary.output.push_str(&format!(
            "test {} ... {label}\n",
            qualified_name(file, &test_fn.name)
        ));
        if let TestOutcome::Fail(ref m) = outcome {
            summary.output.push_str(&format!("  reason: {m}\n"));
        }
        summary.reports.push(TestReport {
            name: test_fn.name.clone(),
            file: file.to_path_buf(),
            outcome,
        });
    }
}

/// Pre-v0.41 single-file shape — retained for back-compat with
/// callers that pre-date the package-aware runner. Treats every
/// `.mty` file under `dir` as a standalone unit, parses/lowers/
/// typechecks/runs it in isolation. The bin entry point at
/// `crates/mty-stdlib/src/bin/mty-test.rs` and the `test_runner.rs`
/// fixture tests still hit this path because they hand in a bare
/// `tests/` dir with no surrounding manifest.
#[cfg(feature = "runner")]
fn run_dir_legacy(dir: &Path) -> TestRunSummary {
    let files = discover_test_files(dir);
    let mut summary = TestRunSummary::default();

    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                summary
                    .output
                    .push_str(&format!("error reading {}: {e}\n", file.display()));
                continue;
            }
        };
        let id = file.display().to_string();
        let parsed = parse_source(src, id);
        let (pkg, mut diags) = lower(&parsed);
        if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
            summary
                .output
                .push_str(&format!("FAILED to parse/lower {}\n", file.display()));
            for d in &diags {
                if matches!(d.severity, Severity::Error) {
                    summary
                        .output
                        .push_str(&format!("  - {}\n", d.primary.message));
                }
            }
            summary.reports.push(TestReport {
                name: "<parse>".into(),
                file: file.to_path_buf(),
                outcome: TestOutcome::Fail(format!("parse/lower failed in {}", file.display())),
            });
            continue;
        }
        diags.extend(type_and_borrow_check(&pkg));
        if diags.iter().any(|d| matches!(d.severity, Severity::Error)) {
            summary
                .output
                .push_str(&format!("FAILED to typecheck {}\n", file.display()));
            for d in &diags {
                if matches!(d.severity, Severity::Error) {
                    summary
                        .output
                        .push_str(&format!("  - {}\n", d.primary.message));
                }
            }
            summary.reports.push(TestReport {
                name: "<typecheck>".into(),
                file: file.to_path_buf(),
                outcome: TestOutcome::Fail(format!("typecheck failed in {}", file.display())),
            });
            continue;
        }
        let typed = mty_types::check_package_typed(&pkg);
        let prog = mty_ir::lower_package(&pkg, &typed);

        for (_id, test_fn) in pkg.fns.iter().filter(|(_, f)| f.name.starts_with("test_")) {
            let mut host = RealHost;
            let res = run_fn_with_budget(&prog, &test_fn.name, vec![], &mut host, 5_000_000);
            let (label, outcome) = match res {
                Ok(_) => ("ok", TestOutcome::Pass),
                Err(RunResult::Trap { code, message }) => (
                    "FAILED",
                    TestOutcome::Fail(format!("trap {code}: {message}")),
                ),
                Err(RunResult::BudgetExceeded) => {
                    ("FAILED", TestOutcome::Fail("step budget exceeded".into()))
                }
                Err(RunResult::MemBudgetExceeded { used, limit }) => (
                    "FAILED",
                    TestOutcome::Fail(format!("mem budget exceeded: {used} B > {limit} B")),
                ),
                Err(RunResult::NoMain) => (
                    "FAILED",
                    TestOutcome::Fail(format!("test fn {} not found", test_fn.name)),
                ),
                Err(RunResult::Ok { exit }) if exit != 0 => {
                    ("FAILED", TestOutcome::Fail(format!("test exited {exit}")))
                }
                Err(RunResult::Ok { .. }) => ("ok", TestOutcome::Pass),
            };
            summary.output.push_str(&format!(
                "test {} ... {label}\n",
                qualified_name(file, &test_fn.name)
            ));
            if let TestOutcome::Fail(ref m) = outcome {
                summary.output.push_str(&format!("  reason: {m}\n"));
            }
            summary.reports.push(TestReport {
                name: test_fn.name.clone(),
                file: file.to_path_buf(),
                outcome,
            });
        }
    }

    summary.finalize();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(discover_test_files(tmp.path()).is_empty());
    }

    #[test]
    fn discover_finds_sd_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("a.mty"), "").unwrap();
        fs::write(tmp.path().join("b.txt"), "").unwrap();
        let found = discover_test_files(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].extension().unwrap(), "mty");
    }
}
