//! `mty replay` — load a recorded trace and either summarize it,
//! dump every event as JSON, or step-replay it. v0.17 Tier 1.4
//! (see `docs/internals/agent-features-roadmap.md`).
//!
//! v0.19 adds byte-identical full re-execution via [`ReplayDriver`]
//! (gated behind `--byte-identical`, opt-in to avoid breaking the
//! v0.17 counting-handler step path; a future minor bump will flip
//! the default). When byte-identical is on, the CLI re-runs the
//! recorded program against the trace and reports per-event
//! divergences.
//!
//! See `docs/reference/cli/mty-replay.md` for the user-facing docs.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use mty_runtime::replay::{
    CountingStepHandler, LlmCallRef, ProvidedTurn, ReplayDriver, Replayer, TraceSummary,
};

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct ReplayArgs {
    pub trace: PathBuf,
    pub dump_json: bool,
    pub step: bool,
    pub json_summary: bool,
    /// v0.19: drive a fresh Runtime from the trace + assert each
    /// emitted event matches the recorded one. Implies `--step`.
    pub byte_identical: bool,
    /// v0.19: IO/Clock/Random reads return recorded bytes instead of
    /// touching the live host. Default `true` for byte-identical so
    /// replay is portable across machines.
    pub mock_io: bool,
    /// v0.19: path to the `.mty` source the trace was recorded
    /// against. Required for `--byte-identical` (the trace itself
    /// doesn't carry the program).
    pub program: Option<PathBuf>,
    /// v0.29 Track F: render an LLM-turn diff for one recorded turn.
    /// Pair with `--turn <id>` to address a specific turn; when
    /// `--turn` is omitted, the CLI lists every recorded LLM turn.
    /// The "live" side reuses the recorded reply — `--diff` is the
    /// inspection surface, eval drivers wire a real provider in.
    pub diff: bool,
    /// v0.29 Track F: address one specific `LlmCall.turn_id` for the
    /// `--diff` renderer. When set without `--diff`, the flag implies
    /// `--diff` so power users can type `mty replay <trace> --turn 7`.
    pub turn: Option<u64>,
}

/// CLI entry point. Returns a Unix-style process exit code.
pub fn run(args: ReplayArgs) -> i32 {
    let replayer = match Replayer::from_path(&args.trace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("mty replay: failed to load {}: {e}", args.trace.display());
            return 1;
        }
    };

    // Always run a self-consistency check up front. A malformed trace
    // would otherwise dump garbage JSON.
    if let Err(e) = replayer.verify_self_consistent() {
        eprintln!("mty replay: trace is not self-consistent: {e}");
        return 1;
    }

    // v0.29 Track F: --diff takes precedence over the other inspection
    // flags so the user can ask for one turn's diff without first
    // disabling --step etc. `--turn <id>` without `--diff` implies it.
    if args.diff || args.turn.is_some() {
        return run_diff(&args.trace, replayer, args.turn);
    }

    if args.dump_json {
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        match replayer.dump_json(&mut lock) {
            Ok(n) => {
                let _ = lock.flush();
                eprintln!("mty replay: dumped {} event(s)", n);
                0
            }
            Err(e) => {
                eprintln!("mty replay: dump failed: {e}");
                1
            }
        }
    } else if args.byte_identical {
        // v0.19: drive a fresh Runtime from the trace + assert each
        // emitted event matches the recorded one.
        let prog_path = match &args.program {
            Some(p) => p.clone(),
            None => {
                eprintln!(
                    "mty replay: --byte-identical requires --program <path-to.mty> \
(the trace alone doesn't carry the program source)"
                );
                return 2;
            }
        };
        run_byte_identical(&args.trace, replayer, &prog_path, args.mock_io)
    } else if args.step {
        let mut handler = CountingStepHandler::new();
        match replayer.step(&mut handler) {
            Ok(n) => {
                println!("{}", render_step_summary(n, &handler));
                0
            }
            Err(e) => {
                eprintln!("mty replay: step failed: {e}");
                1
            }
        }
    } else {
        let summary = replayer.summary();
        if args.json_summary {
            match serde_json::to_string_pretty(&summary) {
                Ok(s) => {
                    println!("{}", s);
                    0
                }
                Err(e) => {
                    eprintln!("mty replay: summary serialization failed: {e}");
                    1
                }
            }
        } else {
            print!("{}", render_summary(&summary, &args.trace));
            0
        }
    }
}

fn render_summary(s: &TraceSummary, path: &std::path::Path) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== Mighty replay trace ({}) ===\n",
        path.display()
    ));
    out.push_str(&format!(
        "  wire version : {}\n  runtime seed : {}\n  worker count : {}\n  created at   : {}\n",
        s.version,
        s.runtime_seed,
        s.worker_count,
        format_unix_ms(s.created_at_ms),
    ));
    out.push_str(&format!(
        "  events       : {} ({} agent(s))\n",
        s.event_count, s.agent_count
    ));
    out.push_str("  breakdown    :\n");
    let rows: [(&str, usize); 8] = [
        ("spawns           ", s.spawn_count),
        ("messages sent    ", s.message_sent_count),
        ("messages handled ", s.message_handled_count),
        ("io reads         ", s.io_read_count),
        ("clock reads      ", s.clock_read_count),
        ("random reads     ", s.random_read_count),
        ("budget exhausted ", s.budget_exhausted_count),
        ("exits            ", s.exit_count),
    ];
    for (label, n) in rows {
        out.push_str(&format!("    {} {:>6}\n", label, n));
    }
    out.push_str(&format!(
        "  handler elapsed (sum) : {} us\n",
        s.total_handler_elapsed_us
    ));
    out.push_str("\nFlags:\n");
    out.push_str("  --dump-json        emit every event as one JSON line\n");
    out.push_str("  --step             walk the trace through a step handler\n");
    out.push_str("  --json             emit this summary as JSON\n");
    out.push_str(
        "  --byte-identical   re-execute the program + assert each event matches (v0.19)\n",
    );
    out.push_str("  --mock-io          IO reads return recorded bytes instead of touching disk\n");
    out.push_str("  --program <path>   .mty source file (required with --byte-identical)\n");
    out.push_str(
        "  --diff             render LLM-turn diffs (v0.29; pair with --turn for one turn)\n",
    );
    out.push_str(
        "  --turn <id>        focus the diff on a single recorded LlmCall.turn_id (v0.29)\n",
    );
    out
}

/// v0.29 Track F: render the LLM-call diff for one (or every)
/// recorded turn in the trace.
///
/// The "live" side reuses the recorded reply byte-for-byte — this
/// CLI surface is the *inspection* path that the eval driver's
/// divergence reporter points at (`std.eval` plugs in a real
/// `MemberTurnProvider` when it wants live dispatch). Rendered as a
/// MATCH so the user can confirm the recorded shape; eval drivers
/// override the provider to surface real DIVERGE rows.
fn run_diff(trace_path: &std::path::Path, replayer: Replayer, turn: Option<u64>) -> i32 {
    let trace = replayer.trace().clone();
    let llm_count = trace.iter_llm_calls().count();
    if llm_count == 0 {
        eprintln!(
            "mty replay --diff: trace at {} contains no LlmCall events; \
             only v3 (v0.29+) traces produced via `MTY_RECORD_TRACE` carry structural LLM turns",
            trace_path.display()
        );
        return 2;
    }
    // The "live" provider replays the recorded reply verbatim, so the
    // CLI surface defaults to MATCH on every turn. Eval drivers swap
    // this provider for a real `MemberTurnProvider`.
    let driver = ReplayDriver::from_trace(trace).with_provider(
        |t: LlmCallRef<'_>| -> Result<ProvidedTurn, String> {
            Ok(ProvidedTurn {
                reply: t.reply.to_string(),
                tool_uses: t.tool_uses.to_vec(),
                cost_cents: t.cost_cents,
            })
        },
    );

    match turn {
        Some(id) => match driver.diff_llm_turn(id) {
            Ok(diff) => {
                print!("{}", diff.render());
                if diff.identical {
                    0
                } else {
                    1
                }
            }
            Err(e) => {
                eprintln!("mty replay --diff: {e}");
                1
            }
        },
        None => match driver.replay_llm_turns() {
            Ok(turns) => {
                println!(
                    "=== LLM-turn diff sweep ({}) — {} turn(s) ===",
                    trace_path.display(),
                    turns.len()
                );
                let mut any_diverge = false;
                for t in &turns {
                    let verdict = if t.replies_match() {
                        "MATCH"
                    } else {
                        any_diverge = true;
                        "DIVERGE"
                    };
                    println!("  turn #{:<4} : {}", t.turn_id, verdict);
                }
                if any_diverge {
                    1
                } else {
                    0
                }
            }
            Err(e) => {
                eprintln!("mty replay --diff: {e}");
                1
            }
        },
    }
}

/// v0.19: drive a fresh Runtime from the trace via [`ReplayDriver`],
/// report the byte-identical diff, and exit non-zero if any mismatch
/// was detected.
fn run_byte_identical(
    trace_path: &std::path::Path,
    replayer: Replayer,
    program_path: &std::path::Path,
    mock_io: bool,
) -> i32 {
    let src = match std::fs::read_to_string(program_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "mty replay: failed to read program {}: {e}",
                program_path.display()
            );
            return 1;
        }
    };
    let prog = match compile_program(&src, program_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "mty replay: failed to compile {}: {e}",
                program_path.display()
            );
            return 1;
        }
    };
    let trace = replayer.trace().clone();
    let mut driver = ReplayDriver::from_trace(trace)
        .with_program(prog)
        .mock_io(mock_io);
    let report = match driver.replay_all() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("mty replay: byte-identical replay failed: {e}");
            return 1;
        }
    };
    println!("=== Byte-identical replay ({}) ===", trace_path.display());
    println!("  events recorded: {}", driver.trace().events.len());
    println!("  events replayed: {}", report.events_replayed);
    println!("  mismatches     : {}", report.mismatch_count());
    println!();
    println!("{}", report.render());
    if report.success {
        0
    } else {
        1
    }
}

/// Compile a `.mty` source file into a SIR `Program`. Mirrors the
/// `compile()` helper in `crates/mty-runtime/tests/replay_e2e.rs`;
/// kept local to avoid widening the public surface of `mty-driver`.
fn compile_program(src: &str, path: &std::path::Path) -> Result<Arc<mty_ir::ir::Program>, String> {
    use mty_driver::pipeline::{lower, lower_to_sir, parse_source, type_and_borrow_check};
    let parsed = parse_source(src.to_string(), path.display().to_string());
    let (pkg, _diags) = lower(&parsed);
    let _ = type_and_borrow_check(&pkg);
    let (prog, _diags) = lower_to_sir(&pkg);
    Ok(Arc::new(prog))
}

fn render_step_summary(n: usize, h: &CountingStepHandler) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Step replay finished: {} event(s) ===\n", n));
    out.push_str(&format!("  spawns           {}\n", h.spawn_count));
    out.push_str(&format!("  messages sent    {}\n", h.message_sent_count));
    out.push_str(&format!("  messages handled {}\n", h.message_handled_count));
    out.push_str(&format!("  io reads         {}\n", h.io_read_count));
    out.push_str(&format!("  clock reads      {}\n", h.clock_read_count));
    out.push_str(&format!("  random reads     {}\n", h.random_read_count));
    out.push_str(&format!(
        "  budget exhausted {}\n",
        h.budget_exhausted_count
    ));
    out.push_str(&format!("  exits            {}\n", h.exit_count));
    out
}

// Same formatter as `mty inspect` — we keep an in-crate copy rather
// than re-exporting one to avoid coupling unrelated commands.
fn format_unix_ms(ms: u64) -> String {
    let secs = ms / 1000;
    let (y, mo, d, h, mi, s) = unix_secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn unix_secs_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    let mut z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };
    z = yr;
    (z as u64, mo as u64, d as u64, h, mi, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mty_runtime::replay::{Recorder, TraceEvent};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);
    fn tmp_path(label: &str) -> PathBuf {
        let n = N.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("mty-replay-cli-{}-{}.bin", label, n));
        p
    }

    #[test]
    fn summary_renders_known_fields() {
        let s = TraceSummary {
            version: 1,
            created_at_ms: 1_779_753_600 * 1000,
            runtime_seed: 42,
            worker_count: 4,
            event_count: 9,
            agent_count: 2,
            spawn_count: 2,
            message_sent_count: 1,
            message_handled_count: 1,
            io_read_count: 1,
            clock_read_count: 1,
            random_read_count: 1,
            budget_exhausted_count: 1,
            exit_count: 1,
            total_handler_elapsed_us: 12345,
            llm_call_count: 0,
        };
        let out = render_summary(&s, std::path::Path::new("/tmp/x.bin"));
        assert!(out.contains("wire version : 1"));
        assert!(out.contains("runtime seed : 42"));
        assert!(out.contains("2026-05-26T00:00:00Z"));
        assert!(out.contains("events       : 9"));
        assert!(out.contains("12345"));
    }

    fn default_args(trace: PathBuf) -> ReplayArgs {
        ReplayArgs {
            trace,
            dump_json: false,
            step: false,
            json_summary: false,
            byte_identical: false,
            mock_io: true,
            program: None,
            diff: false,
            turn: None,
        }
    }

    #[test]
    fn run_with_invalid_path_returns_1() {
        let code = run(default_args(PathBuf::from("/does/not/exist-replay.bin")));
        assert_eq!(code, 1);
    }

    #[test]
    fn run_default_summary_succeeds_on_real_trace() {
        let path = tmp_path("ok");
        let r = Recorder::new(&path, 0, 1);
        r.record_spawn(1, "Echo", None);
        r.record_message_sent(0, 1, "Ping", vec![]);
        r.record_message_handled(1, "Ping", 5);
        r.record_exit(1, "normal");
        r.flush_to_disk().unwrap();

        let code = run(default_args(path.clone()));
        assert_eq!(code, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn byte_identical_without_program_returns_2() {
        // The CLI rejects --byte-identical without --program.
        let path = tmp_path("bi_no_prog");
        let r = Recorder::new(&path, 0, 1);
        r.record_spawn(1, "Echoer", None);
        r.flush_to_disk().unwrap();

        let mut args = default_args(path.clone());
        args.byte_identical = true;
        args.program = None;
        let code = run(args);
        assert_eq!(code, 2, "expected exit code 2 for missing --program");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn step_summary_renders_counters() {
        let mut h = CountingStepHandler::new();
        h.spawn_count = 1;
        h.message_handled_count = 3;
        let s = render_step_summary(4, &h);
        assert!(s.contains("4 event(s)"));
        assert!(s.contains("spawns           1"));
        assert!(s.contains("messages handled 3"));
    }

    // -------------------------------------------------------------------------
    // v0.29 Track F: --diff + --turn CLI tests
    // -------------------------------------------------------------------------

    fn make_trace_with_llm_calls(path: &std::path::Path) {
        let r = Recorder::new(path, 0, 1);
        r.record_llm_call(0, None, "q1", Some("sys"), vec![], "a1", vec![], 1);
        r.record_llm_call(0, None, "q2", None, vec!["search".into()], "a2", vec![], 2);
        r.record_llm_call(0, None, "q3", None, vec![], "a3", vec![], 1);
        r.flush_to_disk().unwrap();
    }

    #[test]
    fn diff_sweep_renders_all_turns_when_no_turn_id() {
        let path = tmp_path("diff_sweep");
        make_trace_with_llm_calls(&path);
        let mut args = default_args(path.clone());
        args.diff = true;
        let code = run(args);
        // Default provider mirrors recorded reply → every turn MATCH → exit 0.
        assert_eq!(code, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn diff_turn_addresses_one_recorded_turn() {
        let path = tmp_path("diff_turn");
        make_trace_with_llm_calls(&path);
        let mut args = default_args(path.clone());
        args.diff = true;
        args.turn = Some(1);
        let code = run(args);
        assert_eq!(code, 0, "exit 0 on MATCH");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn turn_implies_diff() {
        // Setting --turn without --diff should still route to the diff
        // renderer (power-user shortcut).
        let path = tmp_path("turn_implies_diff");
        make_trace_with_llm_calls(&path);
        let mut args = default_args(path.clone());
        args.diff = false;
        args.turn = Some(2);
        let code = run(args);
        assert_eq!(code, 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn diff_with_no_llm_events_returns_2() {
        let path = tmp_path("diff_no_llm");
        let r = Recorder::new(&path, 0, 1);
        r.record_spawn(1, "Echo", None);
        r.flush_to_disk().unwrap();
        let mut args = default_args(path.clone());
        args.diff = true;
        let code = run(args);
        assert_eq!(code, 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn diff_unknown_turn_id_returns_1() {
        let path = tmp_path("diff_bad_turn");
        make_trace_with_llm_calls(&path);
        let mut args = default_args(path.clone());
        args.turn = Some(9_999);
        let code = run(args);
        assert_eq!(code, 1, "unknown turn id should exit 1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn step_handler_visits_recorded_event_in_correct_order() {
        // Sanity-check we actually get the recorded order back —
        // this is the core determinism guarantee.
        let path = tmp_path("order");
        let r = Recorder::new(&path, 0, 1);
        r.record_spawn(1, "Echo", None);
        r.record_message_sent(0, 1, "A", vec![]);
        r.record_message_sent(0, 1, "B", vec![]);
        r.flush_to_disk().unwrap();

        let replayer = Replayer::from_path(&path).unwrap();
        struct OrderCheck(Vec<String>);
        impl mty_runtime::replay::StepHandler for OrderCheck {
            fn on_event(&mut self, _i: usize, ev: &TraceEvent) -> Result<(), String> {
                match ev {
                    TraceEvent::Spawn { .. } => self.0.push("spawn".into()),
                    TraceEvent::MessageSent { msg, .. } => self.0.push(format!("send:{msg}")),
                    _ => {}
                }
                Ok(())
            }
        }
        let mut h = OrderCheck(Vec::new());
        replayer.step(&mut h).unwrap();
        assert_eq!(h.0, vec!["spawn", "send:A", "send:B"]);
        let _ = std::fs::remove_file(&path);
    }
}
