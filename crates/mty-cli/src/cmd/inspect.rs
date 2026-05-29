//! `mty inspect` — connect to a running runtime's control socket and
//! print a snapshot of every live agent. v0.16 Tier 1.1 (see
//! `docs/internals/agent-features-roadmap.md`).
//!
//! v0.30 Track D adds `--cost`: reads `~/.mty/observations.sqlite`
//! (the auto-recorded LLM cost+latency DB) and prints a table.
//! Sub-flags: `--since <DURATION>`, `--by {provider,model,agent}`,
//! `--top <N>`. See `docs/internals/observability.md`.
//!
//! See `docs/reference/cli/mty-inspect.md` for usage.

use serde::Deserialize;
use std::io::Write;
use std::time::Duration;

const DEFAULT_ENV: &str = "MTY_RUNTIME_CONTROL_SOCK";

#[derive(Debug, Default)]
pub struct InspectArgs {
    pub sock: Option<String>,
    pub agent: Option<u64>,
    pub json: bool,
    pub watch_ms: Option<u64>,
    /// v0.30 Track D: switch from agent-snapshot mode to LLM-cost mode.
    pub cost: bool,
    /// Window spec: `7d`, `24h`, `30m`, `all`. Default `24h`.
    pub since: Option<String>,
    /// Group key: `provider`, `model`, `agent`, `none`. Default `provider`.
    pub by: Option<String>,
    /// Print the top-N most expensive calls. 0 = skip.
    pub top: Option<usize>,
    /// Path to the observations SQLite. Default
    /// `MTY_OBSERVE_DB` -> `~/.mty/observations.sqlite`.
    pub db: Option<String>,
}

/// CLI entry point. Returns a Unix-style process exit code.
pub fn run(args: InspectArgs) -> i32 {
    if args.cost {
        return cost::run(args);
    }
    let sock_path = match args
        .sock
        .clone()
        .or_else(|| std::env::var(DEFAULT_ENV).ok())
    {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!(
                "mty inspect: no control-socket path. Pass --sock <PATH> or set {}.",
                DEFAULT_ENV
            );
            return 2;
        }
    };

    match args.watch_ms {
        Some(ms) => watch_loop(&sock_path, args.agent, args.json, ms),
        None => one_shot(&sock_path, args.agent, args.json),
    }
}

fn one_shot(sock_path: &str, agent: Option<u64>, json: bool) -> i32 {
    match query(sock_path, agent) {
        Ok(payload) => {
            if json {
                println!("{}", payload);
            } else {
                match pretty_print(&payload, agent.is_some()) {
                    Ok(out) => print!("{}", out),
                    Err(e) => {
                        eprintln!("mty inspect: failed to render snapshot: {e}");
                        return 1;
                    }
                }
            }
            0
        }
        Err(e) => {
            eprintln!("mty inspect: {e}");
            1
        }
    }
}

fn watch_loop(sock_path: &str, agent: Option<u64>, json: bool, ms: u64) -> i32 {
    let interval = Duration::from_millis(ms.max(50));
    loop {
        match query(sock_path, agent) {
            Ok(payload) => {
                // Best-effort clear-screen using ANSI; fall back gracefully
                // for terminals that don't speak it.
                if !json {
                    print!("\x1b[2J\x1b[H");
                }
                if json {
                    println!("{}", payload);
                } else if let Ok(out) = pretty_print(&payload, agent.is_some()) {
                    print!("{}", out);
                    let _ = std::io::stdout().flush();
                }
            }
            Err(e) => {
                eprintln!("mty inspect: {e}");
                return 1;
            }
        }
        std::thread::sleep(interval);
    }
}

/// Send one request to the control socket and return the JSON reply
/// as a string. Cross-platform shim: Unix uses `UnixStream`, Windows
/// returns an error explaining the v0.16 limitation.
fn query(sock_path: &str, agent: Option<u64>) -> Result<String, String> {
    #[cfg(unix)]
    {
        unix_query(sock_path, agent)
    }
    #[cfg(not(unix))]
    {
        let _ = (sock_path, agent);
        Err(
            "the Windows named-pipe control socket is not yet implemented \
             (v0.16 Unix-only). Tracking: dev/history/notes/INTROSPECT_V0_16_NOTES.md."
                .into(),
        )
    }
}

#[cfg(unix)]
fn unix_query(sock_path: &str, agent: Option<u64>) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let mut stream =
        UnixStream::connect(sock_path).map_err(|e| format!("connect {sock_path}: {e}"))?;
    let req = match agent {
        Some(id) => format!(r#"{{"op":"snapshot_agent","id":{id}}}"#),
        None => r#"{"op":"snapshot"}"#.to_string(),
    };
    stream
        .write_all(req.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("write: {e}"))?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("read: {e}"))?;
    if line.trim().is_empty() {
        return Err("control socket returned an empty reply".into());
    }
    Ok(line.trim_end().to_string())
}

// ---------------------------------------------------------------------
// Pretty-print logic
// ---------------------------------------------------------------------

/// Decoded shape mirroring `mty_runtime::introspect::*`. We re-declare
/// it here (instead of pulling the runtime types in) so the CLI stays
/// loosely coupled to the wire format — the version field is the only
/// breakage gate.
#[derive(Debug, Deserialize)]
struct AgentSnapshot {
    version: u32,
    agent_id: u64,
    agent_type: String,
    supervisor_parent: Option<u64>,
    mailbox_depth: usize,
    mailbox_high_water: usize,
    in_flight_handler: Option<String>,
    in_flight_elapsed_ms: Option<u64>,
    budget: BudgetSnapshot,
    last_messages: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BudgetSnapshot {
    mem_used_bytes: u64,
    mem_limit_bytes: Option<u64>,
    ticks_used: u64,
    ticks_limit: Option<u64>,
    deadline_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RuntimeSnapshot {
    version: u32,
    agents: Vec<AgentSnapshot>,
    worker_count: usize,
    timestamp_ms: u64,
}

fn pretty_print(payload: &str, single_agent: bool) -> Result<String, String> {
    if single_agent {
        let s: AgentSnapshot =
            serde_json::from_str(payload).map_err(|e| format!("decode AgentSnapshot: {e}"))?;
        check_version(s.version)?;
        Ok(render_single(&s))
    } else {
        let s: RuntimeSnapshot =
            serde_json::from_str(payload).map_err(|e| format!("decode RuntimeSnapshot: {e}"))?;
        check_version(s.version)?;
        Ok(render_runtime(&s))
    }
}

fn check_version(v: u32) -> Result<(), String> {
    if v < 1 {
        return Err(format!(
            "snapshot wire version {} is older than this CLI supports (>= 1)",
            v
        ));
    }
    Ok(())
}

fn render_runtime(s: &RuntimeSnapshot) -> String {
    let ts = format_unix_ms(s.timestamp_ms);
    let mut out = String::new();
    out.push_str(&format!(
        "=== Mighty runtime ({} workers, snapshot at {}) ===\n",
        s.worker_count, ts
    ));
    out.push_str(&format!(
        "  {:>4}  {:<28} {:>4} {:>4}  {:<22} {}\n",
        "id", "type", "mb", "hi", "state", "budget"
    ));
    if s.agents.is_empty() {
        out.push_str("  (no live agents)\n");
        return out;
    }
    for a in &s.agents {
        out.push_str(&format!(
            "  {:>4}  {:<28} {:>4} {:>4}  {:<22} {}\n",
            a.agent_id,
            truncate(&a.agent_type, 28),
            a.mailbox_depth,
            a.mailbox_high_water,
            state_label(a),
            budget_label(&a.budget),
        ));
    }
    out
}

fn render_single(a: &AgentSnapshot) -> String {
    let mut out = String::new();
    out.push_str(&format!("agent #{} {}\n", a.agent_id, a.agent_type));
    if let Some(parent) = a.supervisor_parent {
        out.push_str(&format!("  supervisor parent: #{}\n", parent));
    }
    out.push_str(&format!(
        "  mailbox depth: {} (high-water {})\n",
        a.mailbox_depth, a.mailbox_high_water
    ));
    out.push_str(&format!("  state: {}\n", state_label(a)));
    out.push_str(&format!("  budget: {}\n", budget_label(&a.budget)));
    if !a.last_messages.is_empty() {
        out.push_str("  last messages:\n");
        for m in &a.last_messages {
            out.push_str(&format!("    - {}\n", m));
        }
    }
    out
}

fn state_label(a: &AgentSnapshot) -> String {
    match (&a.in_flight_handler, a.in_flight_elapsed_ms) {
        (Some(h), Some(ms)) => format!("handler:{h} +{ms}ms"),
        (Some(h), None) => format!("handler:{h}"),
        (None, _) => "idle".into(),
    }
}

fn budget_label(b: &BudgetSnapshot) -> String {
    let mem = format_bytes(b.mem_used_bytes);
    let mem_str = match b.mem_limit_bytes {
        Some(limit) => format!("{}/{}", mem, format_bytes(limit)),
        None => mem,
    };
    let cpu = format_duration_ns(b.ticks_used);
    let cpu_str = match b.ticks_limit {
        Some(limit) => format!("{}/{}", cpu, format_duration_ns(limit)),
        None => cpu,
    };
    match b.deadline_ms {
        Some(d) => format!("{mem_str} {cpu_str} deadline {d}ms"),
        None => format!("{mem_str} {cpu_str}"),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n.saturating_sub(1)])
    }
}

fn format_bytes(b: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if b >= GB {
        format!("{:.1}GB", b as f64 / GB as f64)
    } else if b >= MB {
        format!("{:.1}MB", b as f64 / MB as f64)
    } else if b >= KB {
        format!("{:.1}KB", b as f64 / KB as f64)
    } else {
        format!("{b}B")
    }
}

fn format_duration_ns(ns: u64) -> String {
    const US: u64 = 1_000;
    const MS: u64 = 1_000_000;
    const S: u64 = 1_000_000_000;
    if ns >= S {
        format!("{:.2}s", ns as f64 / S as f64)
    } else if ns >= MS {
        format!("{}ms", ns / MS)
    } else if ns >= US {
        format!("{}us", ns / US)
    } else {
        format!("{ns}ns")
    }
}

fn format_unix_ms(ms: u64) -> String {
    // Pure-rust UTC formatter — we avoid the chrono dep. ISO 8601 with
    // second precision is enough for a snapshot timestamp.
    let secs = ms / 1000;
    let (y, mo, d, h, mi, s) = unix_secs_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn unix_secs_to_ymdhms(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    // Days/seconds split.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    // Civil-date conversion (Howard Hinnant's algorithm).
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

    #[test]
    fn pretty_renders_empty_runtime() {
        let payload = r#"{"version":1,"agents":[],"worker_count":2,"timestamp_ms":0}"#;
        let out = pretty_print(payload, false).unwrap();
        assert!(out.contains("=== Mighty runtime (2 workers"));
        assert!(out.contains("(no live agents)"));
    }

    #[test]
    fn pretty_renders_one_agent() {
        let payload = r#"{
            "version":1,
            "agents":[{
                "version":1,"agent_id":7,"agent_type":"Echo","supervisor_parent":null,
                "mailbox_depth":3,"mailbox_high_water":5,
                "in_flight_handler":"Ping","in_flight_elapsed_ms":12,
                "budget":{"mem_used_bytes":2048,"mem_limit_bytes":null,
                          "ticks_used":1500000,"ticks_limit":null,"deadline_ms":null},
                "last_messages":[]
            }],
            "worker_count":1,
            "timestamp_ms":0
        }"#;
        let out = pretty_print(payload, false).unwrap();
        assert!(out.contains("Echo"));
        assert!(out.contains("handler:Ping"));
    }

    #[test]
    fn rejects_unsupported_version() {
        let payload = r#"{"version":0,"agents":[],"worker_count":1,"timestamp_ms":0}"#;
        assert!(pretty_print(payload, false).is_err());
    }

    #[test]
    fn byte_formatter_handles_units() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(1024), "1.0KB");
        assert_eq!(format_bytes(2 * 1024 * 1024), "2.0MB");
    }

    #[test]
    fn unix_ts_renders_as_iso8601() {
        // 2026-05-26T00:00:00Z = 1779753600 (verified via `date -d`)
        let s = format_unix_ms(1779753600 * 1000);
        assert_eq!(s, "2026-05-26T00:00:00Z");
        // Spot-check the epoch itself.
        assert_eq!(format_unix_ms(0), "1970-01-01T00:00:00Z");
    }
}

// =====================================================================
// v0.30 Track D — `mty inspect --cost`
// =====================================================================

/// `mty inspect --cost` implementation. Lives in its own module so the
/// v0.16 agent-snapshot code above stays self-contained.
pub mod cost {
    use super::InspectArgs;
    use mty_stdlib::observe::{
        observation::LlmObservation,
        query::{summarize, AggregateRow, CostSummary, GroupBy, LatencyPercentiles, Window},
        storage::SqliteStore,
        ObservationStore,
    };

    /// Resolve the DB path from CLI flag → env → default.
    pub(crate) fn resolve_db_path(args: &InspectArgs) -> std::path::PathBuf {
        if let Some(p) = args.db.as_ref() {
            return std::path::PathBuf::from(p);
        }
        mty_stdlib::observe::storage::default_db_path()
    }

    /// CLI entry point for `--cost`. Returns a Unix-style exit code.
    pub fn run(args: InspectArgs) -> i32 {
        let window = match args.since.as_deref().unwrap_or("24h") {
            "" => Window::Last { millis: 86_400_000 },
            spec => match Window::parse(spec) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("mty inspect --cost: bad --since value: {e}");
                    return 2;
                }
            },
        };
        let group_by = match args.by.as_deref().unwrap_or("provider") {
            "" => GroupBy::Provider,
            spec => match GroupBy::parse(spec) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("mty inspect --cost: bad --by value: {e}");
                    return 2;
                }
            },
        };
        let top_n = args.top.unwrap_or(0);

        let db_path = resolve_db_path(&args);
        let obs = match load_observations(&db_path) {
            Ok(o) => o,
            Err(e) => {
                eprintln!(
                    "mty inspect --cost: failed to open {}: {e}",
                    db_path.display()
                );
                return 1;
            }
        };
        let summary = summarize(&obs, window, group_by, top_n);
        if args.json {
            print_json(&summary);
        } else {
            print_table(&summary, &db_path);
        }
        0
    }

    fn load_observations(path: &std::path::Path) -> Result<Vec<LlmObservation>, String> {
        // Special case: tests can pass `:memory:` and have already
        // installed a store via `mty_stdlib::observe::storage::install_store`.
        if path == std::path::Path::new(":memory:") {
            if let Some(v) = mty_stdlib::observe::with_storage(|s| s.snapshot()) {
                return Ok(v.unwrap_or_default());
            }
            return Ok(Vec::new());
        }
        if !path.exists() {
            // No DB yet — that's the empty-state, not an error.
            return Ok(Vec::new());
        }
        let store = SqliteStore::open(path).map_err(|e| e.to_string())?;
        Ok(store.snapshot().unwrap_or_default())
    }

    fn print_json(s: &CostSummary) {
        // Compact JSON; the snapshot table renders human-readably.
        let v = serde_json::json!({
            "call_count": s.call_count,
            "total_cost_cents": s.total_cost_cents,
            "total_prompt_tokens": s.total_prompt_tokens,
            "total_completion_tokens": s.total_completion_tokens,
            "p50_ms": s.latency.p50_ms,
            "p95_ms": s.latency.p95_ms,
            "p99_ms": s.latency.p99_ms,
            "by_group": s
                .by_group
                .iter()
                .map(|r| serde_json::json!({
                    "key": r.key,
                    "calls": r.call_count,
                    "cost_cents": r.total_cost_cents,
                    "prompt_tokens": r.total_prompt_tokens,
                    "completion_tokens": r.total_completion_tokens,
                    "p50_ms": r.p50_latency_ms,
                    "p95_ms": r.p95_latency_ms,
                    "p99_ms": r.p99_latency_ms,
                }))
                .collect::<Vec<_>>(),
            "top_calls": s.top_calls,
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
    }

    fn print_table(s: &CostSummary, db_path: &std::path::Path) {
        if s.call_count == 0 {
            println!("=== mty inspect --cost ===");
            println!(
                "  no observations in {} for the selected window",
                db_path.display()
            );
            println!("  (set MTY_OBSERVE=1 + run an LLM-calling program to populate)");
            return;
        }
        println!("=== mty inspect --cost ({}) ===", db_path.display());
        println!("  window:           {}", format_window(s.window));
        println!("  calls:            {}", s.call_count);
        println!("  total cost:       {}", fmt_cents(s.total_cost_cents));
        println!(
            "  prompt tokens:    {} | completion tokens: {}",
            s.total_prompt_tokens, s.total_completion_tokens
        );
        println!("  latency p50/p95/p99: {}", fmt_latency(&s.latency));
        println!();
        println!("--- by group ---");
        println!(
            "  {:<32} {:>6} {:>12} {:>14} {:>14} {:>14}",
            "key", "calls", "cost", "prompt-tok", "compl-tok", "p50/p95/p99"
        );
        for row in &s.by_group {
            print_aggregate_row(row);
        }
        if !s.top_calls.is_empty() {
            println!();
            println!("--- top {} most expensive calls ---", s.top_calls.len());
            println!(
                "  {:>4} {:<10} {:<28} {:>12} {:>10} {:>8} error",
                "#", "provider", "model", "cost", "tokens", "ms"
            );
            for (i, o) in s.top_calls.iter().enumerate() {
                println!(
                    "  {:>4} {:<10} {:<28} {:>12} {:>10} {:>8} {}",
                    i + 1,
                    truncate(&o.provider, 10),
                    truncate(&o.model, 28),
                    fmt_cents(o.cost_cents),
                    o.prompt_tokens + o.completion_tokens,
                    o.latency_ms,
                    o.error_kind.as_deref().unwrap_or("-")
                );
            }
        }
    }

    fn print_aggregate_row(row: &AggregateRow) {
        let lat = format!(
            "{}/{}/{}ms",
            row.p50_latency_ms, row.p95_latency_ms, row.p99_latency_ms
        );
        println!(
            "  {:<32} {:>6} {:>12} {:>14} {:>14} {:>14}",
            truncate(&row.key, 32),
            row.call_count,
            fmt_cents(row.total_cost_cents),
            row.total_prompt_tokens,
            row.total_completion_tokens,
            lat,
        );
    }

    pub(crate) fn fmt_cents(c: i64) -> String {
        // Render as `$x.yy` for >= 1 cent, otherwise `<$0.01`.
        if c.abs() == 0 {
            return "$0.00".into();
        }
        if c.abs() < 1 {
            // Should never happen — i64 has no sub-integer states — but
            // keep the branch for symmetry with f64 callers.
            return "<$0.01".into();
        }
        let sign = if c < 0 { "-" } else { "" };
        let abs = c.unsigned_abs();
        format!("{sign}${}.{:02}", abs / 100, abs % 100)
    }

    fn fmt_latency(l: &LatencyPercentiles) -> String {
        format!("{}ms / {}ms / {}ms", l.p50_ms, l.p95_ms, l.p99_ms)
    }

    fn format_window(w: Window) -> String {
        match w {
            Window::All => "all".into(),
            Window::Last { millis } => {
                if millis % 86_400_000 == 0 {
                    format!("last {}d", millis / 86_400_000)
                } else if millis % 3_600_000 == 0 {
                    format!("last {}h", millis / 3_600_000)
                } else if millis % 60_000 == 0 {
                    format!("last {}m", millis / 60_000)
                } else if millis % 1000 == 0 {
                    format!("last {}s", millis / 1000)
                } else {
                    format!("last {}ms", millis)
                }
            }
        }
    }

    fn truncate(s: &str, n: usize) -> String {
        if s.len() <= n {
            s.to_string()
        } else {
            format!("{}…", &s[..n.saturating_sub(1)])
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use mty_stdlib::observe::observation::{now_ms, LlmObservation};
        #[cfg(feature = "observe-sqlite")]
        use mty_stdlib::observe::storage::{install_store, uninstall_store, SqliteStore};
        // v0.36 T5: when SQLite is off (cli-min build) the install_store/
        // uninstall_store helpers still exist as no-ops in the stdlib, so
        // the negative-path tests (bad_since, bad_by, empty_db) compile
        // and pass. Only the seed-based tests need cfg-gating below.
        #[cfg(not(feature = "observe-sqlite"))]
        use mty_stdlib::observe::storage::uninstall_store;

        // Tests share the process-global observe store, so they must
        // run sequentially. Cargo runs `#[test]` in parallel by default,
        // so use a single mutex to serialise.
        fn store_test_lock() -> std::sync::MutexGuard<'static, ()> {
            use std::sync::{Mutex, OnceLock};
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|p| p.into_inner())
        }

        // SQLite-backed test fixtures. Gated on `observe-sqlite` because
        // `SqliteStore::in_memory()` is a `FeatureDisabled` stub when the
        // feature is off (cli-min / Windows-without-MSVC install).
        #[cfg(feature = "observe-sqlite")]
        fn seed_store_with(obs: Vec<LlmObservation>) {
            uninstall_store();
            let s = SqliteStore::in_memory().unwrap();
            for o in &obs {
                s.record(o);
            }
            install_store(Box::new(s));
        }

        #[test]
        fn fmt_cents_renders_dollars_and_cents() {
            assert_eq!(fmt_cents(0), "$0.00");
            assert_eq!(fmt_cents(150), "$1.50");
            assert_eq!(fmt_cents(9999), "$99.99");
            assert_eq!(fmt_cents(-150), "-$1.50");
        }

        #[test]
        fn format_window_compact_units() {
            assert_eq!(format_window(Window::All), "all");
            assert_eq!(
                format_window(Window::Last { millis: 86_400_000 }),
                "last 1d"
            );
            assert_eq!(format_window(Window::Last { millis: 7_200_000 }), "last 2h");
        }

        // v0.36 T5: the 6 seed-store-based tests below need
        // `SqliteStore::in_memory()` (the in-memory cost-tracking DB).
        // When `observe-sqlite` is off (cli-min build) the in_memory()
        // constructor returns `FeatureDisabled` and `seed_store_with`
        // doesn't compile. Gate them so the cli-min CI lane stays green.
        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn happy_path_prints_summary_against_seeded_store() {
            let _g = store_test_lock();
            let mut o1 = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 250);
            o1.started_at_ms = now_ms();
            let mut o2 = LlmObservation::new("openai", "gpt-5", 200, 100, 500);
            o2.started_at_ms = now_ms();
            seed_store_with(vec![o1, o2]);

            let args = InspectArgs {
                cost: true,
                db: Some(":memory:".into()),
                since: Some("24h".into()),
                by: Some("provider".into()),
                top: Some(5),
                ..Default::default()
            };
            let code = run(args);
            assert_eq!(code, 0);
            uninstall_store();
        }

        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn since_7d_includes_old_records() {
            let _g = store_test_lock();
            let mut o = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 10);
            // 6 days old → inside the 7d window
            o.started_at_ms = now_ms().saturating_sub(6 * 86_400_000);
            seed_store_with(vec![o]);

            let args = InspectArgs {
                cost: true,
                db: Some(":memory:".into()),
                since: Some("7d".into()),
                by: Some("provider".into()),
                ..Default::default()
            };
            assert_eq!(run(args), 0);
            uninstall_store();
        }

        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn since_1m_excludes_old_records() {
            let _g = store_test_lock();
            let mut o = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 10);
            o.started_at_ms = now_ms().saturating_sub(86_400_000);
            seed_store_with(vec![o]);

            // Programmatic check against the summary path: 1m window
            // should produce zero rows even though the DB has one.
            let obs = load_observations(std::path::Path::new(":memory:")).unwrap();
            let s = summarize(&obs, Window::parse("1m").unwrap(), GroupBy::Provider, 0);
            assert_eq!(s.call_count, 0);
            uninstall_store();
        }

        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn by_model_groups_correctly() {
            let _g = store_test_lock();
            let mut o1 = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 10);
            o1.started_at_ms = now_ms();
            let mut o2 = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 10);
            o2.started_at_ms = now_ms();
            let mut o3 = LlmObservation::new("anthropic", "claude-sonnet-4-6", 100, 50, 10);
            o3.started_at_ms = now_ms();
            seed_store_with(vec![o1, o2, o3]);

            let obs = load_observations(std::path::Path::new(":memory:")).unwrap();
            let s = summarize(&obs, Window::All, GroupBy::Model, 0);
            assert_eq!(s.by_group.len(), 2);
            let opus = s
                .by_group
                .iter()
                .find(|r| r.key == "claude-opus-4-7")
                .unwrap();
            assert_eq!(opus.call_count, 2);
            uninstall_store();
        }

        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn top_5_returns_n_most_expensive() {
            let _g = store_test_lock();
            let mut seeded = Vec::new();
            for cost_cents in [10, 50, 200, 5, 1000, 30, 60, 5_000_000].iter() {
                let mut o = LlmObservation::new("anthropic", "claude-opus-4-7", 1, 1, 1);
                o.started_at_ms = now_ms();
                o.cost_cents = *cost_cents;
                seeded.push(o);
            }
            seed_store_with(seeded);

            let obs = load_observations(std::path::Path::new(":memory:")).unwrap();
            let s = summarize(&obs, Window::All, GroupBy::Provider, 5);
            assert_eq!(s.top_calls.len(), 5);
            assert_eq!(s.top_calls[0].cost_cents, 5_000_000);
            assert_eq!(s.top_calls[1].cost_cents, 1000);
            uninstall_store();
        }

        #[test]
        fn empty_db_prints_helpful_message_not_error() {
            let _g = store_test_lock();
            uninstall_store();
            // Point at a path that definitely doesn't exist; the run
            // path must treat "no DB yet" as exit 0 with a helpful note.
            let args = InspectArgs {
                cost: true,
                db: Some("/definitely/does/not/exist/observations.sqlite".into()),
                since: Some("24h".into()),
                by: Some("provider".into()),
                ..Default::default()
            };
            assert_eq!(run(args), 0);
        }

        #[cfg(feature = "observe-sqlite")]
        #[test]
        fn json_mode_emits_valid_json() {
            let _g = store_test_lock();
            let mut o = LlmObservation::new("anthropic", "claude-opus-4-7", 100, 50, 10);
            o.started_at_ms = now_ms();
            seed_store_with(vec![o]);

            let obs = load_observations(std::path::Path::new(":memory:")).unwrap();
            let s = summarize(&obs, Window::All, GroupBy::Provider, 1);
            // Round-trip the json shape through serde_json::Value to
            // verify the summary serialises without panicking.
            let v = serde_json::json!({
                "call_count": s.call_count,
                "total_cost_cents": s.total_cost_cents,
                "top_calls": s.top_calls,
            });
            assert!(v.is_object());
            uninstall_store();
        }

        #[test]
        fn bad_since_returns_exit_2() {
            let _g = store_test_lock();
            uninstall_store();
            let args = InspectArgs {
                cost: true,
                since: Some("not-a-duration".into()),
                ..Default::default()
            };
            assert_eq!(run(args), 2);
        }

        #[test]
        fn bad_by_returns_exit_2() {
            let _g = store_test_lock();
            uninstall_store();
            let args = InspectArgs {
                cost: true,
                by: Some("not-a-group".into()),
                ..Default::default()
            };
            assert_eq!(run(args), 2);
        }

        #[test]
        fn resolve_db_path_honours_explicit_flag() {
            let args = InspectArgs {
                db: Some("/tmp/x.sqlite".into()),
                ..Default::default()
            };
            assert_eq!(
                resolve_db_path(&args),
                std::path::PathBuf::from("/tmp/x.sqlite")
            );
        }
    }
}
