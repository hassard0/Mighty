//! Runtime values for the SIR interpreter.

use crate::sir::*;
use sdust_types::{AdtId, CapConstraint, CapFamily, FloatKind, IntKind};

#[derive(Debug, Clone)]
pub enum Value {
    Unit,
    Bool(bool),
    Int(i128, IntKind),
    Float(f64, FloatKind),
    Str(String),
    Char(char),
    Duration(u64),
    Size(u64),
    Tuple(Vec<Value>),
    Array(Vec<Value>),
    /// Map placeholder: stored as Array of 2-tuples in slice 6.
    Struct {
        adt: AdtId,
        fields: Vec<Value>,
    },
    Enum {
        adt: AdtId,
        variant: usize,
        payload: Vec<Value>,
    },
    Ref(Reference),
    Fn(FnRef),
    Agent(AgentHandle),
    Cap {
        family: CapFamily,
        constraint: CapConstraint,
    },
    /// Internal: a void / "ignored" value. Distinguishes never-set from
    /// explicit `Unit` for some interpreter trace points.
    Void,
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(n, _) => *n != 0,
            Value::Unit | Value::Void => false,
            _ => true,
        }
    }

    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Int(n, _) => Some(*n),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Char(c) => c.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n, _) => n.to_string(),
            Value::Float(f, _) => f.to_string(),
            Value::Unit => "()".into(),
            Value::Void => "".into(),
            Value::Tuple(xs) => {
                let parts: Vec<String> = xs.iter().map(|v| v.as_str()).collect();
                format!("({})", parts.join(", "))
            }
            Value::Array(xs) => {
                let parts: Vec<String> = xs.iter().map(|v| v.as_str()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Enum {
                variant, payload, ..
            } => {
                if payload.is_empty() {
                    format!("V{}", variant)
                } else {
                    let parts: Vec<String> = payload.iter().map(|v| v.as_str()).collect();
                    format!("V{}({})", variant, parts.join(", "))
                }
            }
            Value::Struct { fields, .. } => {
                let parts: Vec<String> = fields.iter().map(|v| v.as_str()).collect();
                format!("{{{}}}", parts.join(", "))
            }
            Value::Ref(r) => format!("ref(_{})", r.owner.0),
            Value::Fn(_) => "<fn>".into(),
            Value::Agent(_) => "<agent>".into(),
            Value::Cap { family, .. } => format!("<cap {:?}>", family),
            Value::Duration(n) => format!("{}ms", n),
            Value::Size(n) => format!("{}B", n),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeId(pub u64);

#[derive(Debug, Clone)]
pub struct Reference {
    pub scope: ScopeId,
    pub owner: Local,
    pub proj: Vec<Projection>,
    pub mutable: bool,
}

#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub id: u64,
    pub agent_sir_id: AgentSirId,
    /// Index into the interpreter's agent_states vector.
    pub state_idx: usize,
}

/// Per-call activation record.
#[derive(Debug)]
pub struct Frame {
    pub fn_id: SirFnId,
    pub locals: Vec<Value>,
    pub scope: ScopeId,
    pub block: BlockId,
    pub pc: usize,
    /// Stack of arena ids currently live.
    pub arenas: Vec<ArenaId>,
}

impl Frame {
    pub fn new(fn_id: SirFnId, locals: Vec<Value>, scope: ScopeId, entry: BlockId) -> Self {
        Self {
            fn_id,
            locals,
            scope,
            block: entry,
            pc: 0,
            arenas: vec![],
        }
    }
}
