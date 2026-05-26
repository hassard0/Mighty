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
//!
//! Unknown ops return `{"error":"unknown_op"}`. Bad JSON returns
//! `{"error":"bad_json"}` and the connection stays open.

use crate::agent::AgentRegistry;
use crate::introspect::{
    snapshot_agent, snapshot_runtime, AgentListEntry, AgentSnapshot, IntrospectMap, RuntimeSnapshot,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::runtime::Handle as TokioHandle;

/// Name of the env var the runtime consults at startup.
pub const CONTROL_SOCK_ENV: &str = "MTY_RUNTIME_CONTROL_SOCK";

/// Inbound op.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Snapshot,
    SnapshotAgent { id: u64 },
    List,
}

/// Outbound op. Each response shape gets its own variant so JSON
/// consumers can switch on the shape without looking at a tag.
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum Response {
    Snapshot(RuntimeSnapshot),
    Agent(AgentSnapshot),
    List { agents: Vec<AgentListEntry> },
    Error { error: String },
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
                    };
                };
                let intr = self.introspect.get(id);
                Response::Agent(snapshot_agent(&desc, intr.as_deref()))
            }
            Request::List => {
                let mut agents: Vec<AgentListEntry> = self
                    .registry
                    .iter()
                    .into_iter()
                    .map(|d| AgentListEntry {
                        agent_id: d.id.0,
                        agent_type: d.name.clone(),
                    })
                    .collect();
                agents.sort_by_key(|e| e.agent_id);
                Response::List { agents }
            }
        }
    }
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
    handle: &TokioHandle,
) -> Option<ControlSocketHandle> {
    let path = sock_path_from_env()?;
    spawn_control_socket_at(ctx, handle, &path)
}

/// Like [`spawn_control_socket`] but with an explicit path. Useful
/// for tests that need to set the path programmatically.
pub fn spawn_control_socket_at(
    ctx: ControlContext,
    handle: &TokioHandle,
    path: &str,
) -> Option<ControlSocketHandle> {
    #[cfg(unix)]
    {
        unix_impl::spawn(ctx, handle, path)
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
            Response::Error { error } => assert_eq!(error, "not_found"),
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
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Response::Snapshot(_) => write!(f, "Response::Snapshot"),
            Response::Agent(_) => write!(f, "Response::Agent"),
            Response::List { agents } => write!(f, "Response::List(n={})", agents.len()),
            Response::Error { error } => write!(f, "Response::Error({})", error),
        }
    }
}
