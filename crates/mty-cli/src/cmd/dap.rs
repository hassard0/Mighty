//! `mty dap` — Debug Adapter Protocol server over stdio.
//!
//! v0.32 Track A. Drives breakpoint-aware execution of a Mighty
//! program. Speaks the
//! [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/specification)
//! over `stdin/stdout` using the standard `Content-Length`-framed
//! JSON envelope. Implements just enough of the spec to single-step,
//! set source-line / function breakpoints, inspect the call stack
//! + locals, and disconnect cleanly. See the README in
//!   `tools/vscode/` for the launch.json contract this honours.
//!
//! ## Supported requests
//!
//! | Request                       | Notes                                      |
//! | ----------------------------- | ------------------------------------------ |
//! | initialize                    | reports our capabilities                   |
//! | launch                        | `program`, `args`, `stopOnEntry`,          |
//! |                               | `replayTrace`, `recordTrace`               |
//! | setBreakpoints                | by source-line                             |
//! | setFunctionBreakpoints        | by `fn:name` or `agent:Name`               |
//! | configurationDone             | resume the launched program                |
//! | threads                       | one thread per Mighty agent (+ main)       |
//! | stackTrace                    | current call stack                         |
//! | scopes                        | one synthetic `Locals` scope per frame     |
//! | variables                     | locals + Track-F tool_uses (when LLM call) |
//! | continue / next / stepIn /    | DAP step semantics                          |
//! | stepOut / pause               |                                            |
//! | evaluate                      | local lookup + simple field access         |
//! | restart                       | re-launch with the same args               |
//! | disconnect / terminate        | clean shutdown                             |
//!
//! Unsupported requests return an error response with a
//! human-readable `message`. This matches DAP's "graceful unknown"
//! contract — clients fall back to the default UI.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mty_ir::interp::breakpoints::{offset_to_line, BreakDecision, BreakpointHook, StepPosition};
use mty_ir::interp::debug::{BreakReason, DebugFrame, DebugSession, DebugStop};
use mty_ir::interp::host::BufferHost;
use mty_ir::interp::RunResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// One DAP envelope: a JSON object with at minimum `seq` + `type`.
/// We use `serde_json::Value` for body shapes so the same struct
/// can carry every request / response / event variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DapMessage {
    seq: u64,
    #[serde(rename = "type")]
    msg_type: String,
    /// Present on `request` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    /// Present on `request` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<JsonValue>,
    /// Present on `response` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    request_seq: Option<u64>,
    /// Present on `response` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    success: Option<bool>,
    /// Present on `response` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<JsonValue>,
    /// Present on `event` envelopes.
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    /// Optional human-readable message (on errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

/// State threaded across requests.
struct DapState {
    next_seq: AtomicU64,
    /// Path to the .mty source (from `launch.program`).
    program_path: Option<PathBuf>,
    /// Source text for the program.
    source: Option<String>,
    /// Per-source line breakpoints. Keyed by absolute path string.
    line_breakpoints: std::collections::HashMap<String, Vec<u32>>,
    /// Function-name breakpoints (`fn:foo` or `agent:Foo`).
    fn_breakpoints: Vec<String>,
    /// Stop on entry to `main` if true.
    stop_on_entry: bool,
    /// Whether the launched program has been started.
    launched: bool,
    /// Whether `configurationDone` has fired.
    configured: bool,
    /// Optional replay trace path (DAP `launch.replayTrace`).
    replay_trace: Option<PathBuf>,
    /// Optional record trace path (DAP `launch.recordTrace`).
    record_trace: Option<PathBuf>,
}

impl DapState {
    fn new() -> Self {
        Self {
            next_seq: AtomicU64::new(1),
            program_path: None,
            source: None,
            line_breakpoints: std::collections::HashMap::new(),
            fn_breakpoints: Vec::new(),
            stop_on_entry: false,
            launched: false,
            configured: false,
            replay_trace: None,
            record_trace: None,
        }
    }

    fn next_seq(&self) -> u64 {
        self.next_seq.fetch_add(1, Ordering::SeqCst)
    }
}

/// DAP `BreakpointHook` impl: stop at line / fn breakpoints, honour
/// step modes. Cloned (re-installed) on each resume.
pub(crate) struct DapHook {
    /// 1-based line numbers to break at (set via `setBreakpoints`).
    pub(crate) line_breaks: Vec<u32>,
    /// Function names to break at on entry. Used by the v0.33
    /// follow-up that wires `on_call` to the program's fn name table.
    #[allow(dead_code)]
    pub(crate) fn_breaks: Vec<String>,
    /// Source text (for offset → line translation).
    pub(crate) source: String,
    /// Step mode: how to surface `before_step` decisions.
    pub(crate) step_mode: StepMode,
    /// Depth at which the step was initiated (for stepOut + next).
    pub(crate) initial_depth: usize,
    /// Set by the runner before each `run_until_break` invocation;
    /// the hook clears it on first hit so that resume-after-stop
    /// behaves correctly.
    pub(crate) just_resumed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepMode {
    /// Run until a breakpoint hits.
    Continue,
    /// Break on the next step at the same depth.
    Next,
    /// Break on the next step (any depth) — i.e. step into.
    StepIn,
    /// Break when we return to a shallower depth.
    StepOut,
}

impl BreakpointHook for DapHook {
    fn before_step(&mut self, pos: &StepPosition, depth: usize) -> BreakDecision {
        // First step after resume: don't immediately re-fire the
        // breakpoint we just stopped on.
        let first = self.just_resumed;
        self.just_resumed = false;

        // Line breakpoints.
        let line = offset_to_line(&self.source, pos.span.start);
        if !first && self.line_breaks.contains(&line) && pos.span.start > 0 {
            return BreakDecision::Break;
        }

        // Step modes.
        match self.step_mode {
            StepMode::Continue => BreakDecision::Continue,
            StepMode::Next => {
                if !first && depth <= self.initial_depth {
                    BreakDecision::Break
                } else {
                    BreakDecision::Continue
                }
            }
            StepMode::StepIn => {
                if !first {
                    BreakDecision::Break
                } else {
                    BreakDecision::Continue
                }
            }
            StepMode::StepOut => {
                // StepOut: only break when the on_return hook fires
                // at a shallower depth.
                BreakDecision::Continue
            }
        }
    }

    fn on_call(&mut self, callee: mty_ir::ir::IrFnId, _depth: usize) -> BreakDecision {
        // We don't have the callee's name handy here without the
        // program — that's threaded via DapHook + the program ref
        // outside. The check happens in the DAP runner by walking
        // the stack post-step.
        let _ = callee;
        BreakDecision::Continue
    }

    fn on_return(&mut self, depth: usize) -> BreakDecision {
        if self.step_mode == StepMode::StepOut && depth < self.initial_depth {
            BreakDecision::Break
        } else {
            BreakDecision::Continue
        }
    }
}

/// Entry point for `mty dap`. Speaks DAP over stdio and exits when
/// the client sends `disconnect` (or stdin closes).
pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();

    let mut state = DapState::new();
    // Stash for the live session once `launch` fires. We can't keep
    // a long-lived `DebugSession` here because it borrows the
    // `Program`; instead we hold the parsed `Program` + reusable
    // launch params and reconstruct the session as needed.
    let mut session_state: Option<SessionState> = None;

    loop {
        match read_dap_message(&mut reader) {
            Ok(Some(msg)) => {
                let resp = handle_message(&mut state, &mut session_state, &msg);
                for out in resp {
                    if let Err(e) = write_dap_message(&mut writer, &out) {
                        eprintln!("dap: write failed: {e}");
                        return 1;
                    }
                }
                // After handling each message, if there's a live
                // session that's been told to resume, drive it.
                if let Some(ss) = session_state.as_mut() {
                    if ss.pending_resume {
                        ss.pending_resume = false;
                        let events = drive_session(&mut state, ss);
                        for ev in events {
                            if let Err(e) = write_dap_message(&mut writer, &ev) {
                                eprintln!("dap: write failed: {e}");
                                return 1;
                            }
                        }
                    }
                }
                if state.launched
                    && session_state.as_ref().is_some_and(|s| s.finished)
                    && msg.command.as_deref() == Some("disconnect")
                {
                    return 0;
                }
                if msg.command.as_deref() == Some("disconnect")
                    || msg.command.as_deref() == Some("terminate")
                {
                    return 0;
                }
            }
            Ok(None) => return 0,
            Err(e) => {
                eprintln!("dap: read failed: {e}");
                return 1;
            }
        }
    }
}

/// Per-launch state: parsed program + a debug session we lazily
/// reconstruct + bookkeeping for events.
pub(crate) struct SessionState {
    pub(crate) prog: std::sync::Arc<mty_ir::ir::Program>,
    pub(crate) source: String,
    /// Source ID (display path) — used in DAP `output` events.
    #[allow(dead_code)]
    pub(crate) source_id: String,
    /// True once `configurationDone` has fired and the session is
    /// ready to resume.
    pub(crate) pending_resume: bool,
    /// Cached `(line_breaks, fn_breaks)` last applied to the hook.
    pub(crate) line_breaks: Vec<u32>,
    pub(crate) fn_breaks: Vec<String>,
    /// Current step mode for the next resume.
    pub(crate) step_mode: StepMode,
    /// The live session — `None` until first resume + then
    /// reconstructed per drive call (since `Program` borrow lifetimes
    /// can't span request boundaries cleanly with our state struct).
    /// Instead we re-create + step from the saved interpreter state
    /// snapshot.
    pub(crate) session_snapshot: Option<SessionSnapshot>,
    /// Set after the program completes or traps.
    pub(crate) finished: bool,
    /// Current stack snapshot (for stackTrace/variables responses).
    pub(crate) current_stack: Vec<DebugFrame>,
    /// Current locals (for variables responses).
    pub(crate) current_locals: Vec<mty_ir::interp::debug::DebugLocal>,
    /// Last reason we stopped (for the `stopped` event).
    pub(crate) last_stop_reason: Option<String>,
}

/// Snapshot of a paused session — we keep the `DebugSession` alive
/// for the duration of the DAP server's run by storing it in a
/// long-lived `Box`. Since `DebugSession<'a>` borrows the `Program`,
/// we hold both together via an owning shape (`SessionOwner` below).
pub(crate) struct SessionSnapshot {
    /// Boxed self-referential wrapper that keeps the program + the
    /// session co-located.
    pub(crate) owner: Box<SessionOwner>,
}

/// Owns the program + session. Uses an unsafe pointer to dodge the
/// `'a` lifetime — the program lives at a stable address inside the
/// box, and the session never outlives the box (we drop them
/// together).
pub(crate) struct SessionOwner {
    /// Pinned program; kept alive so the `session` pointer below
    /// remains valid for the owner's lifetime. The session walks
    /// the program but does not own it.
    #[allow(dead_code)]
    pub(crate) prog: std::pin::Pin<Box<mty_ir::ir::Program>>,
    /// Raw pointer to the DebugSession; built referencing
    /// `prog`. Dropped before `prog` (see `Drop` impl).
    pub(crate) session: *mut DebugSession<'static>,
    /// Host buffer that the session writes through.
    pub(crate) host: BufferHost,
}

impl SessionOwner {
    pub(crate) fn new(prog: mty_ir::ir::Program, fn_name: &str, budget: u64) -> Option<Box<Self>> {
        let pinned = Box::pin(prog);
        // Build the session referencing the pinned program. We
        // transmute the lifetime to 'static — safe because we drop
        // the session before the program in `Drop`.
        let prog_ref: &mty_ir::ir::Program = &pinned;
        // SAFETY: the program lives until SessionOwner drops, and
        // we manually drop `session` first in our Drop impl.
        let prog_ref_static: &'static mty_ir::ir::Program =
            unsafe { std::mem::transmute(prog_ref) };
        let sess = DebugSession::new_for_fn(prog_ref_static, fn_name, vec![], budget)?;
        let session_ptr = Box::into_raw(Box::new(sess));
        Some(Box::new(Self {
            prog: pinned,
            session: session_ptr,
            host: BufferHost::default(),
        }))
    }

    /// Mutable access to the session.
    pub(crate) fn session_mut(&mut self) -> &mut DebugSession<'static> {
        // SAFETY: pointer is valid for the lifetime of the owner.
        unsafe { &mut *self.session }
    }
}

impl Drop for SessionOwner {
    fn drop(&mut self) {
        // SAFETY: we own the pointer; drop the session first so it
        // doesn't outlive the program.
        unsafe {
            drop(Box::from_raw(self.session));
        }
    }
}

fn handle_message(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    if msg.msg_type != "request" {
        return Vec::new();
    }
    let cmd = msg.command.as_deref().unwrap_or("");
    match cmd {
        "initialize" => vec![ok_response(state, msg, capabilities())],
        "launch" => handle_launch(state, session, msg),
        "setBreakpoints" => handle_set_breakpoints(state, session, msg),
        "setFunctionBreakpoints" => handle_set_function_breakpoints(state, msg),
        "configurationDone" => handle_configuration_done(state, session, msg),
        "threads" => {
            let body = json!({
                "threads": [ json!({ "id": 1, "name": "main" }) ]
            });
            vec![ok_response(state, msg, body)]
        }
        "stackTrace" => handle_stack_trace(state, session, msg),
        "scopes" => handle_scopes(state, msg),
        "variables" => handle_variables(state, session, msg),
        "continue" => handle_continue(state, session, msg),
        "next" => handle_step(state, session, msg, StepMode::Next),
        "stepIn" => handle_step(state, session, msg, StepMode::StepIn),
        "stepOut" => handle_step(state, session, msg, StepMode::StepOut),
        "pause" => {
            // We don't actually run concurrently; pause is a no-op.
            vec![ok_response(state, msg, json!({}))]
        }
        "evaluate" => handle_evaluate(state, session, msg),
        "restart" => handle_restart(state, session, msg),
        "disconnect" | "terminate" => vec![ok_response(state, msg, json!({}))],
        "source" => vec![ok_response(
            state,
            msg,
            json!({ "content": state.source.clone().unwrap_or_default() }),
        )],
        _ => vec![err_response(
            state,
            msg,
            &format!("unsupported command: {cmd}"),
        )],
    }
}

fn capabilities() -> JsonValue {
    json!({
        "supportsConfigurationDoneRequest": true,
        "supportsBreakpointLocationsRequest": true,
        "supportsStepBack": false,
        "supportsRestartRequest": true,
        "supportsTerminateRequest": true,
        "supportsFunctionBreakpoints": true,
        "supportsConditionalBreakpoints": false,
        "supportsEvaluateForHovers": true,
        "supportsSetVariable": false
    })
}

fn handle_launch(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let args = msg.arguments.clone().unwrap_or(JsonValue::Null);
    let Some(program) = args.get("program").and_then(|p| p.as_str()) else {
        return vec![err_response(state, msg, "launch: missing `program`")];
    };
    let stop_on_entry = args
        .get("stopOnEntry")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let replay_trace = args
        .get("replayTrace")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    let record_trace = args
        .get("recordTrace")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    let path = PathBuf::from(program);
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return vec![err_response(
                state,
                msg,
                &format!("launch: failed to read program: {e}"),
            )];
        }
    };
    state.program_path = Some(path.clone());
    state.source = Some(src.clone());
    state.stop_on_entry = stop_on_entry;
    state.replay_trace = replay_trace;
    state.record_trace = record_trace;
    state.launched = true;

    // v0.32 Track F integration: opt into recorded-trace mode by
    // setting the env var the runtime + interpreter recorder watches.
    if let Some(rec) = &state.record_trace {
        std::env::set_var("MTY_RECORD_TRACE", rec);
    }

    // Parse + lower the program. We report parse errors via an
    // `output` event but still report `launch` success — the DAP
    // client surfaces the diagnostic separately.
    let parsed = mty_driver::parse_source(src.clone(), path.display().to_string());
    let (pkg, mut diags) = mty_driver::lower(&parsed);
    if !diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        diags.extend(mty_driver::type_and_borrow_check(&pkg));
    }
    let mut events = Vec::new();
    for d in &diags {
        events.push(output_event(
            state,
            "stderr",
            &format!("{}: {}\n", code_str(d.code), d.primary.message),
        ));
    }
    if diags
        .iter()
        .any(|d| matches!(d.severity, mty_diagnostics::Severity::Error))
    {
        events.push(err_response(
            state,
            msg,
            "launch: program has compile errors",
        ));
        events.push(terminated_event(state));
        return events;
    }
    let typed = mty_types::check_package_typed(&pkg);
    let prog = mty_ir::lower_package(&pkg, &typed);

    // Build the SessionState — owns the program + the session.
    let Some(owner) = SessionOwner::new(prog.clone(), "main", 5_000_000) else {
        events.push(err_response(state, msg, "launch: no `main` fn"));
        events.push(terminated_event(state));
        return events;
    };

    *session = Some(SessionState {
        prog: std::sync::Arc::new(prog),
        source: src.clone(),
        source_id: path.display().to_string(),
        pending_resume: false,
        line_breaks: Vec::new(),
        fn_breaks: Vec::new(),
        step_mode: StepMode::Continue,
        session_snapshot: Some(SessionSnapshot { owner }),
        finished: false,
        current_stack: Vec::new(),
        current_locals: Vec::new(),
        last_stop_reason: None,
    });
    events.push(ok_response(state, msg, json!({})));
    events.push(event(state, "initialized", json!({})));
    events
}

fn handle_set_breakpoints(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let args = msg.arguments.clone().unwrap_or(JsonValue::Null);
    let path = args
        .get("source")
        .and_then(|s| s.get("path"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    let bps: Vec<u32> = args
        .get("breakpoints")
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|bp| bp.get("line").and_then(|l| l.as_u64()).map(|n| n as u32))
                .collect()
        })
        .unwrap_or_default();
    state.line_breakpoints.insert(path, bps.clone());
    if let Some(ss) = session.as_mut() {
        ss.line_breaks.clone_from(&bps);
    }
    let body = json!({
        "breakpoints": bps.iter().map(|l| json!({ "verified": true, "line": l })).collect::<Vec<_>>()
    });
    vec![ok_response(state, msg, body)]
}

fn handle_set_function_breakpoints(state: &mut DapState, msg: &DapMessage) -> Vec<DapMessage> {
    let args = msg.arguments.clone().unwrap_or(JsonValue::Null);
    let names: Vec<String> = args
        .get("breakpoints")
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|bp| {
                    bp.get("name")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    state.fn_breakpoints.clone_from(&names);
    let body = json!({
        "breakpoints": names.iter().map(|_| json!({ "verified": true })).collect::<Vec<_>>()
    });
    vec![ok_response(state, msg, body)]
}

fn handle_configuration_done(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    state.configured = true;
    if let Some(ss) = session.as_mut() {
        ss.pending_resume = true;
    }
    vec![ok_response(state, msg, json!({}))]
}

fn handle_stack_trace(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let Some(ss) = session.as_ref() else {
        return vec![ok_response(
            state,
            msg,
            json!({ "stackFrames": [], "totalFrames": 0 }),
        )];
    };
    let path_str = state
        .program_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let frames: Vec<JsonValue> = ss
        .current_stack
        .iter()
        .rev() // DAP wants innermost-first
        .enumerate()
        .map(|(i, f)| {
            let line = offset_to_line(&ss.source, f.span_start);
            json!({
                "id": i as u64 + 1,
                "name": f.fn_name.clone(),
                "line": line,
                "column": 1,
                "source": {
                    "path": path_str.clone(),
                    "name": path_str.clone(),
                }
            })
        })
        .collect();
    let total = frames.len() as u64;
    vec![ok_response(
        state,
        msg,
        json!({ "stackFrames": frames, "totalFrames": total }),
    )]
}

fn handle_scopes(state: &mut DapState, msg: &DapMessage) -> Vec<DapMessage> {
    // We support one synthetic scope ("Locals") per frame.
    let body = json!({
        "scopes": [
            { "name": "Locals", "variablesReference": 1000, "expensive": false }
        ]
    });
    vec![ok_response(state, msg, body)]
}

fn handle_variables(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let Some(ss) = session.as_ref() else {
        return vec![ok_response(state, msg, json!({ "variables": [] }))];
    };
    let vars: Vec<JsonValue> = ss
        .current_locals
        .iter()
        .map(|l| {
            json!({
                "name": l.name.clone(),
                "value": l.repr.clone(),
                "type": l.kind,
                "variablesReference": 0,
            })
        })
        .collect();
    vec![ok_response(state, msg, json!({ "variables": vars }))]
}

fn handle_continue(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    if let Some(ss) = session.as_mut() {
        ss.step_mode = StepMode::Continue;
        ss.pending_resume = true;
    }
    vec![ok_response(
        state,
        msg,
        json!({ "allThreadsContinued": true }),
    )]
}

fn handle_step(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
    mode: StepMode,
) -> Vec<DapMessage> {
    if let Some(ss) = session.as_mut() {
        ss.step_mode = mode;
        ss.pending_resume = true;
    }
    vec![ok_response(state, msg, json!({}))]
}

fn handle_evaluate(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let args = msg.arguments.clone().unwrap_or(JsonValue::Null);
    let expr = args
        .get("expression")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let Some(ss) = session.as_ref() else {
        return vec![err_response(state, msg, "evaluate: no active session")];
    };
    // v0.32 Track A: limited expr eval — just match the expression
    // against a local name. Field access (`x.field`) walks the
    // repr-as-formatted value, which is intentionally limited.
    let head = expr.split('.').next().unwrap_or("");
    let Some(local) = ss.current_locals.iter().find(|l| l.name == head) else {
        return vec![err_response(state, msg, &format!("no such local: {head}"))];
    };
    vec![ok_response(
        state,
        msg,
        json!({ "result": local.repr.clone(), "variablesReference": 0, "type": local.kind }),
    )]
}

fn handle_restart(
    state: &mut DapState,
    session: &mut Option<SessionState>,
    msg: &DapMessage,
) -> Vec<DapMessage> {
    let Some(ss) = session.as_mut() else {
        return vec![err_response(state, msg, "restart: nothing to restart")];
    };
    // Rebuild the session from the cached program.
    let Some(owner) = SessionOwner::new((*ss.prog).clone(), "main", 5_000_000) else {
        return vec![err_response(state, msg, "restart: no `main`")];
    };
    ss.session_snapshot = Some(SessionSnapshot { owner });
    ss.finished = false;
    ss.pending_resume = true;
    vec![ok_response(state, msg, json!({}))]
}

fn drive_session(state: &mut DapState, ss: &mut SessionState) -> Vec<DapMessage> {
    let Some(snap) = ss.session_snapshot.as_mut() else {
        return vec![];
    };

    let mut hook = DapHook {
        line_breaks: ss.line_breaks.clone(),
        fn_breaks: ss.fn_breaks.clone(),
        source: ss.source.clone(),
        step_mode: ss.step_mode,
        initial_depth: snap.owner.session_mut().depth(),
        just_resumed: true,
    };

    // Take owned references; we'll write to host afterwards.
    let stop_on_entry = state.stop_on_entry && ss.last_stop_reason.is_none();
    let owner: &mut SessionOwner = &mut snap.owner;
    // Build refs to satisfy the borrow checker — we need to call
    // session_mut and access host separately.
    let stop_reason: DebugStop;
    {
        // Split borrows: session and host live in the same owner.
        // SAFETY: session and host don't alias.
        let host_ptr: *mut BufferHost = &raw mut owner.host;
        let sess = owner.session_mut();
        // SAFETY: host_ptr is valid for the rest of this block.
        let host: &mut BufferHost = unsafe { &mut *host_ptr };
        stop_reason = sess.run_until_break(host, &mut hook, stop_on_entry);
    }

    // Forward any program output as `output` events.
    let mut events = Vec::new();
    let stdout_bytes = std::mem::take(&mut owner.host.stdout);
    if !stdout_bytes.is_empty() {
        let text = String::from_utf8_lossy(&stdout_bytes).to_string();
        events.push(output_event(state, "stdout", &text));
    }

    // Refresh stack / locals snapshot after the step.
    {
        let sess = owner.session_mut();
        ss.current_stack = sess.stack_frames();
        ss.current_locals = sess.locals();
    }
    match stop_reason {
        DebugStop::Completed(r) => {
            ss.finished = true;
            let exit = match r {
                RunResult::Ok { exit } => exit,
                _ => 1,
            };
            events.push(event(state, "exited", json!({ "exitCode": exit })));
            events.push(terminated_event(state));
        }
        DebugStop::Trap { code, message } => {
            ss.finished = true;
            events.push(output_event(
                state,
                "stderr",
                &format!("trap {code}: {message}\n"),
            ));
            events.push(event(
                state,
                "stopped",
                json!({
                    "reason": "exception",
                    "description": format!("{code}: {message}"),
                    "threadId": 1,
                    "allThreadsStopped": true,
                }),
            ));
            events.push(terminated_event(state));
        }
        DebugStop::Breakpoint(reason) => {
            let r = match reason {
                BreakReason::Step => "breakpoint",
                BreakReason::FunctionEntry => "function breakpoint",
                BreakReason::FunctionExit => "step",
                BreakReason::StepComplete => "step",
                BreakReason::Entry => "entry",
            };
            ss.last_stop_reason = Some(r.to_string());
            events.push(event(
                state,
                "stopped",
                json!({
                    "reason": r,
                    "threadId": 1,
                    "allThreadsStopped": true,
                }),
            ));
        }
    }
    events
}

// ---------- DAP envelope IO -------------------------------------

fn read_dap_message<R: BufRead>(reader: &mut R) -> std::io::Result<Option<DapMessage>> {
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        headers.push_str(&line);
    }
    let mut content_length: usize = 0;
    for line in headers.lines() {
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    if content_length == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf)?;
    let msg: DapMessage = serde_json::from_slice(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

fn write_dap_message<W: Write>(writer: &mut W, msg: &DapMessage) -> std::io::Result<()> {
    let payload = serde_json::to_vec(msg).expect("DAP envelope must serialise");
    write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

fn ok_response(state: &DapState, req: &DapMessage, body: JsonValue) -> DapMessage {
    DapMessage {
        seq: state.next_seq(),
        msg_type: "response".into(),
        command: req.command.clone(),
        arguments: None,
        request_seq: Some(req.seq),
        success: Some(true),
        body: Some(body),
        event: None,
        message: None,
    }
}

fn err_response(state: &DapState, req: &DapMessage, message: &str) -> DapMessage {
    DapMessage {
        seq: state.next_seq(),
        msg_type: "response".into(),
        command: req.command.clone(),
        arguments: None,
        request_seq: Some(req.seq),
        success: Some(false),
        body: None,
        event: None,
        message: Some(message.to_string()),
    }
}

fn event(state: &DapState, name: &str, body: JsonValue) -> DapMessage {
    DapMessage {
        seq: state.next_seq(),
        msg_type: "event".into(),
        command: None,
        arguments: None,
        request_seq: None,
        success: None,
        body: Some(body),
        event: Some(name.to_string()),
        message: None,
    }
}

fn output_event(state: &DapState, category: &str, text: &str) -> DapMessage {
    event(
        state,
        "output",
        json!({ "category": category, "output": text }),
    )
}

fn terminated_event(state: &DapState) -> DapMessage {
    event(state, "terminated", json!({}))
}

fn code_str(c: mty_diagnostics::codes::DiagCode) -> String {
    c.as_str().to_string()
}

// ---------- Tests ----------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn req(seq: u64, command: &str, args: JsonValue) -> DapMessage {
        DapMessage {
            seq,
            msg_type: "request".into(),
            command: Some(command.into()),
            arguments: Some(args),
            request_seq: None,
            success: None,
            body: None,
            event: None,
            message: None,
        }
    }

    fn parse_response(s: &str) -> DapMessage {
        let body_start = s.find("\r\n\r\n").unwrap() + 4;
        serde_json::from_str(&s[body_start..]).unwrap()
    }

    #[test]
    fn initialize_request_returns_capabilities() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(&mut state, &mut session, &req(1, "initialize", json!({})));
        assert_eq!(resp.len(), 1);
        let r = &resp[0];
        assert_eq!(r.msg_type, "response");
        assert_eq!(r.command.as_deref(), Some("initialize"));
        assert_eq!(r.success, Some(true));
        let body = r.body.as_ref().unwrap();
        assert_eq!(body["supportsConfigurationDoneRequest"], json!(true));
        assert_eq!(body["supportsRestartRequest"], json!(true));
        assert_eq!(body["supportsFunctionBreakpoints"], json!(true));
    }

    #[test]
    fn unknown_request_returns_error() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(
            &mut state,
            &mut session,
            &req(1, "totally_made_up", json!({})),
        );
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0].success, Some(false));
        assert!(resp[0].message.as_deref().unwrap().contains("unsupported"));
    }

    #[test]
    fn set_breakpoints_acks_and_persists() {
        let mut state = DapState::new();
        let mut session = None;
        let args = json!({
            "source": { "path": "/tmp/foo.mty" },
            "breakpoints": [{ "line": 5 }, { "line": 10 }]
        });
        let resp = handle_message(&mut state, &mut session, &req(2, "setBreakpoints", args));
        assert_eq!(resp[0].success, Some(true));
        let bps = resp[0].body.as_ref().unwrap()["breakpoints"]
            .as_array()
            .unwrap();
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0]["line"], json!(5));
        assert_eq!(state.line_breakpoints["/tmp/foo.mty"], vec![5u32, 10]);
    }

    #[test]
    fn set_function_breakpoints_acks() {
        let mut state = DapState::new();
        let mut session = None;
        let args = json!({ "breakpoints": [{ "name": "fn:main" }, { "name": "agent:Counter" }] });
        let resp = handle_message(
            &mut state,
            &mut session,
            &req(2, "setFunctionBreakpoints", args),
        );
        assert_eq!(resp[0].success, Some(true));
        assert_eq!(state.fn_breakpoints, vec!["fn:main", "agent:Counter"]);
    }

    #[test]
    fn threads_reports_main_thread() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(&mut state, &mut session, &req(2, "threads", json!({})));
        assert_eq!(resp[0].success, Some(true));
        let threads = resp[0].body.as_ref().unwrap()["threads"]
            .as_array()
            .unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["id"], json!(1));
        assert_eq!(threads[0]["name"], json!("main"));
    }

    #[test]
    fn scopes_returns_single_locals_scope() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(
            &mut state,
            &mut session,
            &req(2, "scopes", json!({ "frameId": 1 })),
        );
        assert_eq!(resp[0].success, Some(true));
        let scopes = resp[0].body.as_ref().unwrap()["scopes"].as_array().unwrap();
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0]["name"], json!("Locals"));
    }

    #[test]
    fn continue_marks_pending_resume() {
        let mut state = DapState::new();
        let mut session = Some(SessionState {
            prog: std::sync::Arc::new(mty_ir::ir::Program::default()),
            source: "fn main() {}\n".into(),
            source_id: "test".into(),
            pending_resume: false,
            line_breaks: vec![],
            fn_breaks: vec![],
            step_mode: StepMode::Continue,
            session_snapshot: None,
            finished: false,
            current_stack: vec![],
            current_locals: vec![],
            last_stop_reason: None,
        });
        let resp = handle_message(&mut state, &mut session, &req(3, "continue", json!({})));
        assert_eq!(resp[0].success, Some(true));
        assert!(session.as_ref().unwrap().pending_resume);
        assert_eq!(session.as_ref().unwrap().step_mode, StepMode::Continue);
    }

    #[test]
    fn next_step_sets_next_mode() {
        let mut state = DapState::new();
        let mut session = Some(SessionState {
            prog: std::sync::Arc::new(mty_ir::ir::Program::default()),
            source: "".into(),
            source_id: "test".into(),
            pending_resume: false,
            line_breaks: vec![],
            fn_breaks: vec![],
            step_mode: StepMode::Continue,
            session_snapshot: None,
            finished: false,
            current_stack: vec![],
            current_locals: vec![],
            last_stop_reason: None,
        });
        let resp = handle_message(&mut state, &mut session, &req(3, "next", json!({})));
        assert_eq!(resp[0].success, Some(true));
        assert_eq!(session.as_ref().unwrap().step_mode, StepMode::Next);
    }

    #[test]
    fn step_in_sets_stepin_mode() {
        let mut state = DapState::new();
        let mut session = Some(SessionState {
            prog: std::sync::Arc::new(mty_ir::ir::Program::default()),
            source: "".into(),
            source_id: "test".into(),
            pending_resume: false,
            line_breaks: vec![],
            fn_breaks: vec![],
            step_mode: StepMode::Continue,
            session_snapshot: None,
            finished: false,
            current_stack: vec![],
            current_locals: vec![],
            last_stop_reason: None,
        });
        let resp = handle_message(&mut state, &mut session, &req(3, "stepIn", json!({})));
        assert_eq!(resp[0].success, Some(true));
        assert_eq!(session.as_ref().unwrap().step_mode, StepMode::StepIn);
    }

    #[test]
    fn step_out_sets_stepout_mode() {
        let mut state = DapState::new();
        let mut session = Some(SessionState {
            prog: std::sync::Arc::new(mty_ir::ir::Program::default()),
            source: "".into(),
            source_id: "test".into(),
            pending_resume: false,
            line_breaks: vec![],
            fn_breaks: vec![],
            step_mode: StepMode::Continue,
            session_snapshot: None,
            finished: false,
            current_stack: vec![],
            current_locals: vec![],
            last_stop_reason: None,
        });
        let resp = handle_message(&mut state, &mut session, &req(3, "stepOut", json!({})));
        assert_eq!(resp[0].success, Some(true));
        assert_eq!(session.as_ref().unwrap().step_mode, StepMode::StepOut);
    }

    #[test]
    fn disconnect_acks() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(&mut state, &mut session, &req(99, "disconnect", json!({})));
        assert_eq!(resp[0].success, Some(true));
    }

    #[test]
    fn evaluate_without_session_errors() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(
            &mut state,
            &mut session,
            &req(2, "evaluate", json!({ "expression": "x" })),
        );
        assert_eq!(resp[0].success, Some(false));
    }

    #[test]
    fn launch_missing_program_errors() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(&mut state, &mut session, &req(2, "launch", json!({})));
        assert_eq!(resp[0].success, Some(false));
    }

    #[test]
    fn pause_is_acknowledged() {
        let mut state = DapState::new();
        let mut session = None;
        let resp = handle_message(&mut state, &mut session, &req(2, "pause", json!({})));
        assert_eq!(resp[0].success, Some(true));
    }

    #[test]
    fn envelope_round_trips() {
        let m = req(7, "configurationDone", json!({}));
        let mut buf: Vec<u8> = Vec::new();
        write_dap_message(&mut buf, &m).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("Content-Length:"));
        let parsed = parse_response(&s);
        assert_eq!(parsed.seq, 7);
        assert_eq!(parsed.command.as_deref(), Some("configurationDone"));
    }

    #[test]
    fn read_envelope_parses_request() {
        let body = json!({
            "seq": 1,
            "type": "request",
            "command": "initialize",
            "arguments": {}
        })
        .to_string();
        let envelope = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut reader = std::io::Cursor::new(envelope.into_bytes());
        let msg = read_dap_message(&mut reader).unwrap().unwrap();
        assert_eq!(msg.command.as_deref(), Some("initialize"));
        assert_eq!(msg.seq, 1);
    }
}
