//! Local control socket for runtime introspection (v0.16, Tier 1.1).
//!
//! Off by default. Enable by setting `MTY_RUNTIME_CONTROL_SOCK` to a
//! filesystem path. On Unix the path is a Unix-domain socket; on
//! Windows the path is currently a TODO — `mty inspect` warns and
//! returns gracefully. See
//! [INTROSPECT_V0_16_NOTES.md](../../../dev/history/notes/INTROSPECT_V0_16_NOTES.md).
//!
//! ## Wire protocol
//!
//! Newline-delimited JSON. Each request is one JSON object per line.
//! Each response is one JSON object per line. The server keeps the
//! connection alive so `mty inspect --watch` can poll without
//! reconnecting.
//!
//! Supported ops (see [`Request`]):
//!
//! - `{"op": "snapshot"}` → [`RuntimeSnapshot`] JSON
//! - `{"op": "snapshot_agent", "id": 42}` → [`AgentSnapshot`] JSON
//!   (or `{"error": "not_found"}` if no live agent has that id)
//! - `{"op": "list"}` → `{"agents": [{"agent_id": .., "agent_type": ..}, ...]}`
//! - `{"op": "reload", "agent_type": "Echo", "module_b64": "..."}` →
//!   [`crate::reload::ReloadReport`] JSON (v0.21). The base64-encoded
//!   payload is the replacement wasm module bytes; the runtime
//!   validates the magic, parses the embedded `__mty_agent_type` +
//!   `__mty_schema_hash` custom sections, drains the agent's
//!   in-flight handler, snapshots state, swaps the per-agent program
//!   slot, then resumes. On failure the runtime returns
//!   `{"error":"...","code":"MT506x"}`.
//!
//! Unknown ops return `{"error":"unknown_op"}`. Bad JSON returns
//! `{"error":"bad_json"}` and the connection stays open.

use crate::agent::AgentRegistry;
use crate::introspect::{
    snapshot_agent, snapshot_runtime, AgentListEntry, AgentSnapshot, IntrospectMap, RuntimeSnapshot,
};
use crate::reload::{
    load_agent_module, ModuleSource, Program, ReloadError, ReloadGate, ReloadOptions, ReloadReport,
    ReloadRunner, SchemaRegistry, SwapPlan,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle as TokioHandle;

/// Name of the env var the runtime consults at startup.
pub const CONTROL_SOCK_ENV: &str = "MTY_RUNTIME_CONTROL_SOCK";

/// Inbound op.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Snapshot,
    SnapshotAgent {
        id: u64,
    },
    List,
    /// v0.21 hot reload. The runtime decodes `module_b64` into wasm
    /// bytes, runs the swap pipeline, and returns a `ReloadReport`.
    Reload {
        agent_type: String,
        module_b64: String,
        #[serde(default)]
        deadline_ms: Option<u64>,
    },
}

/// Outbound op. Each response shape gets its own variant so JSON
/// consumers can switch on the shape without looking at a tag.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Response {
    Snapshot(RuntimeSnapshot),
    Agent(AgentSnapshot),
    List {
        agents: Vec<AgentListEntry>,
    },
    Reload(ReloadReport),
    /// `error` is the human-readable message; `code` is the
    /// `MT506x` diagnostic id (v0.21 hot-reload errors carry both
    /// fields; older error replies omit `code`).
    Error {
        error: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

/// Hook the control-socket calls when an op=reload request arrives.
/// The runtime registers an implementation that knows how to drive
/// the typed `ReloadRunner` against the agent's `Resumable` state
/// cell — the socket layer can't know the agent's concrete type, so
/// the registration is dynamic.
pub trait ReloadHook: Send + Sync {
    /// Perform the reload of `agent_type` using `wasm_bytes`. The
    /// implementation is responsible for locating the live agent,
    /// running the swap pipeline, and producing a [`ReloadReport`].
    fn reload(
        &self,
        agent_type: &str,
        wasm_bytes: Vec<u8>,
        deadline: Duration,
    ) -> Result<ReloadReport, ReloadError>;
}

/// Default in-process reload hook. The runtime constructs one per
/// agent type at registration time; the hook owns the typed state
/// cell + the `ReloadGate` + the schema registry handle so the
/// control socket only needs the `Arc<dyn ReloadHook>` reference.
pub struct SimpleReloadHook<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    pub agent_type: String,
    pub agent_id: crate::agent::AgentId,
    pub desc: Arc<crate::agent::AgentDescriptor>,
    pub state: Arc<Mutex<T>>,
    pub gate: Arc<ReloadGate>,
    pub drain_signal: Option<crate::reload::DrainSignal>,
    pub schema_registry: Option<Arc<SchemaRegistry>>,
    pub program: Option<Arc<Mutex<Program>>>,
    /// The agent's `Resumable::SCHEMA_HASH`. Used to fill the swap
    /// plan's `old_schema_hash` without making the hook generic on
    /// the trait constant.
    pub current_schema_hash: u64,
}

impl<T> ReloadHook for SimpleReloadHook<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    fn reload(
        &self,
        agent_type: &str,
        wasm_bytes: Vec<u8>,
        deadline: Duration,
    ) -> Result<ReloadReport, ReloadError> {
        if agent_type != self.agent_type {
            return Err(ReloadError::Internal(format!(
                "reload hook for agent_type={} cannot handle request for {}",
                self.agent_type, agent_type
            )));
        }
        // Peek at the embedded schema hash so we can populate
        // `new_schema_hash` without forcing the caller to spell it
        // out separately on the wire.
        let loaded = load_agent_module(&wasm_bytes)?;
        if loaded.agent_type != agent_type {
            return Err(ReloadError::AgentTypeMismatch {
                requested: agent_type.to_string(),
                embedded: loaded.agent_type,
            });
        }
        let new_schema_hash = loaded.schema_hash;

        let plan = SwapPlan {
            agent_id: self.agent_id,
            agent_type: agent_type.to_string(),
            old_schema_hash: self.current_schema_hash,
            new_schema_hash,
            module: ModuleSource::WasmBytes(&wasm_bytes),
            options: ReloadOptions {
                deadline,
                ..ReloadOptions::default()
            },
        };
        let runner = ReloadRunner {
            plan,
            desc: self.desc.clone(),
            state: self.state.clone(),
            gate: self.gate.clone(),
            drain_signal: self.drain_signal.clone(),
            schema_registry: self.schema_registry.clone(),
            program: self.program.clone(),
        };
        runner.run()
    }
}

/// Thread-safe map of registered reload hooks. The runtime populates
/// this when an agent is spawned (one hook per type); the control
/// socket consults it when an op=reload request arrives.
#[derive(Default)]
pub struct ReloadHookMap {
    inner: Mutex<std::collections::HashMap<String, Arc<dyn ReloadHook>>>,
}

impl ReloadHookMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, agent_type: impl Into<String>, hook: Arc<dyn ReloadHook>) {
        self.inner.lock().insert(agent_type.into(), hook);
    }

    pub fn get(&self, agent_type: &str) -> Option<Arc<dyn ReloadHook>> {
        self.inner.lock().get(agent_type).cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    /// Clear every registered hook.
    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

impl std::fmt::Debug for ReloadHookMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReloadHookMap")
            .field("len", &self.len())
            .finish()
    }
}

/// Process-global reload-hook registry. The runtime registers hooks
/// here at agent-spawn time; the control socket reads from here when
/// an op=reload arrives.
///
/// We keep the registry in a process-global rather than adding a new
/// field to [`ControlContext`] so the existing struct-literal
/// construction in `runtime.rs` (off-limits to this slice) stays
/// source-compatible.
pub fn reload_hooks() -> &'static ReloadHookMap {
    static INSTANCE: std::sync::OnceLock<ReloadHookMap> = std::sync::OnceLock::new();
    INSTANCE.get_or_init(ReloadHookMap::new)
}

/// Inputs the socket server needs. Cloneable so a spawned task can
/// own the registry/map handles without extra plumbing.
#[derive(Clone)]
pub struct ControlContext {
    pub registry: Arc<AgentRegistry>,
    pub introspect: Arc<IntrospectMap>,
    pub worker_count: usize,
}

impl ControlContext {
    /// Compute a response for a single request. Pure function; never
    /// touches I/O. Public so tests can exercise it without spinning
    /// up the socket.
    pub fn handle(&self, req: Request) -> Response {
        match req {
            Request::Snapshot => Response::Snapshot(snapshot_runtime(
                &self.registry,
                &self.introspect,
                self.worker_count,
            )),
            Request::SnapshotAgent { id } => {
                let Some(desc) = self.registry.get(crate::agent::AgentId(id)) else {
                    return Response::Error {
                        error: "not_found".into(),
                        code: None,
                    };
                };
                let intr = self.introspect.get(id);
                Response::Agent(snapshot_agent(&desc, intr.as_deref()))
            }
            Request::List => {
                let mut agents: Vec<AgentListEntry> = self
                    .registry
                    .snapshot_descriptors()
                    .into_iter()
                    .map(|d| AgentListEntry {
                        agent_id: d.id.0,
                        agent_type: d.name.clone(),
                    })
                    .collect();
                agents.sort_by_key(|e| e.agent_id);
                Response::List { agents }
            }
            Request::Reload {
                agent_type,
                module_b64,
                deadline_ms,
            } => self.handle_reload(&agent_type, &module_b64, deadline_ms),
        }
    }

    /// Driver for op=reload. Decodes base64, fetches the registered
    /// hook, calls into the swap pipeline, returns the report or an
    /// error response carrying the `MT506x` diagnostic id.
    fn handle_reload(
        &self,
        agent_type: &str,
        module_b64: &str,
        deadline_ms: Option<u64>,
    ) -> Response {
        let bytes = match base64_decode(module_b64) {
            Ok(b) => b,
            Err(e) => {
                return Response::Error {
                    error: format!("invalid base64 in module_b64: {e}"),
                    code: Some("MT5066".into()),
                }
            }
        };
        if bytes.len() < 8 || &bytes[..4] != b"\0asm" {
            return Response::Error {
                error: format!(
                    "module_b64 doesn't decode to a wasm module (got {} bytes, magic mismatch)",
                    bytes.len()
                ),
                code: Some("MT5066".into()),
            };
        }

        let hooks = reload_hooks();
        if hooks.is_empty() {
            return Response::Error {
                error: "no reload hooks registered on this runtime — \
                    agent must be spawned with a reload hook for op=reload to work"
                    .into(),
                code: Some("MT5061".into()),
            };
        }
        let Some(hook) = hooks.get(agent_type) else {
            return Response::Error {
                error: format!("no reload hook registered for agent_type={agent_type}"),
                code: Some("MT5061".into()),
            };
        };

        let deadline = Duration::from_millis(
            deadline_ms.unwrap_or(ReloadOptions::default().deadline.as_millis() as u64),
        );
        match hook.reload(agent_type, bytes, deadline) {
            Ok(report) => Response::Reload(report),
            Err(e) => Response::Error {
                error: e.to_string(),
                code: Some(e.diag_code().into()),
            },
        }
    }
}

/// Tiny self-contained base64 decoder (RFC 4648). Mirrors the CLI's
/// encoder so the round-trip is well-defined without pulling a
/// workspace dep.
fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "input length {} is not a multiple of 4",
            bytes.len()
        ));
    }
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() / 4 * 3);
    let decode_one = |c: u8| -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            b'=' => Ok(0xFF),
            other => Err(format!("invalid base64 char {:?}", other as char)),
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        let chunk = [
            decode_one(bytes[i])?,
            decode_one(bytes[i + 1])?,
            decode_one(bytes[i + 2])?,
            decode_one(bytes[i + 3])?,
        ];
        let pad = (chunk[2] == 0xFF) as usize + (chunk[3] == 0xFF) as usize;
        out.push((chunk[0] << 2) | (chunk[1] >> 4));
        if pad < 2 {
            out.push(((chunk[1] & 0x0F) << 4) | (chunk[2] >> 2));
        }
        if pad < 1 {
            out.push(((chunk[2] & 0x03) << 6) | chunk[3]);
        }
        i += 4;
    }
    Ok(out)
}

/// Handle returned by [`spawn_control_socket`] — abort it to stop the
/// listener (used at runtime shutdown).
pub struct ControlSocketHandle {
    pub task: tokio::task::JoinHandle<()>,
    pub sock_path: String,
}

/// Returns the configured control-socket path, or `None` if the env
/// var is unset / empty.
pub fn sock_path_from_env() -> Option<String> {
    match std::env::var(CONTROL_SOCK_ENV) {
        Ok(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Spawn the control-socket listener on the given tokio handle.
/// Returns `None` when the env var is unset, when the platform is
/// unsupported (Windows in v0.16), or when binding fails. Failures
/// are logged via `eprintln!` and swallowed — introspection is a
/// best-effort observability surface, never a correctness gate.
pub fn spawn_control_socket(
    ctx: ControlContext,
    handle: TokioHandle,
) -> Option<ControlSocketHandle> {
    let path = sock_path_from_env()?;
    spawn_control_socket_at(ctx, handle, &path)
}

/// Like [`spawn_control_socket`] but with an explicit path. Useful
/// for tests that need to set the path programmatically.
pub fn spawn_control_socket_at(
    ctx: ControlContext,
    handle: TokioHandle,
    path: &str,
) -> Option<ControlSocketHandle> {
    #[cfg(unix)]
    {
        unix_impl::spawn(ctx, &handle, path)
    }
    #[cfg(windows)]
    {
        // Tokio's named-pipe API doesn't expose a stable
        // listener-loop ergonomic in the same shape; deferred to v0.17.
        // See INTROSPECT_V0_16_NOTES.md "Tier-1 followups".
        let _ = (ctx, handle, path);
        eprintln!(
            "[mty-runtime] {} is set but the Windows named-pipe control \
             socket is not yet implemented (v0.16 Unix-only); disable the \
             env var to silence this message",
            CONTROL_SOCK_ENV
        );
        None
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (ctx, handle, path);
        None
    }
}

#[cfg(unix)]
mod unix_impl {
    use super::{ControlContext, ControlSocketHandle, Request, Response};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::runtime::Handle as TokioHandle;

    pub fn spawn(
        ctx: ControlContext,
        handle: &TokioHandle,
        path: &str,
    ) -> Option<ControlSocketHandle> {
        // Remove any stale socket file from a previous run.
        let _ = std::fs::remove_file(path);
        let listener = match UnixListener::bind(path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "[mty-runtime] failed to bind control socket {}: {}",
                    path, e
                );
                return None;
            }
        };
        let sock_path = path.to_string();
        let task = handle.spawn(accept_loop(ctx, listener, sock_path.clone()));
        Some(ControlSocketHandle { task, sock_path })
    }

    async fn accept_loop(ctx: ControlContext, listener: UnixListener, sock_path: String) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let ctx2 = ctx.clone();
                    tokio::spawn(serve_conn(ctx2, stream));
                }
                Err(e) => {
                    eprintln!(
                        "[mty-runtime] control socket {} accept error: {}",
                        sock_path, e
                    );
                    // Backoff briefly to avoid a hot-loop on EMFILE etc.
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }

    async fn serve_conn(ctx: ControlContext, stream: UnixStream) {
        let (rd, mut wr) = stream.into_split();
        let mut lines = BufReader::new(rd).lines();
        loop {
            let line = match lines.next_line().await {
                Ok(Some(l)) => l,
                Ok(None) => break,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let resp = match serde_json::from_str::<Request>(&line) {
                Ok(req) => ctx.handle(req),
                Err(_) => Response::Error {
                    error: "bad_json".into(),
                    code: None,
                },
            };
            let mut bytes = match serde_json::to_vec(&resp) {
                Ok(b) => b,
                Err(_) => br#"{"error":"encode_failed"}"#.to_vec(),
            };
            bytes.push(b'\n');
            if wr.write_all(&bytes).await.is_err() {
                break;
            }
        }
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Snapshot(_) => write!(f, "Response::Snapshot"),
            Response::Agent(_) => write!(f, "Response::Agent"),
            Response::List { agents } => write!(f, "Response::List(n={})", agents.len()),
            Response::Reload(r) => write!(
                f,
                "Response::Reload(agent_id=#{}, type={})",
                r.agent_id, r.agent_type
            ),
            Response::Error { error, code } => write!(
                f,
                "Response::Error({}{})",
                error,
                code.as_deref()
                    .map(|c| format!(" code={c}"))
                    .unwrap_or_default()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentDescriptor, AgentId};
    use crate::budget::{Budget, BudgetTracker};
    use crate::introspect::AgentIntrospectState;
    use crate::mailbox::{Mailbox, SendPolicy};
    use mty_ir::interp::value::Value;
    use mty_ir::ir::AgentIrId;
    use parking_lot::Mutex;
    use std::sync::atomic::AtomicU64;

    fn fixture_ctx() -> ControlContext {
        let registry = Arc::new(AgentRegistry::new());
        let introspect = Arc::new(IntrospectMap::new());
        let id = AgentId(1);
        let desc = Arc::new(AgentDescriptor {
            id,
            name: "Echo".into(),
            sir_id: AgentIrId(0),
            state: Mutex::new(Value::Unit),
            mailbox: Arc::new(Mailbox::new(8, SendPolicy::Block)),
            budget: Arc::new(BudgetTracker::new(Budget::default())),
            supervisor: None,
            mailbox_depth: AtomicU64::new(0),
        });
        registry.insert(desc.clone());
        introspect.insert(id.0, Arc::new(AgentIntrospectState::default()));
        ControlContext {
            registry,
            introspect,
            worker_count: 2,
        }
    }

    #[test]
    fn snapshot_op_returns_runtime_snapshot() {
        let ctx = fixture_ctx();
        let resp = ctx.handle(Request::Snapshot);
        match resp {
            Response::Snapshot(s) => {
                assert_eq!(s.version, 1);
                assert_eq!(s.worker_count, 2);
                assert_eq!(s.agents.len(), 1);
                assert_eq!(s.agents[0].agent_type, "Echo");
            }
            other => panic!("wrong response: {:?}", other),
        }
    }

    #[test]
    fn snapshot_agent_lookup_works() {
        let ctx = fixture_ctx();
        let resp = ctx.handle(Request::SnapshotAgent { id: 1 });
        match resp {
            Response::Agent(a) => assert_eq!(a.agent_id, 1),
            other => panic!("wrong response: {:?}", other),
        }
        let missing = ctx.handle(Request::SnapshotAgent { id: 999 });
        match missing {
            Response::Error { error, .. } => assert_eq!(error, "not_found"),
            other => panic!("wrong response for missing id: {:?}", other),
        }
    }

    #[test]
    fn list_op_returns_agents() {
        let ctx = fixture_ctx();
        match ctx.handle(Request::List) {
            Response::List { agents } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(agents[0].agent_id, 1);
                assert_eq!(agents[0].agent_type, "Echo");
            }
            other => panic!("wrong response: {:?}", other),
        }
    }

    #[test]
    fn env_unset_returns_none() {
        std::env::remove_var(CONTROL_SOCK_ENV);
        assert!(sock_path_from_env().is_none());
    }

    // -----------------------------------------------------------------
    // v0.21 op=reload tests
    // -----------------------------------------------------------------

    fn synth_wasm_module(agent_type: &str, schema_hash: u64) -> Vec<u8> {
        let mut module = wasm_encoder::Module::new();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(crate::reload::SECTION_AGENT_TYPE),
            data: std::borrow::Cow::Borrowed(agent_type.as_bytes()),
        });
        let hash_bytes = schema_hash.to_le_bytes();
        module.section(&wasm_encoder::CustomSection {
            name: std::borrow::Cow::Borrowed(crate::reload::SECTION_SCHEMA_HASH),
            data: std::borrow::Cow::Borrowed(&hash_bytes),
        });
        module.finish()
    }

    fn b64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
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

    #[test]
    fn reload_op_without_hooks_returns_actionable_error() {
        reload_hooks().clear();
        let ctx = fixture_ctx();
        let bytes = synth_wasm_module("Echo", 0xDEAD);
        let resp = ctx.handle(Request::Reload {
            agent_type: "Echo".into(),
            module_b64: b64(&bytes),
            deadline_ms: None,
        });
        match resp {
            Response::Error { error, code } => {
                assert!(error.contains("no reload hooks"), "got: {error}");
                assert_eq!(code.as_deref(), Some("MT5061"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn reload_op_with_bad_base64_returns_clean_error() {
        let ctx = fixture_ctx();
        let resp = ctx.handle(Request::Reload {
            agent_type: "Echo".into(),
            module_b64: "@@@notbase64@@@".into(),
            deadline_ms: None,
        });
        match resp {
            Response::Error { code, .. } => assert_eq!(code.as_deref(), Some("MT5066")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn reload_op_with_non_wasm_payload_rejected_before_hook() {
        let ctx = fixture_ctx();
        let resp = ctx.handle(Request::Reload {
            agent_type: "Echo".into(),
            module_b64: b64(b"definitely-not-wasm"),
            deadline_ms: None,
        });
        match resp {
            Response::Error { code, error } => {
                assert_eq!(code.as_deref(), Some("MT5066"));
                assert!(error.contains("magic mismatch"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn base64_decode_round_trips() {
        let payload = b"the quick brown fox";
        let encoded = b64(payload);
        let decoded = base64_decode(&encoded).expect("decode ok");
        assert_eq!(decoded, payload);
    }

    #[test]
    fn base64_decode_rejects_bad_length() {
        let err = base64_decode("abc").unwrap_err();
        assert!(err.contains("multiple of 4"));
    }
}
