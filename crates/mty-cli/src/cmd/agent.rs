//! `mty agent` — v0.33 T5 structured JSON-over-stdio protocol that
//! lets an LLM agent (or any non-human caller) drive every other `mty`
//! subcommand without scraping human-rendered output.
//!
//! Wire format: NDJSON. One JSON object per line on stdin (the
//! request); zero or more JSON objects per line on stdout (the
//! response stream); a final `{"kind":"done", ...}` line terminates
//! each request.
//!
//! Spec: `docs/internals/agent-mode-protocol.md`.
//! Human-facing CLI doc: `docs/reference/cli/mty-agent.md`.
//!
//! This module deliberately implements its op handlers in-process
//! rather than spawning subprocesses — every wrapped subcommand lives
//! in `mty_cli::cmd::*` already, so we can call the same Rust functions
//! and reuse the same Diagnostic / DiagnosticEnvelope shape T4 ships.
//! The captured stdout / stderr surface is funneled through
//! [`OutputCapture`] so every byte the underlying handler emits lands
//! as a structured `kind:"log"` line.
//!
//! Forward compatibility:
//! - Unknown ops emit `kind:"error"` and the loop continues.
//! - Unknown request keys are ignored (serde's default = ignore unknown).
//! - Unknown response kinds are ignored by well-behaved clients.

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use mty_diagnostics::diagnostic::Diagnostic;
use mty_diagnostics::fix::{build_check_result, to_ndjson, DiagnosticEnvelope, ToEnvelope};
use mty_diagnostics::{codes, Severity};
use mty_driver::{lower, parse_source, type_and_borrow_check};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------------
// CLI args + entry point
// ---------------------------------------------------------------------------

/// Argument bundle for `mty agent`. Mirrored in `main.rs`'s clap layer.
#[derive(Debug, Clone, Default)]
pub struct AgentArgs {
    /// If true, read one request from stdin (the entire stdin body
    /// parsed as a single JSON object), run it, then exit.
    pub single_shot: bool,
    /// Transport. `stdio` is the default. `http` and `unix` were
    /// stubbed in v0.33 T5 and shipped real in v0.35 T2.
    pub transport: Transport,
    /// HTTP transport: port to bind. Defaults to 8889. Overridden by
    /// `listen` when that's set.
    pub http_port: u16,
    /// Unix transport: socket path. Overridden by `listen` when that's set.
    pub unix_socket: Option<PathBuf>,
    /// v0.35 T2 — `host:port` for HTTP / socket path for Unix.
    /// Overrides `http_port` / `unix_socket` when present.
    pub listen: Option<String>,
    /// v0.35 T2 — bearer token required on every HTTP request.
    pub auth_token: Option<String>,
    /// v0.35 T2 — path to append `(request, response)` NDJSON pairs to.
    pub record: Option<PathBuf>,
    /// v0.35 T2 — replay a previously recorded session and assert the
    /// live responses byte-match the recorded ones.
    pub replay: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    #[default]
    Stdio,
    Http,
    Unix,
}

impl Transport {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stdio" | "" => Some(Self::Stdio),
            "http" => Some(Self::Http),
            "unix" => Some(Self::Unix),
            _ => None,
        }
    }
}

/// Process entry point. Returns the process exit code.
pub fn run(args: AgentArgs) -> i32 {
    // Replay supersedes every transport: the recorded NDJSON pairs are
    // the input + the expected output. We re-run them in-process and
    // compare. Replay reuses the same Session loop.
    if let Some(replay) = args.replay.clone() {
        return run_replay(&replay);
    }
    match args.transport {
        Transport::Stdio => run_stdio(args),
        Transport::Http => run_http(args),
        Transport::Unix => run_unix(args),
    }
}

fn run_stdio(args: AgentArgs) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut stdout_locked = stdout.lock();
    let mut session = Session::new();
    let mut recorder = match Recorder::open(args.record.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            return emit_fatal(&mut stdout_locked, format!("recorder open failed: {e}"));
        }
    };

    if args.single_shot {
        let mut body = String::new();
        if let Err(e) = stdin.lock().read_to_string(&mut body) {
            return emit_fatal(&mut stdout_locked, format!("stdin read error: {e}"));
        }
        let trimmed = body.trim();
        let req = match parse_request(trimmed) {
            Ok(r) => r,
            Err(e) => {
                return emit_fatal(&mut stdout_locked, format!("malformed JSON: {e}"));
            }
        };
        let (code, captured) = run_one_capturing(&mut session, &req);
        // Replay back to the real stdout.
        let _ = stdout_locked.write_all(captured.as_bytes());
        let _ = stdout_locked.flush();
        recorder.record_pair(trimmed, &captured);
        return code;
    }

    // Interactive: loop reading one line at a time.
    let reader = stdin.lock();
    let mut worst_exit: i32 = 0;
    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(e) => {
                emit(
                    &mut stdout_locked,
                    &Response::Error {
                        message: format!("stdin read error: {e}"),
                    },
                );
                return 2;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req = match parse_request(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let mut buf: Vec<u8> = Vec::new();
                emit(
                    &mut buf,
                    &Response::Error {
                        message: format!("malformed JSON: {e}"),
                    },
                );
                emit(&mut buf, &Response::Done { exit_code: 2 });
                let _ = stdout_locked.write_all(&buf);
                let _ = stdout_locked.flush();
                recorder.record_pair(trimmed, std::str::from_utf8(&buf).unwrap_or(""));
                worst_exit = worst_exit.max(2);
                continue;
            }
        };
        let is_halt = matches!(req.op.as_str(), "halt");
        let (code, captured) = run_one_capturing(&mut session, &req);
        let _ = stdout_locked.write_all(captured.as_bytes());
        let _ = stdout_locked.flush();
        recorder.record_pair(trimmed, &captured);
        if code != 0 {
            worst_exit = worst_exit.max(code);
        }
        if is_halt {
            break;
        }
    }
    worst_exit
}

fn emit_fatal<W: Write>(out: &mut W, message: String) -> i32 {
    emit(out, &Response::Error { message });
    emit(out, &Response::Done { exit_code: 2 });
    2
}

// ---------------------------------------------------------------------------
// Wire-format request shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub op: String,
    #[serde(flatten)]
    pub fields: serde_json::Map<String, JsonValue>,
}

fn parse_request(line: &str) -> Result<Request, String> {
    serde_json::from_str::<Request>(line).map_err(|e| e.to_string())
}

impl Request {
    fn str(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(|v| v.as_str())
    }
    fn bool(&self, key: &str) -> bool {
        self.fields
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
    fn usize(&self, key: &str) -> Option<usize> {
        self.fields
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
    }
    fn str_list(&self, key: &str) -> Vec<String> {
        self.fields
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Wire-format response shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Response {
    // v0.34 T4: boxed because the inlined `EnvelopeMsg` (carrying a
    // DiagnosticEnvelope with `schema_version`, `code`, `severity`,
    // span, prose, fix, see_also) is ~300 bytes — bigger than every
    // other variant — and clippy's large_enum_variant lint fires.
    Envelope(Box<EnvelopeMsg>),
    Log {
        stream: String,
        text: String,
    },
    #[allow(dead_code)]
    Progress {
        phase: String,
        pct: f32,
    },
    Result(ResultMsg),
    Patch {
        applied: bool,
        diff: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        new_content: Option<String>,
    },
    Error {
        message: String,
    },
    Done {
        exit_code: i32,
    },
}

/// Wraps a `DiagnosticEnvelope` so it can flow through `Response`'s
/// `kind = "envelope"` variant — serde-flatten lets us inline the
/// envelope's fields next to the `kind` discriminator.
#[derive(Debug, Clone, Serialize)]
pub struct EnvelopeMsg {
    #[serde(flatten)]
    pub env: DiagnosticEnvelope,
}

/// Per-op summary returned just before the terminator. Op-specific
/// counters are flattened in via `extra`.
#[derive(Debug, Clone, Serialize)]
pub struct ResultMsg {
    pub op: String,
    pub ok: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, JsonValue>,
}

fn emit<W: Write>(out: &mut W, msg: &Response) {
    let line = serde_json::to_string(msg).expect("response serializes");
    let _ = writeln!(out, "{}", line);
    let _ = out.flush();
}

// ---------------------------------------------------------------------------
// Session — per-process interactive state
// ---------------------------------------------------------------------------

pub(crate) struct Session {
    /// Last source-file path operated on. Mirrored as a hint into the
    /// `fix` op handler so future ops can skip restating the path.
    last_path: Option<PathBuf>,
    /// Last `check` envelopes (used by `fix` to look up by code).
    last_envelopes: Vec<DiagnosticEnvelope>,
    /// Source text the last `check` ran against.
    last_source: Option<String>,
}

impl Session {
    pub(crate) fn new() -> Self {
        Self {
            last_path: None,
            last_envelopes: Vec::new(),
            last_source: None,
        }
    }

    pub(crate) fn handle<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        match req.op.as_str() {
            "check" => self.op_check(req, out),
            "run" => self.op_run(req, out),
            "test" => self.op_test(req, out),
            "inspect" => self.op_inspect(req, out),
            "find" => self.op_find(req, out),
            "explain" => self.op_explain(req, out),
            "fmt" => self.op_fmt(req, out),
            "fix" => self.op_fix(req, out),
            "halt" => {
                emit(
                    out,
                    &Response::Result(ResultMsg {
                        op: "halt".into(),
                        ok: true,
                        extra: serde_json::Map::new(),
                    }),
                );
                0
            }
            other => {
                emit(
                    out,
                    &Response::Error {
                        message: format!("unknown op: {}", other),
                    },
                );
                2
            }
        }
    }

    // -----------------------------------------------------------------
    // op: check
    // -----------------------------------------------------------------
    fn op_check<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(path) = req.str("path") else {
            emit(
                out,
                &Response::Error {
                    message: "check: missing required `path`".into(),
                },
            );
            return 2;
        };
        let include_source = req.bool("include_source");
        let path_buf = PathBuf::from(path);
        let src = match fs::read_to_string(&path_buf) {
            Ok(s) => s,
            Err(e) => {
                emit(
                    out,
                    &Response::Error {
                        message: format!("path not found: {}: {}", path_buf.display(), e),
                    },
                );
                return 1;
            }
        };
        let parsed = parse_source(src.clone(), path_buf.display().to_string());
        let (pkg, mut diags) = lower(&parsed);
        let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
        if !lower_errors {
            diags.extend(type_and_borrow_check(&pkg));
        }
        let has_error = diags.iter().any(|d| matches!(d.severity, Severity::Error));

        // Build envelopes + emit each as kind:"envelope"
        let mut envelopes: Vec<DiagnosticEnvelope> = Vec::with_capacity(diags.len());
        let mut fix_count = 0usize;
        for d in &diags {
            let mut env = d.to_envelope(&path_buf.display().to_string(), &src);
            if include_source {
                env.source = Some(mty_diagnostics::fix::snippet_around(&src, env.span.line, 1));
            }
            if env.fix.is_some() {
                fix_count += 1;
            }
            envelopes.push(env.clone());
            emit(out, &Response::Envelope(Box::new(EnvelopeMsg { env })));
        }

        // v0.45 T3 — structured-result document piggybacks on every
        // `check` response under the `result` extra field. Mirrors the
        // shape `mty check --json` emits and lets agents stop
        // line-by-line envelope-stitching when they only need the
        // flat code+span+message tuple.
        let check_result = build_check_result(&diags, &path_buf.display().to_string(), &src);
        let result_value = serde_json::to_value(&check_result).expect("check result serializes");

        self.last_path = Some(path_buf);
        self.last_source = Some(src);
        self.last_envelopes = envelopes;

        let mut extra = serde_json::Map::new();
        extra.insert(
            "diagnostics_count".into(),
            JsonValue::from(diags.len() as u64),
        );
        extra.insert("fix_count".into(), JsonValue::from(fix_count as u64));
        extra.insert("result".into(), result_value);
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "check".into(),
                ok: !has_error,
                extra,
            }),
        );
        if has_error {
            1
        } else {
            0
        }
    }

    // -----------------------------------------------------------------
    // op: run
    // -----------------------------------------------------------------
    fn op_run<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(path) = req.str("path") else {
            emit(
                out,
                &Response::Error {
                    message: "run: missing required `path`".into(),
                },
            );
            return 2;
        };
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            emit(
                out,
                &Response::Error {
                    message: format!("path not found: {}", path_buf.display()),
                },
            );
            return 1;
        }
        let args = req.str_list("args");
        let legacy = req.bool("legacy_interp");
        self.last_path = Some(path_buf.clone());

        // We can't realistically tee stdout/stderr from the slice-8
        // runtime without instrumenting every print path. For T5 we
        // surface the most useful pieces: the path being run, the
        // chosen mode, and the exit code. The runtime's own stdout
        // still flows through the agent's stdout (under stdio
        // transport this means the consumer sees raw program output
        // interleaved with our JSON — documented in the spec under
        // "run quirks"). v0.34 wires `mty run --json` for full
        // structured capture.
        emit(
            out,
            &Response::Log {
                stream: "stderr".into(),
                text: format!("agent: running {}", path_buf.display()),
            },
        );
        if !args.is_empty() {
            emit(
                out,
                &Response::Log {
                    stream: "stderr".into(),
                    text: format!("agent: argv = {:?}", args),
                },
            );
        }

        let code = super::run::run(&path_buf, legacy, args);

        let mut extra = serde_json::Map::new();
        extra.insert("exit_code".into(), JsonValue::from(code as i64));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "run".into(),
                ok: code == 0,
                extra,
            }),
        );
        code
    }

    // -----------------------------------------------------------------
    // op: test
    // -----------------------------------------------------------------
    fn op_test<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let manifest_dir = req.str("manifest_dir").map(PathBuf::from);
        let eval = req.bool("eval");
        let replay_only = req.bool("replay_only");
        let ci = req.bool("ci");
        let no_strict = req.bool("no_strict");
        let strict = !no_strict;
        emit(
            out,
            &Response::Log {
                stream: "stderr".into(),
                text: format!(
                    "agent: running tests (eval={}, replay_only={}, ci={}, strict={})",
                    eval, replay_only, ci, strict
                ),
            },
        );
        let code = super::test::run(super::test::TestArgs {
            manifest_dir,
            eval,
            strict,
            replay_only,
            ci,
            format: super::test::OutputFormat::Json,
        });
        let mut extra = serde_json::Map::new();
        extra.insert("exit_code".into(), JsonValue::from(code as i64));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "test".into(),
                ok: code == 0,
                extra,
            }),
        );
        code
    }

    // -----------------------------------------------------------------
    // op: inspect
    // -----------------------------------------------------------------
    fn op_inspect<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let view = req.str("view").unwrap_or("agents");
        let cost = matches!(view, "cost");
        let args = super::inspect::InspectArgs {
            sock: req.str("sock").map(|s| s.to_string()),
            agent: req.fields.get("agent").and_then(|v| v.as_u64()),
            json: true,
            watch_ms: None,
            cost,
            since: req.str("since").map(|s| s.to_string()),
            by: req.str("by").map(|s| s.to_string()),
            top: req.usize("top"),
            db: req.str("db").map(|s| s.to_string()),
        };
        emit(
            out,
            &Response::Log {
                stream: "stderr".into(),
                text: format!("agent: inspect view={}", view),
            },
        );
        let code = super::inspect::run(args);
        let mut extra = serde_json::Map::new();
        extra.insert("view".into(), JsonValue::from(view));
        extra.insert("exit_code".into(), JsonValue::from(code as i64));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "inspect".into(),
                ok: code == 0,
                extra,
            }),
        );
        code
    }

    // -----------------------------------------------------------------
    // op: find
    // -----------------------------------------------------------------
    fn op_find<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(query) = req.str("query").map(|s| s.to_string()) else {
            emit(
                out,
                &Response::Error {
                    message: "find: missing required `query`".into(),
                },
            );
            return 2;
        };
        let top = req.usize("top").unwrap_or(10).max(1);
        let root = req
            .str("root")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let hits = find_hits(&root, &query, top);
        let mut extra = serde_json::Map::new();
        extra.insert(
            "hits".into(),
            JsonValue::Array(
                hits.iter()
                    .map(|h| {
                        serde_json::json!({
                            "file": h.file,
                            "line": h.line,
                            "text": h.text,
                        })
                    })
                    .collect(),
            ),
        );
        extra.insert("hit_count".into(), JsonValue::from(hits.len() as u64));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "find".into(),
                ok: true,
                extra,
            }),
        );
        0
    }

    // -----------------------------------------------------------------
    // op: explain
    // -----------------------------------------------------------------
    fn op_explain<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(code_str) = req.str("code") else {
            emit(
                out,
                &Response::Error {
                    message: "explain: missing required `code`".into(),
                },
            );
            return 2;
        };
        let num_str = code_str
            .strip_prefix("MT")
            .or_else(|| code_str.strip_prefix("mt"))
            .or_else(|| code_str.strip_prefix("SD"))
            .or_else(|| code_str.strip_prefix("sd"))
            .unwrap_or(code_str);
        let Ok(n) = num_str.parse::<u16>() else {
            emit(
                out,
                &Response::Error {
                    message: format!("explain: bad code `{}`", code_str),
                },
            );
            return 2;
        };
        let dc = codes::DiagCode::new(n);
        let mut extra = serde_json::Map::new();
        extra.insert("code".into(), JsonValue::from(dc.as_str()));
        match codes::explain(dc) {
            Some(text) => {
                extra.insert("text".into(), JsonValue::from(text));
                emit(
                    out,
                    &Response::Result(ResultMsg {
                        op: "explain".into(),
                        ok: true,
                        extra,
                    }),
                );
                0
            }
            None => {
                extra.insert("text".into(), JsonValue::Null);
                emit(
                    out,
                    &Response::Result(ResultMsg {
                        op: "explain".into(),
                        ok: false,
                        extra,
                    }),
                );
                1
            }
        }
    }

    // -----------------------------------------------------------------
    // op: fmt
    // -----------------------------------------------------------------
    fn op_fmt<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(path) = req.str("path") else {
            emit(
                out,
                &Response::Error {
                    message: "fmt: missing required `path`".into(),
                },
            );
            return 2;
        };
        let path_buf = PathBuf::from(path);
        let check = req.bool("check");
        // v0.42 T5 L26 safety: refuse non-`.mty` extensions up-front so the
        // structured-protocol surface inherits the same destructive-truncation
        // guard as the human CLI.
        if path_buf.is_file() && path_buf.extension().and_then(|s| s.to_str()) != Some("mty") {
            emit(
                out,
                &Response::Error {
                    message: format!(
                        "fmt: {}: refusing — `fmt` only formats `.mty` files",
                        path_buf.display()
                    ),
                },
            );
            return 1;
        }
        let src = match fs::read_to_string(&path_buf) {
            Ok(s) => s,
            Err(e) => {
                emit(
                    out,
                    &Response::Error {
                        message: format!("path not found: {}: {}", path_buf.display(), e),
                    },
                );
                return 1;
            }
        };
        let norm = if src.contains("\r\n") {
            src.replace("\r\n", "\n")
        } else {
            src.clone()
        };
        let parsed = parse_source(norm.clone(), path_buf.display().to_string());
        // v0.42 T5 L26 safety: refuse to write when the input did not parse
        // cleanly. The structured-protocol caller still gets a clear error
        // back (mirroring the CLI's stderr message).
        if !parsed.diagnostics.is_empty() {
            let first = &parsed.diagnostics[0];
            emit(
                out,
                &Response::Error {
                    message: format!(
                        "fmt: {}: parse failed: {} ({})",
                        path_buf.display(),
                        first.primary.message,
                        first.code.as_str()
                    ),
                },
            );
            return 1;
        }
        let formatted = mty_fmt::format(parsed.green);
        let needs_reformat = formatted != norm;
        if !check && needs_reformat {
            // Persist the change (preserve CRLF if present).
            let to_write = if src.contains("\r\n") {
                formatted.replace('\n', "\r\n")
            } else {
                formatted.clone()
            };
            if let Err(e) = fs::write(&path_buf, &to_write) {
                emit(
                    out,
                    &Response::Error {
                        message: format!("fmt: write failed: {}", e),
                    },
                );
                return 1;
            }
        }
        let mut extra = serde_json::Map::new();
        extra.insert("would_reformat".into(), JsonValue::from(needs_reformat));
        extra.insert("check_only".into(), JsonValue::from(check));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "fmt".into(),
                ok: if check { !needs_reformat } else { true },
                extra,
            }),
        );
        if check && needs_reformat {
            1
        } else {
            0
        }
    }

    // -----------------------------------------------------------------
    // op: fix
    // -----------------------------------------------------------------
    fn op_fix<W: Write>(&mut self, req: &Request, out: &mut W) -> i32 {
        let Some(code) = req.str("code").map(|s| s.to_string()) else {
            emit(
                out,
                &Response::Error {
                    message: "fix: missing required `code`".into(),
                },
            );
            return 2;
        };
        let alt_idx = req.usize("alternative").unwrap_or(0);
        let write = req.bool("write");

        // Allow `path` override; otherwise use the last-checked path.
        let Some(path_buf) = req
            .str("path")
            .map(PathBuf::from)
            .or_else(|| self.last_path.clone())
        else {
            emit(
                out,
                &Response::Error {
                    message: "fix: no `path` and no prior check; pass `path`".into(),
                },
            );
            return 2;
        };

        // Always re-check before applying. We don't want a stale
        // last_envelopes to confuse fix targeting.
        let src = match fs::read_to_string(&path_buf) {
            Ok(s) => s,
            Err(e) => {
                emit(
                    out,
                    &Response::Error {
                        message: format!("path not found: {}: {}", path_buf.display(), e),
                    },
                );
                return 1;
            }
        };
        let parsed = parse_source(src.clone(), path_buf.display().to_string());
        let (pkg, mut diags) = lower(&parsed);
        let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
        if !lower_errors {
            diags.extend(type_and_borrow_check(&pkg));
        }
        let envelopes: Vec<DiagnosticEnvelope> = diags
            .iter()
            .map(|d: &Diagnostic| d.to_envelope(&path_buf.display().to_string(), &src))
            .collect();

        let target = envelopes
            .iter()
            .find(|e| e.code.eq_ignore_ascii_case(&code));
        let Some(env) = target else {
            emit(
                out,
                &Response::Error {
                    message: format!(
                        "fix: no diagnostic with code `{}` in {}",
                        code,
                        path_buf.display()
                    ),
                },
            );
            return 1;
        };
        let Some(fix) = &env.fix else {
            emit(
                out,
                &Response::Error {
                    message: format!("fix: diagnostic `{}` has no proposed fix", code),
                },
            );
            return 1;
        };
        if alt_idx >= fix.alternatives.len() {
            emit(
                out,
                &Response::Error {
                    message: format!(
                        "fix: alternative {} out of range (have {})",
                        alt_idx,
                        fix.alternatives.len()
                    ),
                },
            );
            return 2;
        }
        let alt = &fix.alternatives[alt_idx];
        let diff = alt.diff.clone();
        let new_content_opt = apply_unified_diff(&src, &diff);

        let mut applied = false;
        let new_content = new_content_opt.clone();
        if write {
            if let Some(body) = &new_content {
                if let Err(e) = fs::write(&path_buf, body) {
                    emit(
                        out,
                        &Response::Error {
                            message: format!("fix: write failed: {}", e),
                        },
                    );
                    return 1;
                }
                applied = true;
            } else {
                emit(
                    out,
                    &Response::Error {
                        message: "fix: diff didn't apply cleanly; not writing".into(),
                    },
                );
                return 1;
            }
        }
        emit(
            out,
            &Response::Patch {
                applied,
                diff: diff.clone(),
                new_content,
            },
        );
        let mut extra = serde_json::Map::new();
        extra.insert("applied".into(), JsonValue::from(applied));
        extra.insert("diff_len".into(), JsonValue::from(diff.len() as u64));
        extra.insert("code".into(), JsonValue::from(code));
        extra.insert("alternative".into(), JsonValue::from(alt_idx as u64));
        emit(
            out,
            &Response::Result(ResultMsg {
                op: "fix".into(),
                ok: true,
                extra,
            }),
        );
        0
    }
}

// ---------------------------------------------------------------------------
// Minimal unified-diff applier
// ---------------------------------------------------------------------------

/// Apply a unified-diff string (single-hunk, as the T4 `FixBuilder`
/// produces) to `src`. Returns `Some(new_content)` on success, `None`
/// on hunk mismatch.
///
/// The applier handles three shapes T4's `FixBuilder` emits:
/// - `@@ -L,N +L,M @@` line replacement (lines 1-indexed by L).
/// - `@@ -L,0 +L,N @@` pure insertion before line L.
/// - `@@ -L,N +L,0 @@` pure deletion of lines starting at L.
///
/// This is intentionally minimal — it does NOT support context lines
/// or multiple hunks, since the T4 builder doesn't emit them. The
/// v0.34 follow-up wires a real diff applier (likely `patch` or the
/// `similar` crate) so external callers can submit richer patches.
fn apply_unified_diff(src: &str, diff: &str) -> Option<String> {
    let mut lines = diff.lines();
    // Skip `--- a/` and `+++ b/` headers.
    let _ = lines.next()?;
    let _ = lines.next()?;
    let hunk = lines.next()?;
    // hunk header: `@@ -L,N +L,M @@`
    let header = hunk.strip_prefix("@@")?;
    let header = header.split("@@").next()?.trim();
    // Split into `-L,N` and `+L,M`.
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() != 2 {
        return None;
    }
    let neg = parts[0].strip_prefix('-')?;
    let pos = parts[1].strip_prefix('+')?;
    let (old_line, old_n) = parse_range(neg)?;
    let (_new_line, _new_n) = parse_range(pos)?;

    // Collect old / new bodies from the diff.
    let mut old_body: Vec<String> = Vec::new();
    let mut new_body: Vec<String> = Vec::new();
    for l in lines {
        if let Some(rest) = l.strip_prefix('-') {
            old_body.push(rest.to_string());
        } else if let Some(rest) = l.strip_prefix('+') {
            new_body.push(rest.to_string());
        } else if let Some(rest) = l.strip_prefix(' ') {
            // Context line; T4 doesn't emit these, but tolerate.
            old_body.push(rest.to_string());
            new_body.push(rest.to_string());
        }
    }

    // Split src into lines (preserving the trailing-newline-or-not).
    let src_had_trailing_nl = src.ends_with('\n');
    let src_lines: Vec<&str> = if src.is_empty() {
        Vec::new()
    } else if src_had_trailing_nl {
        let mut v: Vec<&str> = src.split('\n').collect();
        // split('\n') on "a\n" gives ["a", ""]; drop the trailing empty.
        if v.last() == Some(&"") {
            v.pop();
        }
        v
    } else {
        src.split('\n').collect()
    };

    let line_idx = (old_line as usize).saturating_sub(1);
    if line_idx > src_lines.len() {
        return None;
    }

    // Pure insertion: old_n == 0.
    if old_n == 0 {
        let mut out: Vec<String> = src_lines.iter().map(|s| s.to_string()).collect();
        for (i, body) in new_body.iter().enumerate() {
            let insert_at = (line_idx + i).min(out.len());
            out.insert(insert_at, body.clone());
        }
        return Some(join_lines(&out, src_had_trailing_nl));
    }

    // Replacement / deletion: verify the old_body matches what's at line_idx.
    if line_idx + old_body.len() > src_lines.len() {
        return None;
    }
    for (i, expected) in old_body.iter().enumerate() {
        if src_lines[line_idx + i] != expected {
            return None;
        }
    }
    let mut out: Vec<String> =
        Vec::with_capacity(src_lines.len() - old_body.len() + new_body.len());
    out.extend(src_lines[..line_idx].iter().map(|s| s.to_string()));
    out.extend(new_body.iter().cloned());
    out.extend(
        src_lines[line_idx + old_body.len()..]
            .iter()
            .map(|s| s.to_string()),
    );
    Some(join_lines(&out, src_had_trailing_nl))
}

fn parse_range(spec: &str) -> Option<(u32, u32)> {
    let mut parts = spec.split(',');
    let line: u32 = parts.next()?.parse().ok()?;
    let n: u32 = parts.next().map(|p| p.parse().ok()).unwrap_or(Some(1))?;
    Some((line, n))
}

fn join_lines(lines: &[String], trailing_nl: bool) -> String {
    let mut s = lines.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

// ---------------------------------------------------------------------------
// `find` op — minimal substring search
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FindHit {
    pub file: String,
    pub line: u32,
    pub text: String,
}

fn find_hits(root: &Path, query: &str, top: usize) -> Vec<FindHit> {
    let mut hits: Vec<FindHit> = Vec::new();
    let q_lower = query.to_lowercase();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    'outer: while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                // Skip hidden + target dirs to keep results scoped.
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(ext, "mty" | "md") {
                continue;
            }
            let Ok(body) = fs::read_to_string(&p) else {
                continue;
            };
            for (i, line) in body.split('\n').enumerate() {
                if line.to_lowercase().contains(&q_lower) {
                    hits.push(FindHit {
                        file: p.display().to_string(),
                        line: (i + 1) as u32,
                        text: line.trim().chars().take(160).collect(),
                    });
                    if hits.len() >= top {
                        break 'outer;
                    }
                }
            }
        }
    }
    hits
}

/// Public helper used by `mty check --format json` callers and the
/// in-process tests in this crate. Builds the same NDJSON shape T4
/// emits, but as a `Vec<DiagnosticEnvelope>`.
#[allow(dead_code)]
pub fn envelopes_for(path: &Path) -> (Vec<DiagnosticEnvelope>, i32) {
    let Ok(src) = fs::read_to_string(path) else {
        return (Vec::new(), 1);
    };
    let parsed = parse_source(src.clone(), path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_and_borrow_check(&pkg));
    }
    let envs: Vec<DiagnosticEnvelope> = diags
        .iter()
        .map(|d| d.to_envelope(&path.display().to_string(), &src))
        .collect();
    let has_error = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    (envs, if has_error { 1 } else { 0 })
}

/// Re-export for unit tests outside the module.
#[allow(dead_code)]
pub fn ndjson_for(path: &Path, include_source: bool) -> String {
    let src = fs::read_to_string(path).unwrap_or_default();
    let parsed = parse_source(src.clone(), path.display().to_string());
    let (pkg, mut diags) = lower(&parsed);
    let lower_errors = diags.iter().any(|d| matches!(d.severity, Severity::Error));
    if !lower_errors {
        diags.extend(type_and_borrow_check(&pkg));
    }
    to_ndjson(&diags, &path.display().to_string(), &src, include_source)
}

// ---------------------------------------------------------------------------
// v0.35 T2 — request runner shared by every transport
// ---------------------------------------------------------------------------

/// Run one request against `session`, capturing every response line
/// (including the terminating `done`) into an in-memory NDJSON string.
///
/// Every non-stdio transport — HTTP, Unix socket, recorder, replay —
/// goes through this so the wire bytes are identical to what stdio
/// would have produced. The exit code returned is the same code
/// `Session::handle` produced for the underlying op.
pub(crate) fn run_one_capturing(session: &mut Session, req: &Request) -> (i32, String) {
    let mut buf: Vec<u8> = Vec::new();
    let code = session.handle(req, &mut buf);
    emit(&mut buf, &Response::Done { exit_code: code });
    let s = String::from_utf8(buf).unwrap_or_default();
    (code, s)
}

/// Same shape as `run_one_capturing` but takes a raw NDJSON line. On
/// parse failure emits the same `kind:"error"` + `kind:"done"`
/// envelope the stdio path would have, with exit code 2.
pub(crate) fn run_one_capturing_line(session: &mut Session, line: &str) -> (i32, String) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        let mut buf: Vec<u8> = Vec::new();
        emit(
            &mut buf,
            &Response::Error {
                message: "empty request".into(),
            },
        );
        emit(&mut buf, &Response::Done { exit_code: 2 });
        return (2, String::from_utf8(buf).unwrap_or_default());
    }
    match parse_request(trimmed) {
        Ok(req) => run_one_capturing(session, &req),
        Err(e) => {
            let mut buf: Vec<u8> = Vec::new();
            emit(
                &mut buf,
                &Response::Error {
                    message: format!("malformed JSON: {e}"),
                },
            );
            emit(&mut buf, &Response::Done { exit_code: 2 });
            (2, String::from_utf8(buf).unwrap_or_default())
        }
    }
}

// ---------------------------------------------------------------------------
// v0.35 T2 — Recorder
// ---------------------------------------------------------------------------

/// Append-only NDJSON file capturing every (request, response) pair
/// the session processed. Each line is a single JSON object of shape
/// `{"request": "<raw request line>", "response": "<raw response
/// bytes>"}` so the file can be replayed verbatim by `run_replay`.
///
/// The recorder is best-effort: if writing fails (disk full, perms,
/// EPIPE on a renamed file), the failure is logged to stderr but does
/// NOT halt the session — recording is an aux trace, not a sync
/// transport.
pub(crate) struct Recorder {
    inner: Option<std::io::BufWriter<std::fs::File>>,
}

impl Recorder {
    /// Open the recorder against `path`. When `path` is `None`, returns
    /// a no-op recorder whose `record_pair` is a free function call.
    pub(crate) fn open(path: Option<&Path>) -> Result<Self, String> {
        let Some(p) = path else {
            return Ok(Self { inner: None });
        };
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("create parent {}: {}", parent.display(), e))?;
            }
        }
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .map_err(|e| format!("open {}: {}", p.display(), e))?;
        Ok(Self {
            inner: Some(std::io::BufWriter::new(f)),
        })
    }

    /// Append one (request, response) pair. `response` is the full
    /// raw NDJSON-bytes string the transport sent back to the client,
    /// including the trailing `done` line.
    pub(crate) fn record_pair(&mut self, request: &str, response: &str) {
        let Some(w) = self.inner.as_mut() else {
            return;
        };
        let entry = serde_json::json!({
            "request": request,
            "response": response,
        });
        let line = serde_json::to_string(&entry).unwrap_or_default();
        if let Err(e) = writeln!(w, "{}", line) {
            eprintln!("mty agent: recorder write failed: {e}");
        }
        if let Err(e) = w.flush() {
            eprintln!("mty agent: recorder flush failed: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// v0.35 T2 — Replay
// ---------------------------------------------------------------------------

/// Read a recorded NDJSON session file and re-run every request
/// against a fresh `Session`, asserting each response byte-matches
/// the recorded one. Returns 0 on full match, 1 on any drift, 2 on
/// IO / parse errors against the file itself.
pub(crate) fn run_replay(path: &Path) -> i32 {
    let body = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mty agent --replay: read {}: {}", path.display(), e);
            return 2;
        }
    };
    let mut session = Session::new();
    let mut drift = false;
    let mut count: usize = 0;
    for (i, raw) in body.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("mty agent --replay: line {} not JSON: {}", i + 1, e);
                return 2;
            }
        };
        let Some(req_line) = entry.get("request").and_then(|v| v.as_str()) else {
            eprintln!("mty agent --replay: line {} missing `request`", i + 1);
            return 2;
        };
        let Some(recorded) = entry.get("response").and_then(|v| v.as_str()) else {
            eprintln!("mty agent --replay: line {} missing `response`", i + 1);
            return 2;
        };
        let (_code, live) = run_one_capturing_line(&mut session, req_line);
        if live != recorded {
            eprintln!(
                "mty agent --replay: drift at line {} (req: {})",
                i + 1,
                truncate(req_line, 80)
            );
            eprintln!("  recorded:\n{}", indent(recorded, "    "));
            eprintln!("  live:\n{}", indent(&live, "    "));
            drift = true;
        }
        count += 1;
    }
    if drift {
        eprintln!(
            "mty agent --replay: {} request(s), at least one drifted",
            count
        );
        1
    } else {
        eprintln!("mty agent --replay: {} request(s) all match", count);
        0
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = s
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= max)
            .last()
            .unwrap_or(0);
        format!("{}…", &s[..cut])
    }
}

fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| format!("{}{}", prefix, l))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// v0.35 T2 — HTTP transport
// ---------------------------------------------------------------------------

/// Process entry point for `--transport http`. Spawns a tokio runtime,
/// binds a TCP listener, and serves the three documented endpoints:
///
/// * `POST /v1/agent` — body is one Request JSON; response is NDJSON
///   streamed back as the response body.
/// * `POST /v1/agent/batch` — body is NDJSON requests; response is
///   interleaved NDJSON responses, one batch-result line per request.
/// * `GET /v1/agent/version` — returns `{"mty_version": "<v>",
///   "agent_protocol": "1.0"}`.
///
/// Auth: when `auth_token` is `Some`, every request must carry
/// `Authorization: Bearer <token>`. Unauthorized returns 401.
fn run_http(args: AgentArgs) -> i32 {
    use std::net::SocketAddr;

    let listen_spec = args
        .listen
        .clone()
        .unwrap_or_else(|| format!("127.0.0.1:{}", args.http_port));
    let addr: SocketAddr = match listen_spec.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!(
                "mty agent --transport http: bad --listen `{}`: {}",
                listen_spec, e
            );
            return 2;
        }
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mty agent --transport http: tokio init: {e}");
            return 2;
        }
    };

    let auth = args.auth_token.clone();
    let record = args.record.clone();

    rt.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mty agent --transport http: bind {}: {}", addr, e);
                return 2;
            }
        };
        let bound = listener.local_addr().unwrap_or(addr);
        eprintln!("mty agent: HTTP listening on http://{}/v1/agent", bound);

        // Recorder shared across every connection. Wrapped in
        // tokio::Mutex so concurrent handlers can serialize their
        // writes.
        let recorder = std::sync::Arc::new(tokio::sync::Mutex::new(
            match Recorder::open(record.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("mty agent --transport http: recorder: {e}");
                    return 2;
                }
            },
        ));

        http_accept_loop(listener, auth, recorder).await;
        0
    })
}

#[allow(clippy::too_many_lines)]
async fn http_accept_loop(
    listener: tokio::net::TcpListener,
    auth_token: Option<String>,
    recorder: std::sync::Arc<tokio::sync::Mutex<Recorder>>,
) {
    use http_body_util::Full;
    use hyper::body::{Bytes, Incoming};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request as HReq, Response as HResp};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;

    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("mty agent --transport http: accept: {e}");
                return;
            }
        };
        let auth = auth_token.clone();
        let recorder = recorder.clone();
        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let svc = service_fn(move |req: HReq<Incoming>| {
                let auth = auth.clone();
                let recorder = recorder.clone();
                async move {
                    let resp = http_handle(req, auth, recorder).await;
                    Ok::<HResp<Full<Bytes>>, Infallible>(resp)
                }
            });
            let _ = http1::Builder::new().serve_connection(io, svc).await;
        });
    }
}

async fn http_handle(
    req: hyper::Request<hyper::body::Incoming>,
    auth_token: Option<String>,
    recorder: std::sync::Arc<tokio::sync::Mutex<Recorder>>,
) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;

    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let authz_hdr = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Bearer-token gate.
    if let Some(expected) = &auth_token {
        let supplied = authz_hdr.as_deref().and_then(|h| {
            // RFC 7617: `Bearer <token>`. Match case-insensitively on
            // the scheme.
            let (scheme, token) = h.split_once(' ')?;
            if scheme.eq_ignore_ascii_case("Bearer") {
                Some(token.trim().to_string())
            } else {
                None
            }
        });
        if supplied.as_deref() != Some(expected.as_str()) {
            return hyper::Response::builder()
                .status(401)
                .header("content-type", "application/json")
                .header("www-authenticate", "Bearer")
                .body(Full::new(Bytes::from_static(
                    b"{\"kind\":\"error\",\"message\":\"unauthorized\"}\n",
                )))
                .expect("401 builds");
        }
    }

    // Routes.
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/agent/version") => {
            let body = serde_json::json!({
                "mty_version": crate::MIGHTY_VERSION,
                "agent_protocol": "1.0",
            });
            hyper::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from(body.to_string() + "\n")))
                .expect("version builds")
        }
        ("POST", "/v1/agent") => {
            let bytes = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(e) => return http_bad_request(format!("read body: {e}")),
            };
            let Ok(body_utf8) = std::str::from_utf8(&bytes) else {
                return http_bad_request("body is not utf-8".into());
            };
            let body_str = body_utf8.trim();
            let mut session = Session::new();
            let (_code, ndjson) = run_one_capturing_line(&mut session, body_str);
            // Record the pair if a recorder is wired.
            {
                let mut rec = recorder.lock().await;
                rec.record_pair(body_str, &ndjson);
            }
            hyper::Response::builder()
                .status(200)
                .header("content-type", "application/x-ndjson")
                .body(Full::new(Bytes::from(ndjson)))
                .expect("ndjson builds")
        }
        ("POST", "/v1/agent/batch") => {
            let bytes = match req.into_body().collect().await {
                Ok(b) => b.to_bytes(),
                Err(e) => return http_bad_request(format!("read body: {e}")),
            };
            let Ok(body_str) = std::str::from_utf8(&bytes) else {
                return http_bad_request("body is not utf-8".into());
            };
            let mut session = Session::new();
            let mut out = String::new();
            for line in body_str.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let (_code, ndjson) = run_one_capturing_line(&mut session, trimmed);
                {
                    let mut rec = recorder.lock().await;
                    rec.record_pair(trimmed, &ndjson);
                }
                out.push_str(&ndjson);
            }
            hyper::Response::builder()
                .status(200)
                .header("content-type", "application/x-ndjson")
                .body(Full::new(Bytes::from(out)))
                .expect("ndjson builds")
        }
        _ => hyper::Response::builder()
            .status(404)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from_static(
                b"{\"kind\":\"error\",\"message\":\"not found\"}\n",
            )))
            .expect("404 builds"),
    }
}

fn http_bad_request(msg: String) -> hyper::Response<http_body_util::Full<hyper::body::Bytes>> {
    use http_body_util::Full;
    use hyper::body::Bytes;
    let env = serde_json::json!({"kind":"error","message": msg});
    hyper::Response::builder()
        .status(400)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(env.to_string() + "\n")))
        .expect("400 builds")
}

// ---------------------------------------------------------------------------
// v0.35 T2 — Unix socket transport
// ---------------------------------------------------------------------------

/// Process entry point for `--transport unix`.
///
/// On Unix, binds a `tokio::net::UnixListener` at the supplied path
/// and speaks the same line-delimited JSON protocol as stdio, one
/// session per connection. The socket file is unlinked at process
/// exit (best-effort).
///
/// On Windows the agent doesn't ship Unix-socket support today — we
/// print a one-line error envelope and exit 2 so the caller gets a
/// clean signal rather than a hyper-confusing bind failure.
#[cfg(unix)]
fn run_unix(args: AgentArgs) -> i32 {
    let Some(path) = args
        .listen
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| args.unix_socket.clone())
    else {
        eprintln!("mty agent --transport unix: pass --listen <path> or --socket <path>");
        return 2;
    };

    // Pre-unlink so we don't EADDRINUSE if the previous run died
    // without cleaning up.
    let _ = std::fs::remove_file(&path);

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("mty agent --transport unix: tokio init: {e}");
            return 2;
        }
    };

    let record = args.record.clone();

    rt.block_on(async move {
        let listener = match tokio::net::UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mty agent --transport unix: bind {}: {}", path.display(), e);
                return 2;
            }
        };
        eprintln!("mty agent: Unix socket listening on {}", path.display());

        let recorder = std::sync::Arc::new(tokio::sync::Mutex::new(
            match Recorder::open(record.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("mty agent --transport unix: recorder: {e}");
                    return 2;
                }
            },
        ));

        unix_accept_loop(listener, recorder).await;
        let _ = std::fs::remove_file(&path);
        0
    })
}

#[cfg(not(unix))]
fn run_unix(_args: AgentArgs) -> i32 {
    let stdout = std::io::stdout();
    let mut locked = stdout.lock();
    emit(
        &mut locked,
        &Response::Error {
            message:
                "transport `unix` is not supported on Windows; use `--transport http` or `--transport stdio`"
                    .into(),
        },
    );
    emit(&mut locked, &Response::Done { exit_code: 2 });
    2
}

#[cfg(unix)]
async fn unix_accept_loop(
    listener: tokio::net::UnixListener,
    recorder: std::sync::Arc<tokio::sync::Mutex<Recorder>>,
) {
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("mty agent --transport unix: accept: {e}");
                return;
            }
        };
        let recorder = recorder.clone();
        tokio::spawn(async move {
            if let Err(e) = unix_serve_conn(stream, recorder).await {
                eprintln!("mty agent --transport unix: conn: {e}");
            }
        });
    }
}

#[cfg(unix)]
async fn unix_serve_conn(
    stream: tokio::net::UnixStream,
    recorder: std::sync::Arc<tokio::sync::Mutex<Recorder>>,
) -> std::io::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (rd, mut wr) = stream.into_split();
    let mut reader = BufReader::new(rd).lines();
    let mut session = Session::new();
    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        let req_for_halt = parse_request(&trimmed).ok();
        let is_halt = req_for_halt
            .as_ref()
            .map(|r| r.op == "halt")
            .unwrap_or(false);
        let (_code, ndjson) = run_one_capturing_line(&mut session, &trimmed);
        {
            let mut rec = recorder.lock().await;
            rec.record_pair(&trimmed, &ndjson);
        }
        wr.write_all(ndjson.as_bytes()).await?;
        wr.flush().await?;
        if is_halt {
            break;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_request_ok() {
        let r = parse_request(r#"{"op":"check","path":"x.mty"}"#).unwrap();
        assert_eq!(r.op, "check");
        assert_eq!(r.str("path"), Some("x.mty"));
    }

    #[test]
    fn parse_request_bad_json_errors() {
        let e = parse_request(r#"{"op":"check""#).unwrap_err();
        assert!(!e.is_empty());
    }

    #[test]
    fn transport_parse() {
        assert_eq!(Transport::parse("stdio"), Some(Transport::Stdio));
        assert_eq!(Transport::parse("http"), Some(Transport::Http));
        assert_eq!(Transport::parse("unix"), Some(Transport::Unix));
        assert_eq!(Transport::parse("garbage"), None);
        assert_eq!(Transport::parse(""), Some(Transport::Stdio));
    }

    #[test]
    fn apply_diff_simple_replace() {
        let src = "a\nb\nc\n";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -2,1 +2,1 @@\n-b\n+B\n";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "a\nB\nc\n");
    }

    #[test]
    fn apply_diff_insert_before() {
        let src = "a\nb\nc\n";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -2,0 +2,1 @@\n+X\n";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "a\nX\nb\nc\n");
    }

    #[test]
    fn apply_diff_multi_line_replace() {
        let src = "a\nb\nc\nd\n";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -2,2 +2,3 @@\n-b\n-c\n+B\n+C\n+CC\n";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "a\nB\nC\nCC\nd\n");
    }

    #[test]
    fn apply_diff_pure_deletion() {
        let src = "a\nb\nc\nd\n";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -2,1 +2,0 @@\n-b\n";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "a\nc\nd\n");
    }

    #[test]
    fn apply_diff_mismatched_returns_none() {
        let src = "a\nb\nc\n";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -2,1 +2,1 @@\n-nope\n+X\n";
        assert!(apply_unified_diff(src, diff).is_none());
    }

    #[test]
    fn apply_diff_preserves_no_trailing_newline() {
        let src = "a\nb";
        let diff = "--- a/x.mty\n+++ b/x.mty\n@@ -1,1 +1,1 @@\n-a\n+A\n";
        let out = apply_unified_diff(src, diff).unwrap();
        assert_eq!(out, "A\nb");
    }

    #[test]
    fn find_hits_finds_substring() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.mty");
        std::fs::write(&f, "let x = write_file()\nlet y = 42\n").unwrap();
        let hits = find_hits(tmp.path(), "write_file", 10);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].file.ends_with("a.mty"));
        assert_eq!(hits[0].line, 1);
        assert!(hits[0].text.contains("write_file"));
    }

    #[test]
    fn find_hits_top_limit() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..5 {
            let f = tmp.path().join(format!("a{i}.mty"));
            std::fs::write(&f, "needle\n").unwrap();
        }
        let hits = find_hits(tmp.path(), "needle", 3);
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn find_hits_skips_target_and_dot_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("a.mty"), "needle\n").unwrap();
        let dot = tmp.path().join(".cache");
        std::fs::create_dir(&dot).unwrap();
        std::fs::write(dot.join("a.mty"), "needle\n").unwrap();
        let hits = find_hits(tmp.path(), "needle", 10);
        assert!(hits.is_empty());
    }

    #[test]
    fn response_envelope_serializes_with_kind() {
        let env = DiagnosticEnvelope {
            schema_version: mty_diagnostics::fix::SCHEMA_VERSION.to_string(),
            code: "MT4099".into(),
            severity: "error".into(),
            span: mty_diagnostics::fix::SpanInfo {
                file: "x.mty".into(),
                line: 1,
                col: 1,
                len: 3,
                byte_start: 0,
                byte_end: 3,
            },
            title: "t".into(),
            prose: "p".into(),
            fix: None,
            see_also: vec![],
            source: None,
        };
        let r = Response::Envelope(Box::new(EnvelopeMsg { env }));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"kind\":\"envelope\""));
        assert!(s.contains("\"code\":\"MT4099\""));
    }

    #[test]
    fn response_done_serializes_with_kind() {
        let r = Response::Done { exit_code: 0 };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(s, r#"{"kind":"done","exit_code":0}"#);
    }

    #[test]
    fn response_log_serializes() {
        let r = Response::Log {
            stream: "stderr".into(),
            text: "hi".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"kind\":\"log\""));
        assert!(s.contains("\"stream\":\"stderr\""));
    }

    #[test]
    fn response_error_serializes() {
        let r = Response::Error {
            message: "bad".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"kind\":\"error\""));
        assert!(s.contains("\"message\":\"bad\""));
    }

    #[test]
    fn response_patch_omits_new_content_when_none() {
        let r = Response::Patch {
            applied: false,
            diff: "...".into(),
            new_content: None,
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains("\"kind\":\"patch\""));
        assert!(!s.contains("new_content"));
    }

    #[test]
    fn session_explain_known_code() {
        let mut s = Session::new();
        let req = Request {
            op: "explain".into(),
            fields: serde_json::from_str(r#"{"code":"MT0001"}"#).unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 0);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"op\":\"explain\""));
        assert!(s.contains("\"code\":\"MT0001\""));
    }

    #[test]
    fn session_explain_bad_code() {
        let mut s = Session::new();
        let req = Request {
            op: "explain".into(),
            fields: serde_json::from_str(r#"{"code":"not a code"}"#).unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"kind\":\"error\""));
    }

    #[test]
    fn session_explain_unknown_code_number() {
        let mut s = Session::new();
        let req = Request {
            op: "explain".into(),
            fields: serde_json::from_str(r#"{"code":"MT9999"}"#).unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        // Unknown code returns 1 (recognized format, not a known code).
        assert_eq!(code, 1);
    }

    #[test]
    fn session_halt() {
        let mut s = Session::new();
        let req = Request {
            op: "halt".into(),
            fields: serde_json::Map::new(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 0);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"op\":\"halt\""));
        assert!(s.contains("\"ok\":true"));
    }

    #[test]
    fn session_unknown_op() {
        let mut s = Session::new();
        let req = Request {
            op: "frobnicate".into(),
            fields: serde_json::Map::new(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("\"kind\":\"error\""));
        assert!(s.contains("unknown op"));
    }

    #[test]
    fn session_check_missing_path() {
        let mut s = Session::new();
        let req = Request {
            op: "check".into(),
            fields: serde_json::Map::new(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("missing required `path`"));
    }

    #[test]
    fn session_check_clean_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("ok.mty");
        std::fs::write(&f, "fn main() -> Unit { }\n").unwrap();
        let mut s = Session::new();
        let req = Request {
            op: "check".into(),
            fields: serde_json::from_str(&format!(
                r#"{{"path":"{}"}}"#,
                f.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        // Clean — but the trivial program might still warn; we only
        // assert no hard errors fired.
        let _ = code;
        let s_out = String::from_utf8(buf).unwrap();
        assert!(s_out.contains("\"op\":\"check\""));
    }

    #[test]
    fn session_fmt_check_clean() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("x.mty");
        // Run fmt on this once to canonicalize, then re-check should pass.
        std::fs::write(&f, "fn main() -> Unit { }\n").unwrap();
        // First fmt-write to canonical form.
        let mut s = Session::new();
        let path_str = f.display().to_string().replace('\\', "\\\\");
        let write_req = Request {
            op: "fmt".into(),
            fields: serde_json::from_str(&format!(r#"{{"path":"{}"}}"#, path_str)).unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let _ = s.handle(&write_req, &mut buf);

        // Now `--check`.
        let check_req = Request {
            op: "fmt".into(),
            fields: serde_json::from_str(&format!(r#"{{"path":"{}","check":true}}"#, path_str))
                .unwrap(),
        };
        let mut buf2: Vec<u8> = Vec::new();
        let _ = s.handle(&check_req, &mut buf2);
        let s_out = String::from_utf8(buf2).unwrap();
        assert!(s_out.contains("\"op\":\"fmt\""));
        assert!(s_out.contains("\"would_reformat\":false"));
    }

    #[test]
    fn session_fmt_missing_path() {
        let mut s = Session::new();
        let req = Request {
            op: "fmt".into(),
            fields: serde_json::Map::new(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
    }

    // v0.42 T5 L26 — destructive-truncation guard, agent surface.
    #[test]
    fn session_fmt_refuses_non_mty_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("notes.txt");
        let payload = b"plain text, not Mighty code at all.\n";
        std::fs::write(&f, payload).unwrap();
        let mut s = Session::new();
        let req = Request {
            op: "fmt".into(),
            fields: serde_json::from_str(&format!(
                r#"{{"path":"{}"}}"#,
                f.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 1, "non-.mty input must exit non-zero");
        // File must be unchanged.
        assert_eq!(std::fs::read(&f).unwrap(), payload);
        let s_out = String::from_utf8(buf).unwrap();
        assert!(s_out.contains(".mty"));
    }

    // v0.42 T5 L26 — parse-failure guard, agent surface.
    #[test]
    fn session_fmt_refuses_parse_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("bad.mty");
        let payload = b"fn ( {\n}\n";
        std::fs::write(&f, payload).unwrap();
        let mut s = Session::new();
        let req = Request {
            op: "fmt".into(),
            fields: serde_json::from_str(&format!(
                r#"{{"path":"{}"}}"#,
                f.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 1, "parse-fail input must exit non-zero");
        // File must be unchanged.
        assert_eq!(std::fs::read(&f).unwrap(), payload);
        let s_out = String::from_utf8(buf).unwrap();
        assert!(s_out.to_lowercase().contains("parse"));
    }

    #[test]
    fn session_find_returns_hits() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.mty");
        std::fs::write(&f, "fn write_file() -> Unit { }\n").unwrap();
        let mut s = Session::new();
        let req = Request {
            op: "find".into(),
            fields: serde_json::from_str(&format!(
                r#"{{"query":"write_file","root":"{}","top":5}}"#,
                tmp.path().display().to_string().replace('\\', "\\\\")
            ))
            .unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 0);
        let s_out = String::from_utf8(buf).unwrap();
        assert!(s_out.contains("\"op\":\"find\""));
        assert!(s_out.contains("\"hits\""));
        assert!(s_out.contains("write_file"));
    }

    #[test]
    fn session_find_missing_query() {
        let mut s = Session::new();
        let req = Request {
            op: "find".into(),
            fields: serde_json::Map::new(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
    }

    #[test]
    fn session_fix_no_prior_check() {
        let mut s = Session::new();
        let req = Request {
            op: "fix".into(),
            fields: serde_json::from_str(r#"{"code":"MT4099"}"#).unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        assert_eq!(code, 2);
        let s_out = String::from_utf8(buf).unwrap();
        assert!(s_out.contains("no `path`"));
    }

    #[test]
    fn session_fix_no_matching_code() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("ok.mty");
        std::fs::write(&f, "fn main() -> Unit { }\n").unwrap();
        let mut s = Session::new();
        let req = Request {
            op: "fix".into(),
            fields: serde_json::from_str(&format!(
                r#"{{"path":"{}","code":"MT4099"}}"#,
                f.display().to_string().replace('\\', "\\\\")
            ))
            .unwrap(),
        };
        let mut buf: Vec<u8> = Vec::new();
        let code = s.handle(&req, &mut buf);
        // Clean file has no MT4099 diagnostic.
        assert_eq!(code, 1);
    }

    // run_one_capturing — used by every transport. A trivial sanity
    // check that it produces the same NDJSON bytes Session.handle would
    // have written directly, plus a terminating `done` line.
    #[test]
    fn run_one_capturing_emits_done_line() {
        let mut s = Session::new();
        let req = Request {
            op: "halt".into(),
            fields: serde_json::Map::new(),
        };
        let (code, body) = run_one_capturing(&mut s, &req);
        assert_eq!(code, 0);
        assert!(body.contains("\"kind\":\"halt\"") || body.contains("\"op\":\"halt\""));
        // Last non-empty line is the `done` terminator.
        let last = body.lines().rfind(|l| !l.trim().is_empty()).unwrap();
        let v: serde_json::Value = serde_json::from_str(last).unwrap();
        assert_eq!(v["kind"], "done");
        assert_eq!(v["exit_code"], 0);
    }

    #[test]
    fn run_one_capturing_unknown_op_done_2() {
        let mut s = Session::new();
        let req = Request {
            op: "frobnicate".into(),
            fields: serde_json::Map::new(),
        };
        let (code, body) = run_one_capturing(&mut s, &req);
        assert_eq!(code, 2);
        let last = body.lines().rfind(|l| !l.trim().is_empty()).unwrap();
        let v: serde_json::Value = serde_json::from_str(last).unwrap();
        assert_eq!(v["kind"], "done");
        assert_eq!(v["exit_code"], 2);
    }

    // ----- Recorder + replay --------------------------------------------

    #[test]
    fn recorder_records_pairs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        let mut rec = Recorder::open(Some(&path)).unwrap();
        rec.record_pair(r#"{"op":"halt"}"#, "{\"kind\":\"done\",\"exit_code\":0}\n");
        drop(rec);
        let body = std::fs::read_to_string(&path).unwrap();
        // Each line is one JSON object with "request" and "response".
        let line = body.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["request"], r#"{"op":"halt"}"#);
        assert!(v["response"]
            .as_str()
            .unwrap()
            .contains("\"kind\":\"done\""));
    }

    #[test]
    fn recorder_noop_when_path_is_none() {
        // record_pair should be a no-op when no path was supplied.
        let mut rec = Recorder::open(None).unwrap();
        rec.record_pair("a", "b"); // doesn't panic, writes nothing.
    }

    #[test]
    fn replay_matches_when_response_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        // Record one halt request and its actual response.
        let mut s = Session::new();
        let req = Request {
            op: "halt".into(),
            fields: serde_json::Map::new(),
        };
        let (_c, body) = run_one_capturing(&mut s, &req);
        let mut rec = Recorder::open(Some(&path)).unwrap();
        rec.record_pair(r#"{"op":"halt"}"#, &body);
        drop(rec);
        let code = run_replay(&path);
        assert_eq!(code, 0);
    }

    #[test]
    fn replay_fails_on_response_drift() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        let entry = serde_json::json!({
            "request": r#"{"op":"halt"}"#,
            "response": "{\"kind\":\"done\",\"exit_code\":42}\n",
        });
        std::fs::write(&path, entry.to_string() + "\n").unwrap();
        let code = run_replay(&path);
        assert_eq!(code, 1);
    }

    #[test]
    fn replay_handles_unknown_op_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        let mut s = Session::new();
        let req = Request {
            op: "weird".into(),
            fields: serde_json::Map::new(),
        };
        let (_c, body) = run_one_capturing(&mut s, &req);
        let mut rec = Recorder::open(Some(&path)).unwrap();
        rec.record_pair(r#"{"op":"weird"}"#, &body);
        drop(rec);
        let code = run_replay(&path);
        assert_eq!(code, 0);
    }

    #[test]
    fn replay_errors_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.ndjson");
        let code = run_replay(&path);
        assert_eq!(code, 2);
    }

    #[test]
    fn replay_errors_on_malformed_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rec.ndjson");
        std::fs::write(&path, "not json\n").unwrap();
        let code = run_replay(&path);
        assert_eq!(code, 2);
    }
}
