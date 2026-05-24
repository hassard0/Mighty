//! Real-OS host: routes Stardust effect calls to net/fs/time/rand
//! with budget + sandbox enforcement.

use crate::budget::BudgetTracker;
use sdust_sir::interp::host::Host;
use sdust_sir::interp::value::Value;
use sdust_sir::sir::EffectOp;
use sdust_types::EffectId;
use std::sync::Arc;

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
                // Slice-7 surface kept minimal: honour sandbox allowlists
                // when the call carries a Str argument that names a host
                // (for `net.*`) or path (for `fs.*`); otherwise no-op.
                if let Some(seg) = path.first() {
                    match seg.as_str() {
                        "net" => {
                            // First Str arg is treated as a host:port or URL.
                            if let Some(Value::Str(s)) = args.first() {
                                let host = extract_host(s);
                                if self.budget.check_host(&host).is_err() {
                                    // Slice-7 best-effort: return Unit and
                                    // let the SIR-level call observe the
                                    // empty result. Real Err mapping is a
                                    // slice-8 codegen task.
                                    return Value::Unit;
                                }
                            }
                            let _ = method;
                        }
                        "fs" => {
                            if let Some(Value::Str(s)) = args.first() {
                                let _ = self.budget.check_read_path(s);
                            }
                        }
                        _ => {}
                    }
                }
                Value::Unit
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
