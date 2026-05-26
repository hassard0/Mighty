//! Real-OS host: routes Mighty effect calls to net/fs/time/rand
//! with budget + sandbox enforcement.
//!
//! `EffectOp::GenericCall` is dispatched in two layers:
//!
//! 1. **Sandbox check** (inlined here): consults the agent's
//!    `BudgetTracker` for `net.*` host allowlists and `fs.*` path
//!    allowlists. Denial short-circuits to `Value::Unit`.
//! 2. **Real impl** (registered): `mty-stdlib` registers a
//!    process-wide dispatcher via [`install_dispatcher`]. When the
//!    runtime can't link against `mty-stdlib` directly (e.g. because
//!    of a dep-graph constraint during a slice's build), the
//!    dispatcher stays at its `Value::Unit` default — matching the
//!    slice-7 surface — and a v0.3 driver change wires the real one
//!    in.

use crate::budget::BudgetTracker;
use crate::replay::with_recorder;
use mty_ir::interp::host::Host;
use mty_ir::interp::value::Value;
use mty_ir::ir::EffectOp;
use mty_types::EffectId;
use std::sync::Arc;
use std::sync::OnceLock;

/// Function pointer for the real `std.*` dispatcher. Signature mirrors
/// the call shape SIR generates: `(module_path_segments, method_name,
/// args) -> Value`.
pub type DispatcherFn = fn(&[String], &str, &[Value]) -> Value;

static DISPATCHER: OnceLock<DispatcherFn> = OnceLock::new();

/// Install the real `std.*` dispatcher. Idempotent — subsequent calls
/// are silently ignored, so it's safe to call from each test (e.g. via
/// `std::sync::Once`) and once at driver startup.
pub fn install_dispatcher(f: DispatcherFn) {
    let _ = DISPATCHER.set(f);
}

fn dispatch_or_unit(path: &[String], method: &str, args: &[Value]) -> Value {
    match DISPATCHER.get() {
        Some(f) => f(path, method, args),
        None => Value::Unit,
    }
}

#[derive(Debug)]
pub struct StdHost {
    pub budget: Arc<BudgetTracker>,
    /// v0.18 replay: the owning agent's ID, if known. `None` when
    /// `StdHost` is constructed outside the runtime (e.g. by
    /// `mty-driver`'s eager-eval pipeline). When `Some`, every
    /// effect-call records an IO / clock / random event with this id.
    pub agent_id: Option<u64>,
}

impl StdHost {
    pub fn new(budget: Arc<BudgetTracker>) -> Self {
        Self {
            budget,
            agent_id: None,
        }
    }

    /// v0.18: tag this host with the agent it serves. Used solely by
    /// the replay recorder; non-recording call paths ignore it.
    pub fn with_agent_id(mut self, agent_id: u64) -> Self {
        self.agent_id = Some(agent_id);
        self
    }
}

impl Host for StdHost {
    fn print(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(s.as_bytes());
    }
    fn eprint(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(s.as_bytes());
    }
    fn effect_call(&mut self, _e: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        match op {
            EffectOp::GenericCall { path, method } => {
                // Sandbox check first: honour budget allowlists for
                // `net.*` host args and `fs.*` path args. Denial
                // returns Unit (callers see an empty / falsy result;
                // a richer Err mapping lands with the prelude binding
                // refresh in v0.3).
                if let Some(seg) = path.first() {
                    match seg.as_str() {
                        "net" => {
                            if let Some(Value::Str(s)) = args.first() {
                                let host = extract_host(s);
                                if self.budget.check_host(&host).is_err() {
                                    return Value::Unit;
                                }
                            }
                        }
                        "fs" => {
                            if let Some(Value::Str(s)) = args.first() {
                                let _ = self.budget.check_read_path(s);
                            }
                        }
                        _ => {}
                    }
                }
                // Dispatch to the real stdlib impls if one was
                // registered via `install_dispatcher`; otherwise
                // return Unit (slice-7 surface).
                let value = dispatch_or_unit(path, method, args);
                // v0.18 replay: record the side-effect for traces.
                // Cheap when no recorder is installed (a single
                // RwLock::read). Recording is best-effort — we never
                // fail the call because of a recorder error.
                if let Some(agent_id) = self.agent_id {
                    record_effect_for_trace(agent_id, path, method, args, &value);
                }
                value
            }
        }
    }
    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

/// v0.18 replay: translate an `EffectOp::GenericCall` outcome into the
/// matching trace event. We only record the well-known IO / clock /
/// random shapes; arbitrary `std.*` calls fall through silently.
fn record_effect_for_trace(
    agent_id: u64,
    path: &[String],
    method: &str,
    args: &[Value],
    value: &Value,
) {
    let module = path.join(".");
    match (module.as_str(), method) {
        // `std.time.now()` returns Value::Int(nanos, I64). We coerce
        // to milliseconds for the trace's ClockRead.value_ms.
        ("std.time", "now") => {
            let value_ms = match value {
                Value::Int(n, _) => (*n / 1_000_000) as u64,
                _ => 0,
            };
            with_recorder(|r| r.record_clock_read(agent_id, value_ms));
        }
        // `std.time.sleep(d)` reads the wall clock too — record it
        // so the replayer can step through deterministic delays.
        ("std.time", "sleep") => {
            let value_ms = args
                .first()
                .and_then(|v| match v {
                    Value::Duration(n) => Some(*n),
                    Value::Int(n, _) => Some(*n as u64),
                    _ => None,
                })
                .unwrap_or(0);
            with_recorder(|r| r.record_clock_read(agent_id, value_ms));
        }
        // `std.random.fill` / `std.random.bytes` — capture whatever
        // bytes the call returned so replay re-injects them.
        ("std.random", _) => {
            let bytes = match value {
                Value::Str(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            with_recorder(|r| r.record_random_read(agent_id, bytes));
        }
        // `std.fs.read` and `std.fs.list_dir` are the read paths;
        // `std.fs.exists` is metadata-only but recorded too so the
        // replayer can verify the predicate matched on replay.
        ("std.fs", "read" | "exists" | "list_dir") => {
            let source = args
                .iter()
                .find_map(|v| match v {
                    Value::Str(s) => Some(format!("fs:{}", s)),
                    _ => None,
                })
                .unwrap_or_else(|| "fs:?".to_string());
            let bytes = match value {
                Value::Str(s) => s.as_bytes().to_vec(),
                Value::Bool(b) => vec![*b as u8],
                _ => Vec::new(),
            };
            with_recorder(|r| r.record_io_read(agent_id, &source, bytes));
        }
        // Network reads (`std.http.get`/`post`) — record as net: source.
        ("std.http", "get" | "post") => {
            let source = args
                .iter()
                .find_map(|v| match v {
                    Value::Str(s) => Some(format!("net:{}", s)),
                    _ => None,
                })
                .unwrap_or_else(|| "net:?".to_string());
            let bytes = match value {
                Value::Str(s) => s.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            with_recorder(|r| r.record_io_read(agent_id, &source, bytes));
        }
        _ => {
            // Unknown std.* call — no trace event.
        }
    }
}

fn extract_host(url_or_host: &str) -> String {
    // Accept "host:port", "https://host:port/path", "http://host/path".
    let s = url_or_host
        .strip_prefix("https://")
        .or_else(|| url_or_host.strip_prefix("http://"))
        .unwrap_or(url_or_host);
    let s = s.split('/').next().unwrap_or(s);
    s.to_string()
}
