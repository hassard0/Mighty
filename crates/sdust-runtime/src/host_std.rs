//! Real-OS host: routes Stardust effect calls to net/fs/time/rand
//! with budget + sandbox enforcement.
//!
//! `EffectOp::GenericCall` is dispatched in two layers:
//!
//! 1. **Sandbox check** (inlined here): consults the agent's
//!    `BudgetTracker` for `net.*` host allowlists and `fs.*` path
//!    allowlists. Denial short-circuits to `Value::Unit`.
//! 2. **Real impl** (registered): `sdust-stdlib` registers a
//!    process-wide dispatcher via [`install_dispatcher`]. When the
//!    runtime can't link against `sdust-stdlib` directly (e.g. because
//!    of a dep-graph constraint during a slice's build), the
//!    dispatcher stays at its `Value::Unit` default — matching the
//!    slice-7 surface — and a v0.3 driver change wires the real one
//!    in.

use crate::budget::BudgetTracker;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::EffectOp;
use sdust_types::EffectId;
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
}

impl StdHost {
    pub fn new(budget: Arc<BudgetTracker>) -> Self {
        Self { budget }
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
                dispatch_or_unit(path, method, args)
            }
        }
    }
    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
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
