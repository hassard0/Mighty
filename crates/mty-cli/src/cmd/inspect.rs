//! `mty inspect` — connect to a running runtime's control socket and
//! print a snapshot of every live agent. v0.16 Tier 1.1 (see
//! `docs/internals/agent-features-roadmap.md`).
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
}

/// CLI entry point. Returns a Unix-style process exit code.
pub fn run(args: InspectArgs) -> i32 {
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
