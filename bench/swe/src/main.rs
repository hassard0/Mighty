//! Mighty SWE-bench Verified harness.
//!
//! Entry point for `make bench-smoke` and `make bench-full`. See
//! `bench/swe/README.md` for the user-facing usage notes and
//! `docs/internals/benchmarks.md` for the methodology.

mod agent;
mod dataset;
mod llm;
mod problems;
mod scorer;
mod tools;
mod workspace;

use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use serde::Serialize;

use crate::agent::{run_agent, AgentConfig, StopReason};
use crate::llm::Client;
use crate::problems::{SmokeProblem, SMOKE_PROBLEMS, SMOKE_SUBSET_VERSION};
use crate::scorer::{score, Outcome};
use crate::tools::Workspace;
use crate::workspace::GitWorkspace;

#[derive(Parser, Debug)]
#[command(name = "mty-swe-bench")]
#[command(about = "Mighty SWE-bench Verified smoke runner", long_about = None)]
struct Cli {
    /// Number of smoke problems to run (defaults to 10 — the full
    /// curated subset). Ignored when `--all` is set.
    #[arg(long, default_value_t = 10)]
    num_problems: usize,

    /// Run the entire SWE-bench Verified set (~500 problems).
    /// GATED — only invoke this from `make bench-full` after
    /// confirming the spend. Refuses to run when MTY_BENCH_FULL_CONFIRM
    /// isn't set, as a second-line guard.
    #[arg(long)]
    all: bool,

    /// LLM target. Format: `<provider>:<model>` (e.g.
    /// `anthropic:claude-opus-4-7`).
    #[arg(long, default_value = "anthropic:claude-opus-4-7")]
    member: String,

    /// Hard ceiling on total spend in USD. The harness aborts with
    /// partial results when the cap is hit. Default: 25 USD (matches
    /// the user-authorised smoke budget).
    #[arg(long, default_value_t = 25.0)]
    dollar_cap: f64,

    /// Per-instance dollar cap (default 3 USD).
    #[arg(long, default_value_t = 3.0)]
    per_instance_cap: f64,

    /// Working directory for git clones + results.
    #[arg(long, default_value = "./.swe-work")]
    work_dir: PathBuf,

    /// Output path for the JSON results file.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Per-instance turn budget.
    #[arg(long, default_value_t = 25)]
    max_turns: u32,

    /// Per-instance wall-clock cap (seconds).
    #[arg(long, default_value_t = 300)]
    max_seconds: u64,
}

#[derive(Debug, Serialize)]
struct RunReport {
    mighty_commit: String,
    started_at: String,
    finished_at: String,
    subset_version: u32,
    model: String,
    num_problems: usize,
    dollar_cap: f64,
    total_cost_usd: f64,
    total_wall_seconds: f64,
    pass_count: usize,
    fail_count: usize,
    error_count: usize,
    skipped_count: usize,
    results: Vec<InstanceReport>,
}

#[derive(Debug, Serialize)]
struct InstanceReport {
    instance_id: String,
    repo: String,
    base_commit: String,
    difficulty: String,
    outcome: Outcome,
    stop_reason: StopReason,
    turns: u32,
    submitted: bool,
    final_summary: Option<String>,
    cost_usd: f64,
    agent_wall_seconds: f64,
    score_wall_seconds: f64,
    input_tokens: u64,
    output_tokens: u64,
    agent_patch_preview: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    if cli.all && std::env::var("MTY_BENCH_FULL_CONFIRM").is_err() {
        eprintln!(
            "REFUSING --all without MTY_BENCH_FULL_CONFIRM=1. \
             Run 'make bench-full' which gates on a typed confirmation."
        );
        std::process::exit(2);
    }

    let (provider, model) = parse_member(&cli.member)?;
    if provider != "anthropic" {
        anyhow::bail!(
            "v0.30 smoke supports anthropic only; v0.31 will add openai+gemini. \
             Got: {}",
            provider
        );
    }

    // Resolve the smoke subset. --all is a v0.31 stretch — for v0.30
    // we still ship the gate but warn that it falls back to the
    // curated 10 until the dataset bulk-pull lands.
    let problems: Vec<&SmokeProblem> = if cli.all {
        tracing::warn!(
            "--all requested; v0.30 ships only the 10-problem smoke. Treating --all as smoke."
        );
        SMOKE_PROBLEMS.iter().collect()
    } else {
        SMOKE_PROBLEMS.iter().take(cli.num_problems).collect()
    };

    tokio::fs::create_dir_all(&cli.work_dir).await.ok();
    let clone_dir = cli.work_dir.join("checkouts");
    let crate_root: PathBuf = std::env::current_dir()?;

    // Fail loud if no key — we'd rather not consume the budget gating
    // somewhere subtler.
    let client = Client::from_env()?;

    let started_at = Utc::now();
    let mut total_cost = 0.0_f64;
    let mut results = Vec::new();
    let mut pass = 0_usize;
    let mut fail = 0_usize;
    let mut errors = 0_usize;
    let mut skipped = 0_usize;
    let start_wall = std::time::Instant::now();

    for (i, smoke) in problems.iter().enumerate() {
        println!(
            "\n=== [{}/{}] {} ({}/{}) ===",
            i + 1,
            problems.len(),
            smoke.instance_id,
            smoke.repo,
            &smoke.base_commit[..8.min(smoke.base_commit.len())]
        );
        if total_cost >= cli.dollar_cap {
            println!(
                "GLOBAL DOLLAR CAP HIT (${:.2} >= ${:.2}); aborting with partial results",
                total_cost, cli.dollar_cap
            );
            break;
        }

        let inst = match dataset::load_instance(&crate_root, smoke).await {
            Ok(i) => i,
            Err(e) => {
                println!("  dataset fetch failed: {e}");
                errors += 1;
                results.push(InstanceReport {
                    instance_id: smoke.instance_id.into(),
                    repo: smoke.repo.into(),
                    base_commit: smoke.base_commit.into(),
                    difficulty: format!("{:?}", smoke.difficulty).to_lowercase(),
                    outcome: Outcome::ScoringSkipped {
                        reason: format!("dataset load: {e}"),
                    },
                    stop_reason: StopReason::ApiError(format!("dataset: {e}")),
                    turns: 0,
                    submitted: false,
                    final_summary: None,
                    cost_usd: 0.0,
                    agent_wall_seconds: 0.0,
                    score_wall_seconds: 0.0,
                    input_tokens: 0,
                    output_tokens: 0,
                    agent_patch_preview: String::new(),
                });
                continue;
            }
        };

        let ws = match GitWorkspace::clone_at(&clone_dir, &inst.repo, &inst.base_commit).await {
            Ok(w) => w,
            Err(e) => {
                println!("  git clone failed: {e}");
                skipped += 1;
                results.push(InstanceReport {
                    instance_id: smoke.instance_id.into(),
                    repo: smoke.repo.into(),
                    base_commit: smoke.base_commit.into(),
                    difficulty: format!("{:?}", smoke.difficulty).to_lowercase(),
                    outcome: Outcome::ScoringSkipped {
                        reason: format!("git clone: {e}"),
                    },
                    stop_reason: StopReason::ApiError(format!("clone: {e}")),
                    turns: 0,
                    submitted: false,
                    final_summary: None,
                    cost_usd: 0.0,
                    agent_wall_seconds: 0.0,
                    score_wall_seconds: 0.0,
                    input_tokens: 0,
                    output_tokens: 0,
                    agent_patch_preview: String::new(),
                });
                continue;
            }
        };
        let _ = ws.reset().await;

        let agent_ws = Workspace::new(ws.root.clone());
        let config = AgentConfig {
            model: model.clone(),
            max_turns: cli.max_turns,
            max_seconds: cli.max_seconds,
            max_tokens_per_call: 4096,
            per_instance_dollar_cap: cli.per_instance_cap,
        };
        let run = match run_agent(
            &client,
            &agent_ws,
            &inst.problem_statement,
            &inst.fail_to_pass,
            &inst.instance_id,
            &config,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                println!("  agent run errored: {e}");
                errors += 1;
                results.push(InstanceReport {
                    instance_id: smoke.instance_id.into(),
                    repo: smoke.repo.into(),
                    base_commit: smoke.base_commit.into(),
                    difficulty: format!("{:?}", smoke.difficulty).to_lowercase(),
                    outcome: Outcome::Failed {
                        details: format!("agent: {e}"),
                    },
                    stop_reason: StopReason::ApiError(format!("agent: {e}")),
                    turns: 0,
                    submitted: false,
                    final_summary: None,
                    cost_usd: 0.0,
                    agent_wall_seconds: 0.0,
                    score_wall_seconds: 0.0,
                    input_tokens: 0,
                    output_tokens: 0,
                    agent_patch_preview: String::new(),
                });
                continue;
            }
        };
        total_cost += run.cost_usd;
        let score_res = match score(&ws, &inst, run.submitted).await {
            Ok(s) => s,
            Err(e) => {
                println!("  scoring errored: {e}");
                errors += 1;
                results.push(InstanceReport {
                    instance_id: smoke.instance_id.into(),
                    repo: smoke.repo.into(),
                    base_commit: smoke.base_commit.into(),
                    difficulty: format!("{:?}", smoke.difficulty).to_lowercase(),
                    outcome: Outcome::Failed {
                        details: format!("scorer: {e}"),
                    },
                    stop_reason: run.stop_reason.clone(),
                    turns: run.turns,
                    submitted: run.submitted,
                    final_summary: run.final_summary.clone(),
                    cost_usd: run.cost_usd,
                    agent_wall_seconds: run.wall_time.as_secs_f64(),
                    score_wall_seconds: 0.0,
                    input_tokens: run.usage.input_tokens,
                    output_tokens: run.usage.output_tokens,
                    agent_patch_preview: String::new(),
                });
                continue;
            }
        };

        match &score_res.outcome {
            Outcome::Passed => pass += 1,
            Outcome::Failed { .. } => fail += 1,
            Outcome::AgentDidNotSubmit => fail += 1,
            Outcome::ScoringSkipped { .. } => skipped += 1,
        }

        println!(
            "  -> {} | turns={} cost=${:.4} wall={:.1}s",
            outcome_tag(&score_res.outcome),
            run.turns,
            run.cost_usd,
            run.wall_time.as_secs_f64()
        );

        let preview = score_res
            .agent_patch
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n");
        results.push(InstanceReport {
            instance_id: smoke.instance_id.into(),
            repo: smoke.repo.into(),
            base_commit: smoke.base_commit.into(),
            difficulty: format!("{:?}", smoke.difficulty).to_lowercase(),
            outcome: score_res.outcome,
            stop_reason: run.stop_reason,
            turns: run.turns,
            submitted: run.submitted,
            final_summary: run.final_summary,
            cost_usd: run.cost_usd,
            agent_wall_seconds: run.wall_time.as_secs_f64(),
            score_wall_seconds: score_res.wall_time.as_secs_f64(),
            input_tokens: run.usage.input_tokens,
            output_tokens: run.usage.output_tokens,
            agent_patch_preview: preview,
        });
    }

    let finished_at = Utc::now();
    let total_wall = start_wall.elapsed().as_secs_f64();
    let mighty_commit = git_head_short().unwrap_or_else(|| "unknown".into());
    let report = RunReport {
        mighty_commit: mighty_commit.clone(),
        started_at: started_at.to_rfc3339(),
        finished_at: finished_at.to_rfc3339(),
        subset_version: SMOKE_SUBSET_VERSION,
        model: model.clone(),
        num_problems: problems.len(),
        dollar_cap: cli.dollar_cap,
        total_cost_usd: total_cost,
        total_wall_seconds: total_wall,
        pass_count: pass,
        fail_count: fail,
        error_count: errors,
        skipped_count: skipped,
        results,
    };

    let out_path = cli.output.unwrap_or_else(|| {
        cli.work_dir.join("..").join("results").join(format!(
            "{}_{}.json",
            &mighty_commit[..8.min(mighty_commit.len())],
            Utc::now().format("%Y%m%d_%H%M%S")
        ))
    });
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&out_path, serde_json::to_vec_pretty(&report)?)
        .await
        .with_context(|| format!("write results to {}", out_path.display()))?;

    println!(
        "\n==============================\n\
         RESULT: {}/{} passed ({} failed, {} skipped, {} errors)\n\
         Cost: ${:.2} | Wall: {:.0}s\n\
         Written to: {}\n\
         ==============================",
        pass,
        problems.len(),
        fail,
        skipped,
        errors,
        total_cost,
        total_wall,
        out_path.display()
    );

    Ok(())
}

fn parse_member(s: &str) -> Result<(String, String)> {
    let (provider, model) = s
        .split_once(':')
        .with_context(|| format!("--member must be `<provider>:<model>`, got `{s}`"))?;
    Ok((provider.into(), model.into()))
}

fn outcome_tag(o: &Outcome) -> &'static str {
    match o {
        Outcome::Passed => "PASS",
        Outcome::Failed { .. } => "FAIL",
        Outcome::AgentDidNotSubmit => "NOSUBMIT",
        Outcome::ScoringSkipped { .. } => "SKIP",
    }
}

fn git_head_short() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
