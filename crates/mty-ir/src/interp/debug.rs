//! v0.32 Track A — debug-aware interpreter loop.
//!
//! The [`DebugSession`] is a thin wrapper around the existing
//! `Interp` that yields control to a [`BreakpointHook`] between
//! steps. Resumption is in-place — the session preserves the
//! interpreter state across pauses so the DAP loop can continue
//! / step / etc. without re-launching.
//!
//! The DAP server (`mty dap`) owns one `DebugSession` per debug
//! target. The session exposes:
//!
//! - [`DebugSession::run_until_break`] — resume until the hook
//!   returns `Break` or the program exits.
//! - [`DebugSession::stack_frames`] — read the current call stack
//!   for DAP `stackTrace` responses.
//! - [`DebugSession::locals`] — read the current frame's locals
//!   for DAP `variables` responses.
//! - [`DebugSession::current_position`] — peek the next-to-execute
//!   position for DAP `stopped` events.

use super::breakpoints::{BreakDecision, BreakpointHook, StepPosition};
use super::host::Host;
use super::run::{Interp, RunResult, StepOutcome};
use super::value::{Frame, Value};
use crate::ir::{Function, IrFnId, LocalDecl, Program};

/// Outcome of a [`DebugSession::run_until_break`] call.
#[derive(Debug, Clone)]
pub enum DebugStop {
    /// The program ran to completion (no more frames). `result`
    /// carries the final `RunResult`; the session is dead after this.
    Completed(RunResult),
    /// A breakpoint hook fired and asked us to suspend. The session
    /// is paused at the position returned by
    /// [`DebugSession::current_position`].
    Breakpoint(BreakReason),
    /// A runtime trap fired during execution.
    Trap { code: &'static str, message: String },
}

/// Why the session paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakReason {
    /// A `before_step` hook returned `Break`.
    Step,
    /// An `on_call` hook returned `Break`.
    FunctionEntry,
    /// An `on_return` hook returned `Break`.
    FunctionExit,
    /// The DAP-level step / next / stepOut control hit its target.
    StepComplete,
    /// Initial pause: the session was constructed but no steps have
    /// run yet. The DAP server uses this to surface a `stopped`
    /// event on launch when `stopOnEntry: true`.
    Entry,
}

/// A live debug session.
pub struct DebugSession<'a> {
    interp: Interp<'a>,
    /// Initialised on first `run_until_break` to surface an entry
    /// stop. After that, becomes false.
    pending_entry_stop: bool,
}

impl<'a> DebugSession<'a> {
    /// Build a debug session that will start executing the named fn
    /// with the provided args + step budget.
    pub fn new_for_fn(
        prog: &'a Program,
        fn_name: &str,
        args: Vec<Value>,
        step_budget: u64,
    ) -> Option<Self> {
        let f = prog.fn_by_name(fn_name)?;
        let mut interp = Interp::new(prog, step_budget);
        let initial_locals = build_initial_locals(f, &args);
        let scope = interp.fresh_scope_pub();
        let frame = Frame::new(f.id, initial_locals, scope, f.entry);
        interp.stack.push(frame);
        Some(Self {
            interp,
            pending_entry_stop: true,
        })
    }

    /// Resume execution. The session calls `hook.before_step` /
    /// `hook.on_call` / `hook.on_return` between interpreter steps
    /// and returns when one of them asks to break (or the program
    /// completes / traps).
    ///
    /// `stop_on_entry` — if true and this is the first call into the
    /// session, return immediately with [`BreakReason::Entry`] so the
    /// caller can surface a launch-time stopped event.
    pub fn run_until_break(
        &mut self,
        host: &mut dyn Host,
        hook: &mut dyn BreakpointHook,
        stop_on_entry: bool,
    ) -> DebugStop {
        if self.pending_entry_stop && stop_on_entry {
            self.pending_entry_stop = false;
            return DebugStop::Breakpoint(BreakReason::Entry);
        }
        self.pending_entry_stop = false;

        loop {
            // Peek before stepping so the hook sees the about-to-
            // execute position.
            let Some(pos) = self.interp.peek_position() else {
                return DebugStop::Completed(RunResult::Ok { exit: 0 });
            };
            let depth = self.interp.stack.len();
            if hook.before_step(&pos, depth) == BreakDecision::Break {
                return DebugStop::Breakpoint(BreakReason::Step);
            }
            // Step.
            match self.interp.step(host) {
                StepOutcome::Continue => {}
                StepOutcome::FrameReturned(value) => {
                    self.interp.set_last_return(value.clone());
                    if self.interp.stack.len() == 1 {
                        if let Some(top) = self.interp.stack.last() {
                            self.interp.set_last_frame_locals(top.locals.clone());
                        }
                    }
                    self.interp.stack.pop();
                    let return_depth = self.interp.stack.len();
                    if hook.on_return(return_depth) == BreakDecision::Break {
                        if self.interp.stack.is_empty() {
                            return DebugStop::Completed(crate::interp::run::main_exit_for_value(
                                &value,
                            ));
                        }
                        return DebugStop::Breakpoint(BreakReason::FunctionExit);
                    }
                    if self.interp.stack.is_empty() {
                        return DebugStop::Completed(crate::interp::run::main_exit_for_value(
                            &value,
                        ));
                    }
                }
                StepOutcome::Trap(code, msg) => {
                    return DebugStop::Trap { code, message: msg };
                }
            }
        }
    }

    /// Read the current call stack — outermost (oldest) first to
    /// innermost (top) last. Matches DAP's `stackTrace` order when
    /// reversed by the caller.
    pub fn stack_frames(&self) -> Vec<DebugFrame> {
        self.interp
            .stack
            .iter()
            .map(|f| {
                let func = self.interp.prog.fn_by_id(f.fn_id);
                DebugFrame {
                    fn_id: f.fn_id,
                    fn_name: func.name.clone(),
                    block_idx: f.block.0,
                    pc: f.pc,
                    span_start: func.span.start,
                    span_end: func.span.end,
                }
            })
            .collect()
    }

    /// Read the active (top) frame's locals as a `(name, repr)` list.
    /// Names come from the function's `LocalDecl` table; the local-0
    /// "return slot" is filtered out.
    pub fn locals(&self) -> Vec<DebugLocal> {
        let Some(f) = self.interp.stack.last() else {
            return Vec::new();
        };
        let func = self.interp.prog.fn_by_id(f.fn_id);
        f.locals
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                if i == 0 {
                    return None; // return slot
                }
                let name = func
                    .locals
                    .get(i)
                    .map(|d: &LocalDecl| d.name.clone())
                    .unwrap_or_else(|| format!("_{i}"));
                if matches!(v, Value::Void) {
                    return None;
                }
                Some(DebugLocal {
                    name,
                    repr: v.as_str(),
                    kind: value_kind(v),
                })
            })
            .collect()
    }

    /// Peek the next step position. Returns `None` if the program
    /// has completed (no live frames).
    pub fn current_position(&self) -> Option<StepPosition> {
        self.interp.peek_position()
    }

    /// Number of live frames (== call depth).
    pub fn depth(&self) -> usize {
        self.interp.stack.len()
    }

    /// True once the interpreter has no live frames.
    pub fn is_finished(&self) -> bool {
        self.interp.stack.is_empty()
    }
}

/// One frame for DAP `stackTrace`.
#[derive(Debug, Clone)]
pub struct DebugFrame {
    pub fn_id: IrFnId,
    pub fn_name: String,
    pub block_idx: u32,
    pub pc: usize,
    pub span_start: u32,
    pub span_end: u32,
}

/// One local for DAP `variables`.
#[derive(Debug, Clone)]
pub struct DebugLocal {
    pub name: String,
    pub repr: String,
    pub kind: &'static str,
}

fn value_kind(v: &Value) -> &'static str {
    match v {
        Value::Unit | Value::Void => "unit",
        Value::Bool(_) => "bool",
        Value::Int(_, _) => "int",
        Value::Float(_, _) => "float",
        Value::Str(_) => "string",
        Value::Char(_) => "char",
        Value::Duration(_) => "duration",
        Value::Size(_) => "size",
        Value::Tuple(_) => "tuple",
        Value::Array(_) => "array",
        Value::Struct { .. } => "struct",
        Value::Enum { .. } => "enum",
        Value::Ref(_) => "ref",
        Value::Fn(_) => "fn",
        Value::Agent(_) => "agent",
        Value::Cap { .. } => "cap",
    }
}

fn build_initial_locals(f: &Function, args: &[Value]) -> Vec<Value> {
    let mut locals = vec![Value::Void; f.locals.len()];
    locals[0] = Value::Void;
    for (i, p) in f.params.iter().enumerate() {
        let v = args.get(i).cloned().unwrap_or(Value::Unit);
        if (p.0 as usize) < locals.len() {
            locals[p.0 as usize] = v;
        }
    }
    locals
}

/// Helper exposed on `Interp` so `DebugSession` can mint scopes
/// without re-implementing the monotonic counter.
impl<'a> Interp<'a> {
    pub(crate) fn fresh_scope_pub(&mut self) -> super::value::ScopeId {
        let s = super::value::ScopeId(self.next_scope);
        self.next_scope += 1;
        s
    }

    pub(crate) fn set_last_return(&mut self, v: Value) {
        self.last_return = v;
    }

    pub(crate) fn set_last_frame_locals(&mut self, locals: Vec<Value>) {
        self.last_frame_locals = Some(locals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::breakpoints::{offset_to_line, BreakDecision, BreakpointHook, NullHook};
    use crate::interp::host::RealHost;
    use crate::ir::{
        Block, BlockId, Const, Function, IrFnId, IrTy, Local, LocalDecl, LocalSource, Operand,
        Place, Program, Rvalue, Stmt, Term,
    };
    use mty_hir::SourceSpan;

    /// Tiny one-block program: `let x = 41 + 1; return x`.
    fn tiny_program() -> Program {
        let mut prog = Program::default();
        let f = Function {
            id: IrFnId(0),
            name: "main".into(),
            params: vec![],
            locals: vec![
                LocalDecl {
                    name: "_ret".into(),
                    ty: IrTy::Unit,
                    mutable: false,
                    source: LocalSource::Return,
                },
                LocalDecl {
                    name: "x".into(),
                    ty: IrTy::Int(mty_types::IntKind::I32),
                    mutable: false,
                    source: LocalSource::UserLet,
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                stmts: vec![
                    // x = 41 + 1
                    Stmt::Assign(
                        Place::local(Local(1)),
                        Rvalue::BinOp(
                            crate::ir::BinOp::Add,
                            Operand::Const(Const::Int(41, mty_types::IntKind::I32)),
                            Operand::Const(Const::Int(1, mty_types::IntKind::I32)),
                        ),
                    ),
                    // _ret = x
                    Stmt::Assign(
                        Place::local(Local(0)),
                        Rvalue::Use(Operand::Copy(Place::local(Local(1)))),
                    ),
                ],
                terminator: Term::Return(Operand::Copy(Place::local(Local(0)))),
            }],
            entry: BlockId(0),
            ret_ty: IrTy::Int(mty_types::IntKind::I32),
            effects: vec![],
            hir_fn: None,
            span: SourceSpan { start: 0, end: 0 },
        };
        prog.fns.push(f);
        prog
    }

    #[test]
    fn debug_session_constructs_and_completes_with_null_hook() {
        let prog = tiny_program();
        let mut sess = DebugSession::new_for_fn(&prog, "main", vec![], 1_000).unwrap();
        assert_eq!(sess.depth(), 1);
        let mut host = RealHost;
        let mut hook = NullHook;
        let stop = sess.run_until_break(&mut host, &mut hook, false);
        assert!(matches!(stop, DebugStop::Completed(_)));
        assert!(sess.is_finished());
    }

    #[test]
    fn debug_session_honors_stop_on_entry() {
        let prog = tiny_program();
        let mut sess = DebugSession::new_for_fn(&prog, "main", vec![], 1_000).unwrap();
        let mut host = RealHost;
        let mut hook = NullHook;
        let stop = sess.run_until_break(&mut host, &mut hook, true);
        assert!(matches!(stop, DebugStop::Breakpoint(BreakReason::Entry)));
        assert!(!sess.is_finished());
        // Resuming runs to completion.
        let stop2 = sess.run_until_break(&mut host, &mut hook, false);
        assert!(matches!(stop2, DebugStop::Completed(_)));
    }

    /// Hook that breaks the first time `before_step` fires.
    struct OneShot {
        fired: bool,
    }
    impl BreakpointHook for OneShot {
        fn before_step(&mut self, _pos: &StepPosition, _depth: usize) -> BreakDecision {
            if !self.fired {
                self.fired = true;
                BreakDecision::Break
            } else {
                BreakDecision::Continue
            }
        }
    }

    #[test]
    fn debug_session_surfaces_step_break() {
        let prog = tiny_program();
        let mut sess = DebugSession::new_for_fn(&prog, "main", vec![], 1_000).unwrap();
        let mut host = RealHost;
        let mut hook = OneShot { fired: false };
        let stop = sess.run_until_break(&mut host, &mut hook, false);
        assert!(matches!(stop, DebugStop::Breakpoint(BreakReason::Step)));
        // Resume → completes (the hook only fires once).
        let stop2 = sess.run_until_break(&mut host, &mut hook, false);
        assert!(matches!(stop2, DebugStop::Completed(_)));
    }

    #[test]
    fn debug_session_reports_locals_and_frames() {
        let prog = tiny_program();
        let mut sess = DebugSession::new_for_fn(&prog, "main", vec![], 1_000).unwrap();
        let mut host = RealHost;
        let mut hook = OneShot { fired: false };
        let _ = sess.run_until_break(&mut host, &mut hook, true);
        let frames = sess.stack_frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].fn_name, "main");
        // After stopOnEntry the frame is still fresh; locals are
        // mostly Void so we may get nothing — that's expected. The
        // important assertion is that the API doesn't panic.
        let _locals = sess.locals();
    }

    /// Verify offset_to_line returns the right line for a known span.
    #[test]
    fn offset_to_line_integration() {
        let src = "fn main() {\n  let x = 1\n}\n";
        assert_eq!(offset_to_line(src, 14), 2);
    }

    /// Function-entry hook test: install an on_call that breaks the
    /// FIRST time it sees any call, exercises the path even though
    /// tiny_program has no call.
    struct CallBreaker {
        broke: bool,
    }
    impl BreakpointHook for CallBreaker {
        fn on_call(&mut self, _callee: IrFnId, _depth: usize) -> BreakDecision {
            self.broke = true;
            BreakDecision::Break
        }
    }

    #[test]
    fn debug_session_runs_callbreaker_without_calls() {
        // tiny_program has no user-fn calls, so on_call never fires;
        // session runs to completion.
        let prog = tiny_program();
        let mut sess = DebugSession::new_for_fn(&prog, "main", vec![], 1_000).unwrap();
        let mut host = RealHost;
        let mut hook = CallBreaker { broke: false };
        let stop = sess.run_until_break(&mut host, &mut hook, false);
        assert!(matches!(stop, DebugStop::Completed(_)));
        assert!(!hook.broke);
    }
}
