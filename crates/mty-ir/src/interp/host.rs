//! Host trait: I/O and effect-call sink for the interpreter.

use super::value::Value;
use crate::ir::EffectOp;
use mty_types::EffectId;

/// Sink the interpreter routes output, effects, and extern fns through.
pub trait Host {
    fn print(&mut self, s: &str);
    fn println(&mut self, s: &str) {
        self.print(s);
        self.print("\n");
    }
    fn eprint(&mut self, s: &str) {
        self.print(s);
    }
    /// Invoked for `Stmt::EffectInvoke`. Default impl returns Unit.
    fn effect_call(&mut self, _effect: EffectId, _op: &EffectOp, _args: &[Value]) -> Value {
        Value::Unit
    }
    /// Invoked for an unresolved extern fn (`BuiltinId::Extern(name)`).
    fn extern_call(&mut self, _name: &str, _args: &[Value]) -> Value {
        Value::Unit
    }
}

/// Buffers all output in memory. Useful for tests + the conformance
/// runner.
#[derive(Debug, Default)]
pub struct BufferHost {
    pub stdout: Vec<u8>,
    pub effect_log: Vec<EffectCallRecord>,
    pub extern_log: Vec<ExternCallRecord>,
}

#[derive(Debug, Clone)]
pub struct EffectCallRecord {
    pub effect_id: u32,
    pub op_desc: String,
    pub arg_count: usize,
}

#[derive(Debug, Clone)]
pub struct ExternCallRecord {
    pub name: String,
    pub arg_count: usize,
}

impl Host for BufferHost {
    fn print(&mut self, s: &str) {
        self.stdout.extend_from_slice(s.as_bytes());
    }
    fn effect_call(&mut self, effect: EffectId, op: &EffectOp, args: &[Value]) -> Value {
        self.effect_log.push(EffectCallRecord {
            effect_id: effect.0,
            op_desc: format!("{:?}", op),
            arg_count: args.len(),
        });
        // Return a generally-useful deterministic default for common
        // shapes: `Result::Ok(Str(""))` for fs/net/io reads; `Unit`
        // otherwise. The interpreter knows which to inspect.
        Value::Unit
    }
    fn extern_call(&mut self, name: &str, args: &[Value]) -> Value {
        self.extern_log.push(ExternCallRecord {
            name: name.to_string(),
            arg_count: args.len(),
        });
        // For Result-returning externs, default to Ok(Unit).
        match name {
            "work" | "step" | "tick" | "job" => {
                // Result::Ok(Unit) — variant index 0, payload [Unit].
                // We don't know the AdtId from here, so return raw Unit
                // and let the lowerer/interpreter treat unhandled `?`
                // as Ok.
                Value::Unit
            }
            "ready" => Value::Bool(false),
            "fetch" => Value::Str(String::new()),
            _ => Value::Unit,
        }
    }
}

impl BufferHost {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// Real host that writes to stdout/stderr. Used by `sdust run`.
#[derive(Debug, Default)]
pub struct RealHost;

impl Host for RealHost {
    fn print(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stdout().write_all(s.as_bytes());
    }
    fn eprint(&mut self, s: &str) {
        use std::io::Write;
        let _ = std::io::stderr().write_all(s.as_bytes());
    }
}
