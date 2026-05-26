//! Phase-1 conformance harness for spec §37 categories filled in v0.2.
//!
//! Walks `tests/conformance/<category>/<NN_name>/` and runs the case
//! described by `command.txt`. Each case is a directory containing:
//!
//! - `input.mty` — the source under test
//! - `command.txt` — one of: `check`, `run`
//! - `expected_diagnostics.txt` — optional, one SDxxxx code per line.
//!   The harness asserts every listed code is present in the produced
//!   error diagnostics (set-membership, order insensitive). Extra
//!   codes are tolerated so the test stays robust against compiler
//!   enrichment.
//! - `expected_stdout.txt` — optional, exact stdout (with normalised
//!   line endings + trim_end). Compared only for `run` cases.
//! - `expected_exit_code.txt` — optional, parsed as `i32`. Defaults
//!   to `0` when absent. For `check` cases: `0` = no errors, `1` =
//!   at-least-one error.
//!
//! For `run` cases we go through the slice-6 SIR interpreter (same path
//! as `conformance_runtime` / `conformance_runtime_7`) — that's
//! deterministic by construction and side-steps the tokio runtime,
//! keeping the suite fast and reproducible.
//!
//! The intentionally-broken cases are tagged in `INTENTIONALLY_IGNORED`
//! below with a reason. Anything else that fails is a real defect.

use mty_diagnostics::{Diagnostic, Severity};
use mty_driver::{lower, lower_to_sir, parse_source, type_and_borrow_check};
use mty_ir::interp::run::run_fn_with_budget;
use mty_ir::interp::{run, BufferHost, RunResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf()
}

/// Per the spec amendment, a few cases exercise behaviour that the
/// v0.2 compiler doesn't yet enforce. They live in the suite so the
/// shape is captured but are skipped by the harness. Each entry has
/// the form `<category>/<case>` and a one-line reason.
const INTENTIONALLY_IGNORED: &[(&str, &str)] = &[
    // (category/case, reason)
    //
    // v0.3 Task 3 (see CONFORMANCE_V0_3_NOTES.md): we removed three of
    // the five v0.2 entries:
    //   - budget_violation/03_wall_timeout      → already passes today
    //   - supervisor_restart/03_rate_limit_…    → already passes today
    //   - budget_violation/02_step_budget_…     → fixture rewritten to
    //     use recursive call (which DOES tick the interp step budget)
    //     instead of `loop { … }` (which slice-6 lowers as single-iter)
    //
    // The remaining two each block on a change in another agent's
    // crate (mty-syntax / mty-types) and so stay ignored for v0.3.
    (
        "capability_checking/03_narrow_to_ro",
        "narrow positive case: requires runtime fs.ro plumbing that depends on Slice-8 cap-narrowing impl beyond v0.2 scope (mty-types)",
    ),
    (
        "supervisor_restart/02_escalate",
        "parser does not yet accept `escalate` action in `on_fail` (only `restart`/`backoff`); tracked for v0.4 supervisor grammar expansion (mty-syntax)",
    ),
    // v0.15: the v0.14 red-shirt for `borrow_checking/14_borrow_outlives_owner`
    // is now live (the BLOCK fix in mty-hir's `is_expr_node` lets the inner
    // `{ ... }` lower as an expression and the borrow walker sees the
    // reassign — MT3007 fires as expected). Removed from this list.
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Check,
    Run,
}

impl Command {
    fn parse(s: &str) -> Result<Self, String> {
        match s.trim() {
            "check" => Ok(Command::Check),
            "run" => Ok(Command::Run),
            other => Err(format!("unknown command.txt value: {:?}", other)),
        }
    }
}

struct CaseSpec {
    category: String,
    name: String,
    dir: PathBuf,
    command: Command,
    input_src: String,
    expected_diags: Option<Vec<String>>,
    /// v0.11 (Gap B unlock): warning-severity codes the case asserts.
    /// The error-only filter used by the diag check hid warning-only
    /// codes (e.g. MT2026 protocol_msg_unknown) so they had no reachable
    /// conformance_full fixture pre-v0.11. Optional per-case file
    /// `expected_warnings.txt` mirrors the errors file.
    expected_warnings: Option<Vec<String>>,
    expected_stdout: Option<String>,
    expected_exit_code: i32,
    /// Optional override for the interpreter step budget (default = 1M).
    /// Set via `step_budget.txt` per-case. v0.3 added this so the
    /// budget-violation cases can trip MT5009 without overflowing the
    /// real Rust stack (recursion grows the host stack faster than the
    /// default 1M-step budget exhausts).
    step_budget: Option<u64>,
}

fn read_opt(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn load_case(category: &str, dir: &Path) -> Result<CaseSpec, String> {
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let input_path = dir.join("input.mty");
    let command_path = dir.join("command.txt");
    let input_src = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("[{}/{}] read input.mty: {}", category, name, e))?;
    let command_raw = std::fs::read_to_string(&command_path)
        .map_err(|e| format!("[{}/{}] read command.txt: {}", category, name, e))?;
    let command =
        Command::parse(&command_raw).map_err(|e| format!("[{}/{}] {}", category, name, e))?;

    let expected_diags = read_opt(&dir.join("expected_diagnostics.txt")).map(|s| {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect::<Vec<_>>()
    });
    // v0.11: parallel optional list of warning-severity codes.
    let expected_warnings = read_opt(&dir.join("expected_warnings.txt")).map(|s| {
        s.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect::<Vec<_>>()
    });
    let expected_stdout = read_opt(&dir.join("expected_stdout.txt"));
    let expected_exit_code = read_opt(&dir.join("expected_exit_code.txt"))
        .as_deref()
        .map(str::trim)
        .map(|s| s.parse::<i32>().unwrap_or(0))
        .unwrap_or(0);
    let step_budget = read_opt(&dir.join("step_budget.txt"))
        .as_deref()
        .map(str::trim)
        .and_then(|s| s.parse::<u64>().ok());

    Ok(CaseSpec {
        category: category.to_string(),
        name,
        dir: dir.to_path_buf(),
        command,
        input_src,
        expected_diags,
        expected_warnings,
        expected_stdout,
        expected_exit_code,
        step_budget,
    })
}

/// v0.11 (Gap D unlock): RAII guard that temporarily chdir's into the
/// case's directory when it ships a `mighty.toml`. The type checker's
/// `load_profile_from_star_toml()` reads `./mighty.toml` from cwd, so
/// this is the only way to feed a per-case `profile = "core"` to the
/// existing pipeline without modifying mty-types source.
///
/// Restores the original cwd on Drop. Single-threaded test harness so
/// no race with other cases.
struct CwdGuard {
    saved: Option<PathBuf>,
}

impl CwdGuard {
    fn maybe_chdir(case_dir: &Path) -> Self {
        let case_toml = case_dir.join("mighty.toml");
        if !case_toml.exists() {
            return CwdGuard { saved: None };
        }
        let saved = std::env::current_dir().ok();
        let _ = std::env::set_current_dir(case_dir);
        CwdGuard { saved }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(p) = self.saved.take() {
            let _ = std::env::set_current_dir(p);
        }
    }
}

fn check_diagnostics(case: &CaseSpec) -> Result<(i32, Vec<String>, Vec<String>), String> {
    let _cwd = CwdGuard::maybe_chdir(&case.dir);
    let parsed = parse_source(
        case.input_src.clone(),
        case.dir.join("input.mty").display().to_string(),
    );
    let (pkg, mut diags) = lower(&parsed);
    let lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_err {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let error_codes: Vec<String> = diags
        .iter()
        .filter(|d: &&Diagnostic| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect();
    // v0.11: also surface warning-severity codes so cases can assert
    // them (e.g. MT2026 protocol_msg_unknown is warning-only).
    let warning_codes: Vec<String> = diags
        .iter()
        .filter(|d: &&Diagnostic| matches!(d.severity, Severity::Warning))
        .map(|d| d.code.as_str())
        .collect();
    let exit = if error_codes.is_empty() { 0 } else { 1 };
    Ok((exit, error_codes, warning_codes))
}

fn run_program(case: &CaseSpec) -> Result<(i32, String, Vec<String>, Vec<String>), String> {
    let _cwd = CwdGuard::maybe_chdir(&case.dir);
    let parsed = parse_source(
        case.input_src.clone(),
        case.dir.join("input.mty").display().to_string(),
    );
    let (pkg, mut diags) = lower(&parsed);
    let lower_err = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_err {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let error_codes: Vec<String> = diags
        .iter()
        .filter(|d: &&Diagnostic| matches!(d.severity, Severity::Error))
        .map(|d| d.code.as_str())
        .collect();
    let warning_codes: Vec<String> = diags
        .iter()
        .filter(|d: &&Diagnostic| matches!(d.severity, Severity::Warning))
        .map(|d| d.code.as_str())
        .collect();
    if !error_codes.is_empty() {
        return Ok((1, String::new(), error_codes, warning_codes));
    }

    let (prog, _) = lower_to_sir(&pkg);
    let mut host = BufferHost::default();
    // Per-case step budget override (default = run with the interp's
    // built-in 1M budget). When a `step_budget.txt` file is present we
    // invoke `run_fn_with_budget` instead so cases can deliberately
    // trip MT5009 with a smaller bound (avoids growing the host stack
    // past its limit during recursive shapes; see CONFORMANCE_V0_3_NOTES).
    let res = match case.step_budget {
        Some(b) => match run_fn_with_budget(&prog, "main", vec![], &mut host, b) {
            Ok(_) => RunResult::Ok { exit: 0 },
            Err(r) => r,
        },
        None => run(&prog, &mut host),
    };
    let stdout = host.stdout_str();
    let (exit, mut runtime_codes) = match res {
        RunResult::Ok { exit } => (exit, vec![]),
        RunResult::Trap { code, .. } => (1, vec![code.to_string()]),
        RunResult::BudgetExceeded => (3, vec!["MT5009".to_string()]),
        RunResult::MemBudgetExceeded { .. } => (4, vec!["MT5009".to_string()]),
        RunResult::NoMain => (2, vec![]),
    };
    // Surface trap codes alongside any check-time codes for diag-assertion
    // (which is normally empty for `run` cases that succeed).
    runtime_codes.extend(error_codes);
    Ok((exit, stdout, runtime_codes, warning_codes))
}

fn verify(case: &CaseSpec) -> Result<(), String> {
    let prefix = format!("[{}/{}]", case.category, case.name);
    let (exit, stdout, codes, warning_codes) = match case.command {
        Command::Check => {
            let (e, c, w) = check_diagnostics(case)?;
            (e, String::new(), c, w)
        }
        Command::Run => run_program(case)?,
    };

    // Exit code assertion.
    if exit != case.expected_exit_code {
        return Err(format!(
            "{} exit code mismatch: expected {}, got {} (codes={:?}, stdout={:?})",
            prefix, case.expected_exit_code, exit, codes, stdout
        ));
    }

    // Diagnostic codes (set-membership).
    if let Some(want) = &case.expected_diags {
        let got: HashSet<&str> = codes.iter().map(String::as_str).collect();
        let missing: Vec<&String> = want.iter().filter(|c| !got.contains(c.as_str())).collect();
        if !missing.is_empty() {
            return Err(format!(
                "{} missing expected diagnostics: {:?} (got: {:?})",
                prefix, missing, codes
            ));
        }
    }

    // Stdout (run cases).
    if let Some(want) = &case.expected_stdout {
        let want_n = want.replace("\r\n", "\n");
        let got_n = stdout.replace("\r\n", "\n");
        if want_n.trim_end() != got_n.trim_end() {
            return Err(format!(
                "{} stdout mismatch:\nwant: {:?}\ngot:  {:?}",
                prefix, want_n, got_n
            ));
        }
    }

    // v0.11: Warning-severity diagnostic codes (set-membership). Same
    // contract as the errors file: extras tolerated, missing fails.
    if let Some(want) = &case.expected_warnings {
        let got: HashSet<&str> = warning_codes.iter().map(String::as_str).collect();
        let missing: Vec<&String> = want.iter().filter(|c| !got.contains(c.as_str())).collect();
        if !missing.is_empty() {
            return Err(format!(
                "{} missing expected warnings: {:?} (got: {:?})",
                prefix, missing, warning_codes
            ));
        }
    }

    Ok(())
}

fn discover_cases(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = vec![];
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for cat in entries.flatten() {
        let cat_path = cat.path();
        if !cat_path.is_dir() {
            continue;
        }
        let cat_name = cat_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(sub) = std::fs::read_dir(&cat_path) else {
            continue;
        };
        let mut cases: Vec<PathBuf> = sub
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir() && p.join("input.mty").exists() && p.join("command.txt").exists()
            })
            .collect();
        cases.sort();
        for c in cases {
            out.push((cat_name.clone(), c));
        }
    }
    out.sort();
    out
}

fn is_ignored(category: &str, case: &str) -> Option<&'static str> {
    for (key, reason) in INTENTIONALLY_IGNORED {
        let (k_cat, k_case) = key.split_once('/').unwrap();
        if k_cat == category && k_case == case {
            return Some(reason);
        }
    }
    None
}

#[test]
fn phase1_conformance_full() {
    let root = workspace_root().join("tests/conformance");
    let cases = discover_cases(&root);

    let mut failures: Vec<String> = vec![];
    let mut ran = 0usize;
    let mut skipped: Vec<String> = vec![];
    let mut by_category: std::collections::BTreeMap<String, usize> = Default::default();

    // Optional bisect filter for debugging:
    //   STARDUST_CONF_ONLY=<category>            — only that category
    //   STARDUST_CONF_CASE=<category>/<case>     — only that case
    let only = std::env::var("STARDUST_CONF_ONLY").ok();
    let only_case = std::env::var("STARDUST_CONF_CASE").ok();
    for (category, dir) in &cases {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if let Some(o) = &only {
            if category != o {
                continue;
            }
        }
        if let Some(o) = &only_case {
            let key = format!("{}/{}", category, name);
            if &key != o {
                continue;
            }
        }
        if let Some(reason) = is_ignored(category, &name) {
            skipped.push(format!("[{}/{}] ignored: {}", category, name, reason));
            continue;
        }
        let spec = match load_case(category, dir) {
            Ok(s) => s,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        ran += 1;
        *by_category.entry(category.clone()).or_default() += 1;
        eprintln!("conformance_full: running {}/{}", category, name);
        if let Err(e) = verify(&spec) {
            failures.push(e);
        }
    }

    if !skipped.is_empty() {
        eprintln!("conformance_full: {} skipped:", skipped.len());
        for s in &skipped {
            eprintln!("  - {}", s);
        }
    }
    eprintln!(
        "conformance_full: {} cases ran across {} categories",
        ran,
        by_category.len()
    );
    for (k, v) in &by_category {
        eprintln!("  {}: {}", k, v);
    }

    // v0.10 conformance audit expanded the corpus: ≥70 cases across
    // 15 categories (was 25 / 9 at v0.2; +50 at v0.10). See
    // CONFORMANCE_V0_10_NOTES.md + docs/spec/conformance-coverage.md
    // for the per-FROZEN-code coverage table. Skip the floor check
    // when bisecting via STARDUST_CONF_ONLY.
    if only.is_none() && only_case.is_none() {
        assert!(
            ran >= 70,
            "expected ≥70 conformance_full cases, ran {} (have you regressed the corpus?)",
            ran
        );
    }
    assert!(
        failures.is_empty(),
        "{} conformance_full failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
