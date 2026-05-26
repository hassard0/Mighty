//! `mty reload <agent-type> --from new.wasm` — request a hot reload
//! against a running Mighty runtime via its control socket.
//!
//! v0.20 Tier 1.5 (see `docs/internals/agent-features-roadmap.md`
//! and `docs/internals/hot-reload.md` for the architecture).
//! v0.21 closes the loop: the runtime now implements the op=reload
//! handler (`crates/mty-runtime/src/control_socket.rs::handle_reload`),
//! so the CLI's wasm bytes round-trip end-to-end. The fall-back
//! `unknown_op` detection is retained for backward compatibility with
//! pre-v0.21 runtimes that may still be running in production.
//!
//! See `docs/reference/cli/mty-reload.md` for user-facing docs.

use std::io::Write;
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

const DEFAULT_ENV: &str = "MTY_RUNTIME_CONTROL_SOCK";
const DEFAULT_DEADLINE_MS: u64 = 5_000;

#[derive(Debug)]
pub struct ReloadArgs {
    pub agent_type: String,
    pub from: PathBuf,
    pub deadline_ms: Option<u64>,
    pub sock: Option<String>,
    pub json: bool,
    /// Skip the actual socket round-trip and just validate inputs.
    /// Useful for smoke tests; mirrors `mty build --dry-run`.
    pub dry_run: bool,
}

/// CLI entry point. Returns a Unix-style process exit code.
pub fn run(args: ReloadArgs) -> i32 {
    // (1) input validation — done before we touch I/O so the user
    // sees actionable errors even when no runtime is reachable.
    if args.agent_type.trim().is_empty() {
        eprintln!("mty reload: agent-type must be a non-empty identifier");
        return 2;
    }
    if !args.from.exists() {
        eprintln!(
            "mty reload: replacement module not found: {}",
            args.from.display()
        );
        return 2;
    }
    let bytes = match std::fs::read(&args.from) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("mty reload: failed to read {}: {e}", args.from.display());
            return 1;
        }
    };
    if bytes.is_empty() {
        eprintln!(
            "mty reload: replacement module is empty: {}",
            args.from.display()
        );
        return 2;
    }
    // Wasm magic + version. Best-effort sanity check — defers the
    // real validation to the runtime's component-loader.
    if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
        eprintln!(
            "mty reload: {} doesn't look like a wasm module (magic mismatch)",
            args.from.display()
        );
        return 2;
    }

    let deadline_ms = args.deadline_ms.unwrap_or(DEFAULT_DEADLINE_MS);

    if args.dry_run {
        if args.json {
            println!(
                "{{\"agent_type\":\"{}\",\"module_size\":{},\"deadline_ms\":{},\"dry_run\":true}}",
                args.agent_type,
                bytes.len(),
                deadline_ms
            );
        } else {
            println!(
                "mty reload (dry-run): agent_type={}, module={} ({} bytes), deadline={}ms",
                args.agent_type,
                args.from.display(),
                bytes.len(),
                deadline_ms
            );
        }
        return 0;
    }

    // (2) resolve the control-socket path.
    let sock_path = match args
        .sock
        .clone()
        .or_else(|| std::env::var(DEFAULT_ENV).ok())
    {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!(
                "mty reload: no control-socket path. Pass --sock <PATH> or set {}.",
                DEFAULT_ENV
            );
            return 2;
        }
    };

    // (3) issue the reload request.
    match send_reload_request(&sock_path, &args.agent_type, &bytes, deadline_ms) {
        Ok(payload) => {
            // v0.21: the server may return either a ReloadReport JSON
            // (success) or a `{"error":"...","code":"MT506x"}` (failure).
            // Distinguish them so the CLI exits non-zero on a server
            // error rather than silently rendering an empty report.
            if payload.contains("\"error\"") {
                match parse_error(&payload) {
                    Some((msg, code)) => {
                        if let Some(c) = code {
                            eprintln!("mty reload: [{c}] {msg}");
                        } else {
                            eprintln!("mty reload: {msg}");
                        }
                    }
                    None => eprintln!("mty reload: server returned: {payload}"),
                }
                return 1;
            }
            if args.json {
                println!("{}", payload);
            } else {
                match pretty_print(&payload) {
                    Ok(s) => print!("{}", s),
                    Err(e) => {
                        eprintln!("mty reload: failed to render reply: {e}");
                        return 1;
                    }
                }
            }
            let _ = std::io::stdout().flush();
            0
        }
        Err(e) => {
            eprintln!("mty reload: {e}");
            1
        }
    }
}

/// Extract `(error, code)` from a server error JSON. Returns `None`
/// when the payload doesn't look like our error shape.
fn parse_error(payload: &str) -> Option<(String, Option<String>)> {
    #[derive(serde::Deserialize)]
    struct ErrRepr {
        error: String,
        #[serde(default)]
        code: Option<String>,
    }
    serde_json::from_str::<ErrRepr>(payload)
        .ok()
        .map(|e| (e.error, e.code))
}

/// Send one reload request to the control socket and return the JSON
/// reply as a string. Cross-platform shim: Unix uses `UnixStream`,
/// Windows currently returns an explanatory error (the control-socket
/// listener doesn't bind on Windows in v0.20 either).
fn send_reload_request(
    sock_path: &str,
    agent_type: &str,
    module_bytes: &[u8],
    deadline_ms: u64,
) -> Result<String, String> {
    #[cfg(unix)]
    {
        unix_query(sock_path, agent_type, module_bytes, deadline_ms)
    }
    #[cfg(not(unix))]
    {
        let _ = (sock_path, agent_type, module_bytes, deadline_ms);
        Err(
            "the Windows named-pipe control socket is not yet implemented \
             (v0.20 Unix-only — same as `mty inspect`). \
             Tracking: dev/history/notes/INTROSPECT_V0_16_NOTES.md."
                .into(),
        )
    }
}

#[cfg(unix)]
fn unix_query(
    sock_path: &str,
    agent_type: &str,
    module_bytes: &[u8],
    deadline_ms: u64,
) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    let mut stream =
        UnixStream::connect(sock_path).map_err(|e| format!("connect {sock_path}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(
            deadline_ms.saturating_add(2_000).max(1_000),
        )))
        .map_err(|e| format!("set_read_timeout: {e}"))?;

    // Encode the module bytes as base64 so the request stays
    // one-line JSON (matches `mty inspect`'s newline-delimited wire
    // contract). We do this with a tiny pure-rust encoder rather than
    // adding a `base64` workspace dep for a single call-site.
    let b64 = base64_encode(module_bytes);
    let req = format!(
        r#"{{"op":"reload","agent_type":{q_type},"module_b64":"{b64}","deadline_ms":{deadline_ms}}}"#,
        q_type = json_str(agent_type),
        b64 = b64,
        deadline_ms = deadline_ms,
    );
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
        return Err("control socket returned an empty reply (is the runtime alive?)".into());
    }
    let trimmed = line.trim_end().to_string();
    // Distinguish the "v0.20 wire-not-implemented" reply from a
    // genuine failure so users know what to do next.
    if trimmed.contains("\"unknown_op\"") {
        return Err(
            "the runtime's control socket doesn't recognise op=reload — \
             this is expected in v0.20 (the CLI surface ships ahead of the \
             runtime handler). Tracking: docs/internals/hot-reload.md, \
             v0.21 wire-up section."
                .into(),
        );
    }
    Ok(trimmed)
}

/// Quote a string for embedding in JSON. Handles the minimum-viable
/// escape set (we only emit identifier-shaped agent types from the
/// CLI, but be safe).
#[cfg_attr(not(unix), allow(dead_code))]
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Tiny self-contained base64 encoder (RFC 4648). We use it once
/// per `mty reload` invocation — no need for a workspace dep.
#[cfg_attr(not(unix), allow(dead_code))]
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b0 = bytes[i];
        let b1 = bytes[i + 1];
        let b2 = bytes[i + 2];
        out.push(ALPHABET[(b0 >> 2) as usize] as char);
        out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(ALPHABET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        out.push(ALPHABET[(b2 & 0x3f) as usize] as char);
        i += 3;
    }
    match bytes.len() - i {
        0 => {}
        1 => {
            let b0 = bytes[i];
            out.push(ALPHABET[(b0 >> 2) as usize] as char);
            out.push(ALPHABET[((b0 & 0x03) << 4) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let b0 = bytes[i];
            let b1 = bytes[i + 1];
            out.push(ALPHABET[(b0 >> 2) as usize] as char);
            out.push(ALPHABET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            out.push(ALPHABET[((b1 & 0x0f) << 2) as usize] as char);
            out.push('=');
        }
        _ => unreachable!(),
    }
    out
}

// ---------------------------------------------------------------------
// Pretty-print of a successful ReloadReport JSON
// ---------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct ReloadReportRepr {
    agent_id: u64,
    agent_type: String,
    old_schema_hash: u64,
    new_schema_hash: u64,
    state_bytes_size: u64,
    drain_elapsed_ms: u64,
    total_elapsed_ms: u64,
}

fn pretty_print(payload: &str) -> Result<String, String> {
    let r: ReloadReportRepr =
        serde_json::from_str(payload).map_err(|e| format!("decode ReloadReport: {e}"))?;
    let mut out = String::new();
    out.push_str("=== mty reload report ===\n");
    out.push_str(&format!(
        "  agent          #{} {}\n",
        r.agent_id, r.agent_type
    ));
    out.push_str(&format!(
        "  schema hash    {:#018x}  (old: {:#018x})\n",
        r.new_schema_hash, r.old_schema_hash
    ));
    out.push_str(&format!("  snapshot size  {} B\n", r.state_bytes_size));
    out.push_str(&format!("  drain elapsed  {} ms\n", r.drain_elapsed_ms));
    out.push_str(&format!("  total elapsed  {} ms\n", r.total_elapsed_ms));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encode_round_trip_rfc_examples() {
        // RFC 4648 §10 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn json_str_escapes_control_chars() {
        assert_eq!(json_str("hi"), "\"hi\"");
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_str("a\nb"), "\"a\\nb\"");
        assert_eq!(json_str("a\x01b"), "\"a\\u0001b\"");
    }

    #[test]
    fn pretty_print_renders_report() {
        let p = r#"{
            "agent_id": 7,
            "agent_type": "ConnAgent",
            "old_schema_hash": 1,
            "new_schema_hash": 1,
            "state_bytes_size": 128,
            "drain_elapsed_ms": 3,
            "total_elapsed_ms": 7
        }"#;
        let out = pretty_print(p).expect("render");
        assert!(out.contains("agent          #7 ConnAgent"));
        assert!(out.contains("snapshot size  128 B"));
    }
}
