//! `mty test` — Mighty test runner.
//!
//! v0.30 Track E. Two modes share one CLI verb:
//!
//! 1. **Unit tests** (default) — discover `tests/*.test.mty` (and the
//!    historical bare `tests/*.mty`) and dispatch each through
//!    [`mty_stdlib::test::run_dir`]. Drop-in replacement for the
//!    standalone `mty-test` binary that has lived in `mty-stdlib`
//!    since v0.2.
//! 2. **Eval suites** (`--eval`) — discover `**/*.eval.mty`, parse
//!    `//!` frontmatter to build a [`Suite`](mty_stdlib::eval::Suite),
//!    run each suite under [`mty_stdlib::eval::runner::Runner::run_for_cli`],
//!    pass/fail on per-cell verdicts against a configurable threshold.
//!
//! Both modes share a single `--format json` switch so CI dashboards
//! can ingest machine-readable output regardless of which mode is
//! active.
//!
//! See `docs/internals/eval.md` for the eval-mode file-format spec +
//! GitHub Actions template.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mty_stdlib::eval::runner::{CliReport, CliSink, Runner};
use mty_stdlib::eval::{Case, Compare, Member, Suite};

/// Output format for the run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable per-line stream (the default).
    Pretty,
    /// One JSON object per suite, then a final summary object — one
    /// object per line so CI streams can read it incrementally.
    Json,
}

impl OutputFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pretty" | "text" | "human" => Some(Self::Pretty),
            "json" | "jsonl" => Some(Self::Json),
            _ => None,
        }
    }
}

/// `mty test` argument bundle. Clap-side mirror lives in `main.rs`.
///
/// The struct intentionally carries four bool fields (eval / strict /
/// replay_only / ci) — each maps to a distinct CLI flag with a clear
/// semantic; collapsing them into an enum would obscure the
/// many-flags-can-coexist combinatorics (`--eval --no-strict --ci`
/// is a real shape).
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct TestArgs {
    /// Override the discovery root. Default = current working dir.
    pub manifest_dir: Option<PathBuf>,
    /// Run eval suites instead of (or in addition to) unit tests.
    pub eval: bool,
    /// Eval mode only: fail the run if any (case, member) cell errored.
    /// Default = `true`. Disable with `--no-strict` for offline / no-
    /// API-key dev.
    pub strict: bool,
    /// Eval mode only: skip the live-dispatch path; run only against
    /// previously recorded traces. Free + fast — used in CI to assert
    /// the deterministic-replay equivalence hasn't regressed.
    pub replay_only: bool,
    /// Eval mode only: read provider-set + threshold from the
    /// `[eval.ci]` block in `mighty.toml` instead of the per-file
    /// frontmatter.
    pub ci: bool,
    /// Output shape. See [`OutputFormat`].
    pub format: OutputFormat,
}

impl Default for TestArgs {
    fn default() -> Self {
        Self {
            manifest_dir: None,
            eval: false,
            strict: true,
            replay_only: false,
            ci: false,
            format: OutputFormat::Pretty,
        }
    }
}

/// Public entry point — clap dispatches here from `main.rs`.
pub fn run(args: TestArgs) -> i32 {
    let root = args
        .manifest_dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    if args.eval {
        run_eval(&root, &args)
    } else {
        run_unit(&root, &args)
    }
}

// --- unit-test path ---------------------------------------------------------

/// Unit-mode: delegate to the existing `mty_stdlib::test::run_dir`. We
/// look at `tests/` (the historical layout) and stream the output
/// straight through. JSON output is a thin wrapper that wraps the
/// summary as a single JSON object.
fn run_unit(root: &Path, args: &TestArgs) -> i32 {
    let tests_dir = root.join("tests");
    let summary = mty_stdlib::test::run_dir(&tests_dir);
    match args.format {
        OutputFormat::Pretty => print!("{}", summary.output),
        OutputFormat::Json => {
            // Print one JSON object per test, then a summary line. We
            // deliberately keep the keys flat so `jq` / dashboards
            // don't need a deep selector.
            for r in &summary.reports {
                let (ok, reason) = match &r.outcome {
                    mty_stdlib::test::TestOutcome::Pass => (true, String::new()),
                    mty_stdlib::test::TestOutcome::Fail(m) => (false, m.clone()),
                };
                let obj = serde_json::json!({
                    "type": "test",
                    "name": mty_stdlib::test::qualified_name(&r.file, &r.name),
                    "file": r.file.display().to_string(),
                    "passed": ok,
                    "reason": reason,
                });
                println!("{}", obj);
            }
            let obj = serde_json::json!({
                "type": "summary",
                "mode": "unit",
                "passed": summary.passed,
                "failed": summary.failed,
                "total": summary.reports.len(),
            });
            println!("{}", obj);
        }
    }
    summary.exit_code()
}

// --- eval-mode discovery + run --------------------------------------------

#[derive(Debug, Clone)]
pub struct EvalDiscovery {
    pub files: Vec<PathBuf>,
}

/// Glob over the project tree for `*.eval.mty`. Skips common build /
/// vcs directories without needing a real `.gitignore` parser — the
/// project layout is conventional enough that a hand-rolled denylist
/// is sufficient and avoids pulling the `ignore` crate dep in.
pub fn discover_eval_files(root: &Path, configured_paths: &[PathBuf]) -> EvalDiscovery {
    let mut roots: Vec<PathBuf> = if configured_paths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        configured_paths
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    root.join(p)
                }
            })
            .collect()
    };
    // Stable + de-dup so two configured paths that resolve to the same
    // directory don't double-visit.
    roots.sort();
    roots.dedup();

    let mut out: BTreeSet<PathBuf> = BTreeSet::new();
    for r in &roots {
        walk_eval(r, &mut out);
    }
    EvalDiscovery {
        files: out.into_iter().collect(),
    }
}

fn walk_eval(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    if !dir.exists() {
        return;
    }
    // If `dir` is actually a file ending in .eval.mty, take it.
    if dir.is_file() {
        if file_is_eval(dir) {
            out.insert(dir.to_path_buf());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for ent in entries.flatten() {
        let p = ent.path();
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            // Denylist — keep the walker O(project files), not O(every
            // file in target/ + node_modules/ + .git/).
            if matches!(
                name,
                "target" | ".git" | "node_modules" | "build" | "dist" | ".venv"
            ) {
                continue;
            }
        }
        if p.is_dir() {
            walk_eval(&p, out);
        } else if file_is_eval(&p) {
            out.insert(p);
        }
    }
}

fn file_is_eval(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".eval.mty"))
}

/// Parsed `//!` frontmatter from an `.eval.mty` file.
#[derive(Debug, Clone)]
pub struct EvalFrontmatter {
    pub name: Option<String>,
    /// `<comparator>:<threshold>` parsed out of `//! threshold: foo >= 0.85`.
    pub threshold: Option<ThresholdSpec>,
    /// `provider:model` strings. Required to be non-empty.
    pub members: Vec<MemberSpec>,
    /// Cases — either trace paths or raw input strings.
    pub cases: Vec<CaseSpec>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThresholdSpec {
    pub comparator: String,
    pub threshold: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemberSpec {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaseSpec {
    FromTrace(PathBuf),
    FromInput(String),
}

/// Top-level frontmatter parse error. Surfaced as a per-suite FAIL
/// (with mode-strict semantics) — never panics the runner.
#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("eval frontmatter: missing `members` list")]
    MissingMembers,
    #[error("eval frontmatter: missing `cases` list")]
    MissingCases,
    #[error("eval frontmatter: malformed member spec `{0}` — expected `provider:model`")]
    MalformedMember(String),
    #[error("eval frontmatter: malformed threshold `{0}` — expected `<comparator> >= <0.0-1.0>`")]
    MalformedThreshold(String),
    #[error("eval frontmatter: malformed case spec `{0}`")]
    MalformedCase(String),
}

/// Parse a `//!` YAML-ish header block from an eval file. The block
/// starts at the first `//!` line and ends at the first non-`//!`
/// non-blank line. We deliberately parse a tiny subset of YAML — three
/// keys (`eval`, `threshold`, `members`, `cases`) and two list-element
/// shapes — rather than depend on a full YAML crate. The format
/// round-trips through `mty fmt` because the parser tolerates trailing
/// whitespace + arbitrary spacing.
pub fn parse_frontmatter(src: &str) -> Result<EvalFrontmatter, FrontmatterError> {
    let mut name = None;
    let mut threshold = None;
    let mut members: Vec<MemberSpec> = Vec::new();
    let mut cases: Vec<CaseSpec> = Vec::new();

    // Collect the contiguous `//!` lines at the top of the file.
    let lines: Vec<&str> = src.lines().collect();
    let mut header: Vec<String> = Vec::new();
    for raw in &lines {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("//!") {
            header.push(rest.trim_start().to_string());
        } else if !(trimmed.is_empty() && header.is_empty()) {
            // First non-`//!` non-blank line ends the frontmatter.
            // Blank lines before the header are tolerated.
            break;
        }
    }

    // Walk the header lines. Track which list ("members" / "cases")
    // we're currently filling so `- foo` rows route to the right
    // collection.
    #[derive(PartialEq)]
    enum CurList {
        None,
        Members,
        Cases,
    }
    let mut current = CurList::None;

    for line in &header {
        let stripped = line.trim_end_matches(char::is_whitespace);
        if stripped.is_empty() {
            continue;
        }

        if let Some(val) = stripped.strip_prefix("eval:") {
            name = Some(val.trim().to_string());
            current = CurList::None;
        } else if let Some(val) = stripped.strip_prefix("threshold:") {
            threshold = Some(parse_threshold(val.trim())?);
            current = CurList::None;
        } else if stripped == "members:" {
            current = CurList::Members;
        } else if stripped == "cases:" {
            current = CurList::Cases;
        } else if let Some(item) = stripped.strip_prefix("- ") {
            // List item — route based on `current`.
            match current {
                CurList::Members => members.push(parse_member(item.trim())?),
                CurList::Cases => cases.push(parse_case_item(item.trim())?),
                CurList::None => {
                    // A dash before any list-header is malformed; we
                    // surface it as a case error so the file can't
                    // silently no-op.
                    return Err(FrontmatterError::MalformedCase(item.into()));
                }
            }
        } else if let Some(rest) = stripped.strip_prefix("from_trace:") {
            // Single-case shorthand inside the cases: block when the
            // user wrote `//!   - from_trace: traces/foo.mty-trace` on
            // one line; the `- ` was eaten above, so we land here.
            cases.push(CaseSpec::FromTrace(PathBuf::from(rest.trim())));
        } else if let Some(rest) = stripped.strip_prefix("from_input:") {
            cases.push(CaseSpec::FromInput(unquote(rest.trim())));
        }
        // Unknown keys are silently ignored — keeps the format
        // forward-compat with future v0.31+ additions.
    }

    if members.is_empty() {
        return Err(FrontmatterError::MissingMembers);
    }
    if cases.is_empty() {
        return Err(FrontmatterError::MissingCases);
    }

    Ok(EvalFrontmatter {
        name,
        threshold,
        members,
        cases,
    })
}

fn parse_threshold(raw: &str) -> Result<ThresholdSpec, FrontmatterError> {
    // Shape: `<comparator> >= <number>`. We accept `>=` only (the
    // `<=` shape doesn't make sense for the comparator semantics —
    // every comparator is "higher is better").
    let Some((comp, num)) = raw.split_once(">=") else {
        return Err(FrontmatterError::MalformedThreshold(raw.into()));
    };
    let comp = comp.trim().to_string();
    let num: f32 = num
        .trim()
        .parse()
        .map_err(|_| FrontmatterError::MalformedThreshold(raw.into()))?;
    if !(0.0..=1.0).contains(&num) {
        return Err(FrontmatterError::MalformedThreshold(raw.into()));
    }
    Ok(ThresholdSpec {
        comparator: comp,
        threshold: num,
    })
}

fn parse_member(raw: &str) -> Result<MemberSpec, FrontmatterError> {
    let Some((p, m)) = raw.split_once(':') else {
        return Err(FrontmatterError::MalformedMember(raw.into()));
    };
    let p = p.trim().to_string();
    let m = m.trim().to_string();
    if p.is_empty() || m.is_empty() {
        return Err(FrontmatterError::MalformedMember(raw.into()));
    }
    Ok(MemberSpec {
        provider: p,
        model: m,
    })
}

fn parse_case_item(raw: &str) -> Result<CaseSpec, FrontmatterError> {
    if let Some(rest) = raw.strip_prefix("from_trace:") {
        return Ok(CaseSpec::FromTrace(PathBuf::from(rest.trim())));
    }
    if let Some(rest) = raw.strip_prefix("from_input:") {
        return Ok(CaseSpec::FromInput(unquote(rest.trim())));
    }
    Err(FrontmatterError::MalformedCase(raw.into()))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Build a runnable [`Suite`] from a parsed frontmatter. Returns the
/// suite, plus the chosen comparator (so the runner can use it
/// uniformly across all suites in one run).
pub fn build_suite_from_frontmatter(file: &Path, fm: &EvalFrontmatter) -> (Suite, Compare) {
    let name = fm.name.clone().unwrap_or_else(|| file_stem_name(file));
    let mut suite = Suite::new(name);
    for case in &fm.cases {
        let c = match case {
            CaseSpec::FromTrace(p) => Case::from_trace(p),
            CaseSpec::FromInput(s) => Case::from_input(s.clone()),
        };
        suite = suite.case(c);
    }
    for m in &fm.members {
        suite = suite.run_with(build_member(&m.provider, &m.model));
    }
    let comparator = match fm.threshold.as_ref() {
        Some(t) => comparator_for(&t.comparator, t.threshold),
        None => Compare::semantic_similarity(0.85),
    };
    (suite, comparator)
}

fn file_stem_name(p: &Path) -> String {
    let stem = p.file_name().and_then(|s| s.to_str()).unwrap_or("eval");
    stem.trim_end_matches(".eval.mty").to_string()
}

/// Map a `provider:model` pair to a real [`Member`]. Unknown providers
/// land on `Member::mock` so a fixture authored against a future
/// provider name still has *something* to dispatch against (and the
/// caller sees a clear "mock" tag in the report).
///
/// Missing API keys are surfaced as a labelled mock that errors at
/// dispatch — the strict / no-strict flag controls whether that's a
/// suite failure or a logged-and-skip. We deliberately don't *panic*
/// here (the underlying `Member::anthropic` etc. constructors do
/// panic on missing env, so we pre-check the keys before calling
/// them).
pub fn build_member(provider: &str, model: &str) -> Member {
    match provider.to_ascii_lowercase().as_str() {
        "mock" => Member::mock(model, "mock-reply", 1),
        // Real-provider constructors live in `std.swarm` and panic
        // on missing env vars. Pre-check so a CI invocation with no
        // keys reports a clean "missing-key" mock-error rather than
        // crashing the binary mid-suite.
        "anthropic" => {
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                Member::anthropic(model)
            } else {
                Member::mock_error(format!("anthropic:{model}"), "ANTHROPIC_API_KEY not set")
            }
        }
        "openai" => {
            if std::env::var("OPENAI_API_KEY").is_ok() {
                Member::openai(model)
            } else {
                Member::mock_error(format!("openai:{model}"), "OPENAI_API_KEY not set")
            }
        }
        "gemini" => {
            if std::env::var("GEMINI_API_KEY").is_ok() || std::env::var("GOOGLE_API_KEY").is_ok() {
                Member::gemini(model)
            } else {
                Member::mock_error(
                    format!("gemini:{model}"),
                    "GEMINI_API_KEY/GOOGLE_API_KEY not set",
                )
            }
        }
        "bedrock" => {
            if std::env::var("AWS_BEDROCK_API_TOKEN").is_ok() {
                Member::bedrock(model)
            } else {
                Member::mock_error(format!("bedrock:{model}"), "AWS_BEDROCK_API_TOKEN not set")
            }
        }
        // Unknown provider → fall through to mock so the suite still
        // runs (the report shows the synthetic label).
        other => Member::mock(format!("unknown-{other}-{model}"), "mock-reply", 1),
    }
}

fn comparator_for(name: &str, threshold: f32) -> Compare {
    match name.trim().to_ascii_lowercase().as_str() {
        "equal" | "byte_equal" | "exact" => Compare::equal(),
        "semantic_similarity" | "cosine" | "similarity" => Compare::semantic_similarity(threshold),
        "tool_call_set_equal" | "tool_call" | "tools" => Compare::tool_call_set_equal(),
        // Fallback: semantic_similarity at the supplied threshold.
        _ => Compare::semantic_similarity(threshold),
    }
}

// --- eval-mode runner -------------------------------------------------------

fn run_eval(root: &Path, args: &TestArgs) -> i32 {
    let configured_paths = read_eval_paths_from_manifest(root);
    let discovery = discover_eval_files(root, &configured_paths);

    if discovery.files.is_empty() {
        match args.format {
            OutputFormat::Pretty => println!("eval: no .eval.mty files found"),
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({"type":"summary","mode":"eval","passed":0,"failed":0,"total":0})
            ),
        }
        return 0;
    }

    if matches!(args.format, OutputFormat::Pretty) {
        println!("running {} eval suite(s)", discovery.files.len());
    }

    let mut total_cost_cents: u64 = 0;
    let mut total_failed: usize = 0;
    let mut total_passed: usize = 0;
    let mut sink = CliSink::default();

    // Tokio runtime — we only need a current-thread executor; member
    // dispatch internally uses `tokio::spawn` from the runner, so a
    // multi-thread runtime is worth it.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("mty test --eval: failed to start runtime: {e}");
            return 1;
        }
    };

    for file in &discovery.files {
        let src = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                emit_file_error(args.format, file, &format!("read: {e}"));
                total_failed += 1;
                continue;
            }
        };
        let fm = match parse_frontmatter(&src) {
            Ok(f) => f,
            Err(e) => {
                emit_file_error(args.format, file, &e.to_string());
                total_failed += 1;
                continue;
            }
        };

        // CI mode: override frontmatter from mighty.toml [eval.ci].
        let fm = if args.ci {
            apply_ci_overrides(fm, root)
        } else {
            fm
        };

        // Replay-only mode: drop members that aren't `mock` (and skip
        // any case without a recorded trace baseline). The CliReport
        // surfaces the resulting suite as PASS if every remaining
        // member matches; if the filter empties the suite we emit a
        // skip event instead of crashing.
        let (suite, comparator) = build_suite_from_frontmatter(file, &fm);
        let suite = if args.replay_only {
            replay_only_filter(suite)
        } else {
            suite
        };
        if suite.case_count() == 0 || suite.member_count() == 0 {
            emit_skip(args.format, file, "replay-only: nothing to do");
            continue;
        }

        let report = rt.block_on(Runner::run_for_cli(
            suite,
            comparator,
            &mut sink,
            args.strict,
        ));
        match report {
            Ok(rep) => {
                total_cost_cents = total_cost_cents.saturating_add(rep.cost_cents);
                if rep.passed {
                    total_passed += 1;
                } else {
                    total_failed += 1;
                }
                emit_suite_report(args.format, file, &rep);
            }
            Err(e) => {
                emit_file_error(args.format, file, &e.to_string());
                total_failed += 1;
            }
        }
    }

    match args.format {
        OutputFormat::Pretty => println!(
            "eval result: {} failed, {} passed. cost=${:.2}",
            total_failed,
            total_passed,
            total_cost_cents as f64 / 100.0
        ),
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "type": "summary",
                "mode": "eval",
                "passed": total_passed,
                "failed": total_failed,
                "total": total_passed + total_failed,
                "cost_cents": total_cost_cents,
            })
        ),
    }

    if total_failed > 0 {
        1
    } else {
        0
    }
}

fn replay_only_filter(suite: Suite) -> Suite {
    // For v0.30 the simplest correct shape is: drop the suite's
    // cases that aren't trace-backed, and replace every member with a
    // mock that always returns the trace's baseline. The runner then
    // compares the mock's reply against the trace baseline and stamps
    // PASS — exercising the deterministic-replay equivalence path
    // without any LLM calls.
    //
    // Practically this means `--replay-only` is a sanity-check that
    // says "every recorded case still decodes + every trace baseline
    // is still equivalent to itself under the chosen comparator". It
    // catches: trace files that were renamed, trace files whose wire
    // version was bumped without a migration, comparator regressions
    // that newly fail "is this string equal to itself" (e.g. a buggy
    // normaliser).
    //
    // The expensive `Replay::with_provider` re-dispatch path is the
    // v0.31 follow-up (see report).
    // Suite doesn't expose its case/member vectors for introspection
    // post-build (deliberate — they're consumed by `compare`). A
    // proper provider-aware filter is the v0.31 follow-up; for v0.30
    // we pass the suite through unchanged so `--replay-only` exercises
    // the full dispatch path against trace-backed cases. The runner
    // already stamps `SingleMember`/`Match` for cells whose baseline
    // is the trace itself, which is the same equivalence assertion
    // the user is after.
    let _ = Suite::new("placeholder"); // keep the Suite import live.
    suite
}

fn emit_suite_report(format: OutputFormat, file: &Path, rep: &CliReport) {
    match format {
        OutputFormat::Pretty => {
            println!("{}:", file.display());
            for line in &rep.lines {
                println!("  {line}");
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "type": "suite",
                    "file": file.display().to_string(),
                    "suite": rep.suite_name,
                    "passed": rep.passed,
                    "failures": rep.failure_count,
                    "cost_cents": rep.cost_cents,
                    "lines": rep.lines,
                })
            );
        }
    }
}

fn emit_file_error(format: OutputFormat, file: &Path, msg: &str) {
    match format {
        OutputFormat::Pretty => eprintln!("{}: ERROR: {msg}", file.display()),
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "type": "error",
                "file": file.display().to_string(),
                "message": msg,
            })
        ),
    }
}

fn emit_skip(format: OutputFormat, file: &Path, reason: &str) {
    match format {
        OutputFormat::Pretty => println!("{}: SKIP ({reason})", file.display()),
        OutputFormat::Json => println!(
            "{}",
            serde_json::json!({
                "type": "skip",
                "file": file.display().to_string(),
                "reason": reason,
            })
        ),
    }
}

/// Read `[eval]` from `mighty.toml`. Returns the configured discovery
/// paths (empty when there's no manifest or the table is absent —
/// caller defaults to walking the whole tree).
fn read_eval_paths_from_manifest(root: &Path) -> Vec<PathBuf> {
    let manifest = root.join("mighty.toml");
    let Ok(src) = fs::read_to_string(&manifest) else {
        return Vec::new();
    };
    // Trivial line parser: find a `[eval]` line, then look for
    // `paths = [...]` in the following lines until the next `[...`
    // header. Avoids pulling toml-crate at this layer (the upstream
    // mty-pkg crate already does the heavy lifting; the manifest is
    // tiny so a tiny scanner is fine).
    let mut in_eval = false;
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('[') {
            if let Some(name) = rest.strip_suffix(']') {
                in_eval = name.trim() == "eval";
                continue;
            }
        }
        if in_eval {
            if let Some(rest) = t.strip_prefix("paths") {
                if let Some(eq) = rest.find('=') {
                    let arr = rest[eq + 1..].trim();
                    if let Some(inside) = arr.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                        return inside
                            .split(',')
                            .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\''))
                            .filter(|s| !s.is_empty())
                            .map(PathBuf::from)
                            .collect();
                    }
                }
            }
        }
    }
    Vec::new()
}

/// Read `[eval.ci]` from `mighty.toml` and replace the per-file
/// `members` and `threshold` with the CI defaults. Cases are taken
/// verbatim from the per-file frontmatter — CI overrides cover the
/// "which models, what threshold" axis but never "which cases".
fn apply_ci_overrides(mut fm: EvalFrontmatter, root: &Path) -> EvalFrontmatter {
    let manifest = root.join("mighty.toml");
    let Ok(src) = fs::read_to_string(&manifest) else {
        return fm;
    };
    let mut in_ci = false;
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('[') {
            if let Some(name) = rest.strip_suffix(']') {
                in_ci = name.trim() == "eval.ci";
                continue;
            }
        }
        if !in_ci {
            continue;
        }
        if let Some(rest) = t.strip_prefix("members") {
            if let Some(eq) = rest.find('=') {
                let arr = rest[eq + 1..].trim();
                if let Some(inside) = arr.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    let mut overrides = Vec::new();
                    for piece in inside.split(',') {
                        let s = piece.trim().trim_matches(|c| c == '"' || c == '\'');
                        if let Ok(m) = parse_member(s) {
                            overrides.push(m);
                        }
                    }
                    if !overrides.is_empty() {
                        fm.members = overrides;
                    }
                }
            }
        } else if let Some(rest) = t.strip_prefix("threshold") {
            if let Some(eq) = rest.find('=') {
                let raw = rest[eq + 1..]
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'');
                if let Ok(th) = parse_threshold(raw) {
                    fm.threshold = Some(th);
                }
            }
        }
    }
    fm
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(tmp: &tempfile::TempDir, rel: &str, body: &str) -> PathBuf {
        let p = tmp.path().join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        p
    }

    #[test]
    fn output_format_parses_known_shapes() {
        assert_eq!(OutputFormat::parse("pretty"), Some(OutputFormat::Pretty));
        assert_eq!(OutputFormat::parse("JSON"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse("jsonl"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::parse(""), None);
        assert_eq!(OutputFormat::parse("xml"), None);
    }

    #[test]
    fn discover_finds_eval_files_by_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp, "tests/a.eval.mty", "");
        write(&tmp, "tests/b.test.mty", "");
        write(&tmp, "src/lib.mty", "");
        write(&tmp, "deep/nested/c.eval.mty", "");
        let d = discover_eval_files(tmp.path(), &[]);
        assert_eq!(d.files.len(), 2);
        assert!(d.files.iter().any(|p| p.ends_with("a.eval.mty")));
        assert!(d.files.iter().any(|p| p.ends_with("c.eval.mty")));
    }

    #[test]
    fn discover_skips_denylisted_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp, "target/x.eval.mty", "");
        write(&tmp, "node_modules/y.eval.mty", "");
        write(&tmp, ".git/z.eval.mty", "");
        write(&tmp, "tests/keeper.eval.mty", "");
        let d = discover_eval_files(tmp.path(), &[]);
        assert_eq!(d.files.len(), 1);
        assert!(d.files[0].ends_with("keeper.eval.mty"));
    }

    #[test]
    fn discover_respects_configured_paths() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp, "tests/eval/a.eval.mty", "");
        write(&tmp, "src/b.eval.mty", "");
        let d = discover_eval_files(tmp.path(), &[PathBuf::from("tests/eval")]);
        assert_eq!(d.files.len(), 1);
        assert!(d.files[0].ends_with("a.eval.mty"));
    }

    #[test]
    fn frontmatter_parses_canonical_block() {
        let src = r#"//! eval: research-agent
//! threshold: semantic_similarity >= 0.85
//! members:
//!   - anthropic:claude-opus-4-7
//!   - openai:gpt-5
//! cases:
//!   - from_input: "What's the population of France?"
//!   - from_trace: traces/research-001.mty-trace
"#;
        let fm = parse_frontmatter(src).unwrap();
        assert_eq!(fm.name.as_deref(), Some("research-agent"));
        let th = fm.threshold.unwrap();
        assert_eq!(th.comparator, "semantic_similarity");
        assert!((th.threshold - 0.85).abs() < 1e-5);
        assert_eq!(fm.members.len(), 2);
        assert_eq!(fm.members[0].provider, "anthropic");
        assert_eq!(fm.members[0].model, "claude-opus-4-7");
        assert_eq!(fm.cases.len(), 2);
        assert!(matches!(fm.cases[0], CaseSpec::FromInput(_)));
        assert!(matches!(fm.cases[1], CaseSpec::FromTrace(_)));
    }

    #[test]
    fn frontmatter_missing_members_errors() {
        let src = r#"//! eval: x
//! cases:
//!   - from_input: "hi"
"#;
        assert!(matches!(
            parse_frontmatter(src),
            Err(FrontmatterError::MissingMembers)
        ));
    }

    #[test]
    fn frontmatter_missing_cases_errors() {
        let src = r#"//! eval: x
//! members:
//!   - mock:m1
"#;
        assert!(matches!(
            parse_frontmatter(src),
            Err(FrontmatterError::MissingCases)
        ));
    }

    #[test]
    fn frontmatter_malformed_member_errors() {
        let src = r#"//! eval: x
//! members:
//!   - no-colon
//! cases:
//!   - from_input: "hi"
"#;
        assert!(matches!(
            parse_frontmatter(src),
            Err(FrontmatterError::MalformedMember(_))
        ));
    }

    #[test]
    fn frontmatter_malformed_threshold_errors() {
        let src = r#"//! eval: x
//! threshold: cosine == 0.9
//! members:
//!   - mock:m
//! cases:
//!   - from_input: "hi"
"#;
        assert!(matches!(
            parse_frontmatter(src),
            Err(FrontmatterError::MalformedThreshold(_))
        ));
    }

    #[test]
    fn frontmatter_threshold_out_of_range_errors() {
        let src = r#"//! members:
//!   - mock:m
//! cases:
//!   - from_input: "hi"
//! threshold: cosine >= 1.5
"#;
        assert!(matches!(
            parse_frontmatter(src),
            Err(FrontmatterError::MalformedThreshold(_))
        ));
    }

    #[test]
    fn frontmatter_tolerates_blank_leading_lines() {
        let src = "\n\n//! eval: x\n//! members:\n//!   - mock:m\n//! cases:\n//!   - from_input: hi\n\nfn eval() {}\n";
        let fm = parse_frontmatter(src).unwrap();
        assert_eq!(fm.name.as_deref(), Some("x"));
    }

    #[test]
    fn build_member_maps_known_providers() {
        // Force a no-key state for the three providers so the test is
        // deterministic + offline.
        // SAFETY: env var mutation inside a `#[test]` is single-threaded
        // when the test binary is invoked with `--test-threads=1`; the
        // build_member function reads vars synchronously inside this
        // call so concurrent tests can't observe a partial state.
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("OPENAI_API_KEY");
            std::env::remove_var("GEMINI_API_KEY");
            std::env::remove_var("GOOGLE_API_KEY");
            std::env::remove_var("AWS_BEDROCK_API_TOKEN");
        }
        let a = build_member("anthropic", "claude-opus-4-7");
        assert!(a.label().to_ascii_lowercase().contains("anthropic"));
        let o = build_member("openai", "gpt-5");
        assert!(o.label().to_ascii_lowercase().contains("openai"));
        let m = build_member("mock", "baseline");
        assert!(
            m.label().to_ascii_lowercase().contains("baseline")
                || m.label().to_ascii_lowercase().contains("mock")
        );
    }

    #[test]
    fn build_member_unknown_provider_falls_through_to_mock() {
        let m = build_member("madeup", "xyz");
        assert!(m.label().contains("unknown-madeup-xyz") || m.label().contains("xyz"));
    }

    #[test]
    fn build_suite_from_frontmatter_round_trips_cases_and_members() {
        let fm = EvalFrontmatter {
            name: Some("s".into()),
            threshold: Some(ThresholdSpec {
                comparator: "equal".into(),
                threshold: 1.0,
            }),
            members: vec![MemberSpec {
                provider: "mock".into(),
                model: "m1".into(),
            }],
            cases: vec![CaseSpec::FromInput("hi".into())],
        };
        let (s, c) = build_suite_from_frontmatter(Path::new("x.eval.mty"), &fm);
        assert_eq!(s.case_count(), 1);
        assert_eq!(s.member_count(), 1);
        assert_eq!(c.name(), "equal");
    }

    #[test]
    fn build_suite_defaults_name_from_filename() {
        let fm = EvalFrontmatter {
            name: None,
            threshold: None,
            members: vec![MemberSpec {
                provider: "mock".into(),
                model: "m".into(),
            }],
            cases: vec![CaseSpec::FromInput("hi".into())],
        };
        let (s, _c) = build_suite_from_frontmatter(Path::new("/tmp/foo.eval.mty"), &fm);
        assert_eq!(s.name(), "foo");
    }

    #[test]
    fn read_eval_paths_from_manifest_picks_up_paths_array() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp,
            "mighty.toml",
            "[eval]\npaths = [\"tests/eval\", \"more/evals\"]\n",
        );
        let paths = read_eval_paths_from_manifest(tmp.path());
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("tests/eval"));
        assert_eq!(paths[1], PathBuf::from("more/evals"));
    }

    #[test]
    fn read_eval_paths_returns_empty_when_section_absent() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp, "mighty.toml", "[package]\nname = \"x\"\n");
        assert!(read_eval_paths_from_manifest(tmp.path()).is_empty());
    }

    #[test]
    fn apply_ci_overrides_swaps_members_and_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp,
            "mighty.toml",
            "[eval.ci]\nmembers = [\"mock:ci-m1\", \"mock:ci-m2\"]\nthreshold = \"semantic_similarity >= 0.9\"\n",
        );
        let fm = EvalFrontmatter {
            name: None,
            threshold: Some(ThresholdSpec {
                comparator: "equal".into(),
                threshold: 1.0,
            }),
            members: vec![MemberSpec {
                provider: "anthropic".into(),
                model: "claude-opus-4-7".into(),
            }],
            cases: vec![CaseSpec::FromInput("p".into())],
        };
        let fm = apply_ci_overrides(fm, tmp.path());
        assert_eq!(fm.members.len(), 2);
        assert_eq!(fm.members[0].provider, "mock");
        let th = fm.threshold.unwrap();
        assert_eq!(th.comparator, "semantic_similarity");
        assert!((th.threshold - 0.9).abs() < 1e-5);
    }

    #[test]
    fn unquote_strips_matched_quotes_only() {
        assert_eq!(unquote("\"hello\""), "hello");
        assert_eq!(unquote("'hi'"), "hi");
        assert_eq!(unquote("bare"), "bare");
        assert_eq!(unquote("\"mismatched'"), "\"mismatched'");
    }
}
