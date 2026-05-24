//! `std.test` — Stardust-native test runner.
//!
//! Discovery rule (v0.2): every `fn` declared in any `.sd` file under
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
use sdust_diagnostics::Severity;
#[cfg(feature = "runner")]
use sdust_driver::{lower, parse_source, type_and_borrow_check};
#[cfg(feature = "runner")]
use sdust_sir::interp::host::RealHost;
#[cfg(feature = "runner")]
use sdust_sir::interp::run::{run_fn_with_budget, RunResult};
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

/// Walk `dir` recursively and return all `.sd` files in lexicographic
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
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("sd") {
            out.push(p);
        }
    }
}

/// Compose a `crate::file_stem::test_name` identifier for the reporter.
pub fn qualified_name(file: &Path, name: &str) -> String {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("test");
    format!("{stem}::{name}")
}

/// Run every `test_*` fn in every `.sd` file under `dir`. Returns a
/// summary suitable for printing + exit-code use. Requires the
/// `runner` feature (default-on) which pulls the driver + diagnostics
/// stack.
#[cfg(feature = "runner")]
pub fn run_dir(dir: &Path) -> TestRunSummary {
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
        let typed = sdust_types::check_package_typed(&pkg);
        let prog = sdust_sir::lower_package(&pkg, &typed);

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
        fs::write(tmp.path().join("a.sd"), "").unwrap();
        fs::write(tmp.path().join("b.txt"), "").unwrap();
        let found = discover_test_files(tmp.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].extension().unwrap(), "sd");
    }
}
