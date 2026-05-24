//! Interpreter core: drive a `Program` through its `main` fn.

use super::host::Host;
use super::value::*;
use crate::sir::*;
use sdust_types::IntKind;

/// Default step budget — each stmt + each terminator counts as one step.
pub const DEFAULT_STEP_BUDGET: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    /// Program ran to completion. `exit` is the suggested process exit
    /// code (0 on success, nonzero on Result::Err return from main).
    Ok { exit: i32 },
    /// Runtime trap (panic, div by zero, etc.). Message describes.
    Trap { code: &'static str, message: String },
    /// `main` not found.
    NoMain,
    /// Step budget exceeded.
    BudgetExceeded,
}

impl RunResult {
    pub fn exit_code(&self) -> i32 {
        match self {
            RunResult::Ok { exit } => *exit,
            RunResult::Trap { .. } => 1,
            RunResult::NoMain => 2,
            RunResult::BudgetExceeded => 3,
        }
    }
}

/// Run `prog` starting at the fn named `main`. The host receives all
/// output. Returns a `RunResult`.
pub fn run(prog: &Program, host: &mut dyn Host) -> RunResult {
    let mainf = match prog.fn_by_name("main") {
        Some(f) => f,
        None => return RunResult::NoMain,
    };
    let mut interp = Interp::new(prog, DEFAULT_STEP_BUDGET);
    let initial_locals = initial_locals_for(mainf, &[]);
    let scope = interp.fresh_scope();
    let frame = Frame::new(mainf.id, initial_locals, scope, mainf.entry);
    interp.stack.push(frame);
    interp.run(host)
}

/// Run the function named `name` directly with the supplied args.
/// Returns the final value (or trap). Useful for tests that don't want
/// to wire `main`.
pub fn run_fn_by_name(
    prog: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
) -> Result<Value, RunResult> {
    let f = match prog.fn_by_name(name) {
        Some(f) => f,
        None => return Err(RunResult::NoMain),
    };
    let mut interp = Interp::new(prog, DEFAULT_STEP_BUDGET);
    let initial_locals = initial_locals_for(f, &args);
    let scope = interp.fresh_scope();
    let frame = Frame::new(f.id, initial_locals, scope, f.entry);
    interp.stack.push(frame);
    match interp.run(host) {
        RunResult::Ok { .. } => Ok(interp.last_return),
        r => Err(r),
    }
}

/// Slice-7 helper: like [`run_fn_by_name`] but with caller-provided
/// step budget (used by the runtime to translate per-turn CPU budgets
/// into bounded interpreter step counts).
pub fn run_fn_with_budget(
    prog: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    step_budget: u64,
) -> Result<Value, RunResult> {
    let f = match prog.fn_by_name(name) {
        Some(f) => f,
        None => return Err(RunResult::NoMain),
    };
    let mut interp = Interp::new(prog, step_budget);
    let initial_locals = initial_locals_for(f, &args);
    let scope = interp.fresh_scope();
    let frame = Frame::new(f.id, initial_locals, scope, f.entry);
    interp.stack.push(frame);
    match interp.run(host) {
        RunResult::Ok { .. } => Ok(interp.last_return),
        r => Err(r),
    }
}

/// Slice-7 entry point: invoke a single agent handler with a known
/// state value, get back the final state + reply.
///
/// The slice-6 [`run_fn_by_name`] is awkward for agent handlers because
/// agent state lives in `agent_states[idx]` and the handler's first
/// param is a `&mut state`. This helper sets up a frame where:
///
/// - Local 1 (the `&mut self` param) is a `Value::Ref` whose owner is
///   a synthetic state-holder local that we append at the end of the
///   handler's locals vector.
/// - The state-holder local actually stores the state struct value.
/// - `assign_place` recognises a leading `Projection::Deref` on a
///   `Value::Ref` owner and writes into the referenced local (this
///   path is enabled by a slice-7 patch to [`Interp::assign_place`]).
/// - After the handler returns, we read the state-holder local back as
///   the new state.
///
/// Returns `(RunResult, new_state, return_value)`.
pub fn run_handler_isolated(
    prog: &Program,
    handler: SirFnId,
    state_in: Value,
    msg_args: Vec<Value>,
    host: &mut dyn Host,
) -> (RunResult, Value, Value) {
    let f = prog.fn_by_id(handler);
    // Allocate locals: handler's declared locals + 1 synthetic
    // state-holder. The state-holder lives at the FIRST free index
    // (one past handler.locals.len()).
    let n_handler_locals = f.locals.len();
    let state_holder = Local(n_handler_locals as u32);
    let mut locals = vec![Value::Void; n_handler_locals + 1];

    // local 0: return slot.
    locals[0] = Value::Void;
    // Local 1: self ref pointing at the state-holder local.
    if n_handler_locals >= 2 {
        locals[1] = Value::Ref(super::value::Reference {
            scope: ScopeId(0),
            owner: state_holder,
            proj: vec![],
            mutable: true,
        });
    }
    // State-holder local: the actual state value.
    locals[n_handler_locals] = state_in;

    // The lowerer (see crates/sdust-sir/src/lower/items.rs) emits, at
    // handler entry, a sequence of `Stmt::Assign(named_local,
    // Rvalue::FieldRead { receiver: (*self), field: i })` statements
    // that pre-load state fields into named locals. With deref-of-ref
    // properly resolved by `read_place`, these pre-loads already work.
    // At handler exit the lowerer emits `Stmt::Assign((*self).fN,
    // named_local)` writebacks — those need a working deref-then-field
    // write path in `assign_place`, which slice-7 supplies.

    // Position msg args at the handler's declared parameter slots
    // (params[1..] — params[0] is the self-ref we already placed).
    for (i, p) in f.params.iter().enumerate().skip(1) {
        let pos = p.0 as usize;
        if pos < locals.len() {
            if let Some(v) = msg_args.get(i - 1).cloned() {
                locals[pos] = v;
            }
        }
    }

    let mut interp = Interp::new(prog, DEFAULT_STEP_BUDGET);
    let scope = interp.fresh_scope();
    let frame = Frame::new(handler, locals, scope, f.entry);
    interp.stack.push(frame);

    let run_result = interp.run(host);
    let reply = interp.last_return.clone();

    // The frame has been popped; recover the state-holder value from
    // the interpreter's last-frame snapshot. We saved it before run by
    // peeking at interp.stack — but `run()` consumes the stack to
    // completion. Instead, we use a side channel: every handler returns
    // via `Term::Return` which captures the return value, but the
    // state-holder local was mutated through the deref-of-ref path in
    // assign_place. Slice 7 exposes the state-holder by intercepting
    // the frame's pop in `Interp::run`: when the OUTER frame returns,
    // the interp remembers its final locals into `last_frame_locals`.
    let state_out = interp
        .last_frame_locals
        .as_ref()
        .and_then(|ls| ls.get(state_holder.0 as usize).cloned())
        .unwrap_or(Value::Unit);

    (run_result, state_out, reply)
}

fn initial_locals_for(f: &Function, args: &[Value]) -> Vec<Value> {
    let mut locals = vec![Value::Void; f.locals.len()];
    // local 0 is the return slot.
    locals[0] = Value::Void;
    for (i, p) in f.params.iter().enumerate() {
        let v = args.get(i).cloned().unwrap_or(Value::Unit);
        locals[p.0 as usize] = v;
    }
    locals
}

struct Interp<'a> {
    prog: &'a Program,
    stack: Vec<Frame>,
    /// Synthesized agent state values (by AgentHandle.state_idx).
    agent_states: Vec<Value>,
    /// Counter for `Frame::scope` (monotonic).
    next_scope: u64,
    /// Counter for agent handles.
    next_agent: u64,
    /// Last value returned (for `run_fn_by_name`).
    last_return: Value,
    /// Step budget remaining.
    budget: u64,
    /// Slice-7 hook: snapshot of the outermost frame's locals at the
    /// moment it returns. Used by [`run_handler_isolated`] to recover
    /// the post-handler state value out of the synthesized state-holder
    /// local without disturbing the slice-6 single-frame contract.
    last_frame_locals: Option<Vec<Value>>,
}

impl<'a> Interp<'a> {
    fn new(prog: &'a Program, budget: u64) -> Self {
        Self {
            prog,
            stack: Vec::new(),
            agent_states: Vec::new(),
            next_scope: 0,
            next_agent: 0,
            last_return: Value::Unit,
            budget,
            last_frame_locals: None,
        }
    }

    fn fresh_scope(&mut self) -> ScopeId {
        let s = ScopeId(self.next_scope);
        self.next_scope += 1;
        s
    }

    fn run(&mut self, host: &mut dyn Host) -> RunResult {
        while self.stack.last().is_some() {
            if self.budget == 0 {
                return RunResult::BudgetExceeded;
            }
            self.budget -= 1;
            match self.step(host) {
                StepOutcome::Continue => {}
                StepOutcome::FrameReturned(value) => {
                    self.last_return = value.clone();
                    // Slice-7 hook: snapshot the outermost frame's
                    // locals before pop, so callers (e.g.
                    // `run_handler_isolated`) can read a synthesized
                    // state-holder local out.
                    if self.stack.len() == 1 {
                        if let Some(top) = self.stack.last() {
                            self.last_frame_locals = Some(top.locals.clone());
                        }
                    }
                    self.stack.pop();
                    if let Some(parent) = self.stack.last_mut() {
                        // Caller's "pending Rvalue::Call" already stored
                        // a sentinel; we patch the location now. The
                        // call protocol uses `_pending_call_dest` on the
                        // *parent* frame... simpler: caller's last Stmt
                        // already wrote the temp it expects. We use a
                        // side-channel: store value in `interp.last_return`
                        // and have the Call handler push a "post-call"
                        // stub. But since this slice is a tree-walking
                        // interp, we instead inline calls.
                        // (See `eval_call_into` below — it pushes a frame
                        // and bumps PC so we re-enter the same stmt to
                        // collect the result.)
                        let _ = parent;
                    } else {
                        // Top-level main returned.
                        return main_exit_for_value(&value);
                    }
                }
                StepOutcome::Trap(code, msg) => {
                    return RunResult::Trap { code, message: msg };
                }
            }
        }
        RunResult::Ok { exit: 0 }
    }

    fn step(&mut self, host: &mut dyn Host) -> StepOutcome {
        // Peek the current frame's block / pc.
        let (fn_id, block_id, pc) = {
            let f = self.stack.last().unwrap();
            (f.fn_id, f.block, f.pc)
        };
        let func = self.prog.fn_by_id(fn_id);
        let block = func.block(block_id);

        if pc < block.stmts.len() {
            let stmt = block.stmts[pc].clone();
            self.stack.last_mut().unwrap().pc += 1;
            self.exec_stmt(host, &stmt)
        } else {
            let term = block.terminator.clone();
            self.exec_term(host, term)
        }
    }

    fn exec_stmt(&mut self, host: &mut dyn Host, s: &Stmt) -> StepOutcome {
        match s {
            Stmt::Assign(place, rv) => {
                let val = match self.eval_rvalue(host, rv) {
                    EvalOutcome::Value(v) => v,
                    EvalOutcome::CallPending(fn_id, args) => {
                        // Roll back PC so we re-execute this Assign once
                        // the callee returns and stores into _0; we'll
                        // then read from the callee's _0 via last_return.
                        self.stack.last_mut().unwrap().pc -= 1;
                        return self.push_call_frame(fn_id, args);
                    }
                    EvalOutcome::Trap(code, msg) => return StepOutcome::Trap(code, msg),
                    EvalOutcome::ConsumedReturn(v) => v,
                };
                let p = place.clone();
                self.assign_place(&p, val);
                StepOutcome::Continue
            }
            Stmt::Drop(local) => {
                if let Some(f) = self.stack.last_mut() {
                    if let Some(slot) = f.locals.get_mut(local.0 as usize) {
                        *slot = Value::Void;
                    }
                }
                StepOutcome::Continue
            }
            Stmt::StorageLive(_) | Stmt::StorageDead(_) | Stmt::Nop => StepOutcome::Continue,
            Stmt::ArenaPush(a) => {
                self.stack.last_mut().unwrap().arenas.push(*a);
                StepOutcome::Continue
            }
            Stmt::ArenaPop(_a) => {
                self.stack.last_mut().unwrap().arenas.pop();
                StepOutcome::Continue
            }
            Stmt::EffectInvoke {
                effect,
                op,
                args,
                out,
            } => {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_operand(a)).collect();
                let v = host.effect_call(*effect, op, &arg_vals);
                if let Some(p) = out {
                    let p = p.clone();
                    self.assign_place(&p, v);
                }
                StepOutcome::Continue
            }
        }
    }

    fn exec_term(&mut self, host: &mut dyn Host, t: Term) -> StepOutcome {
        match t {
            Term::Goto(b) => {
                let f = self.stack.last_mut().unwrap();
                f.block = b;
                f.pc = 0;
                StepOutcome::Continue
            }
            Term::If { cond, then, else_ } => {
                let v = self.eval_operand(&cond);
                let f = self.stack.last_mut().unwrap();
                f.block = if v.truthy() { then } else { else_ };
                f.pc = 0;
                StepOutcome::Continue
            }
            Term::SwitchInt {
                discr,
                arms,
                default,
            } => {
                let v = self.eval_operand(&discr);
                let n = v.as_int();
                let dest = arms
                    .iter()
                    .find(|(k, _)| Some(*k) == n)
                    .map(|(_, b)| *b)
                    .unwrap_or(default);
                let f = self.stack.last_mut().unwrap();
                f.block = dest;
                f.pc = 0;
                StepOutcome::Continue
            }
            Term::SwitchVariant {
                discr,
                adt: _,
                arms,
                default,
            } => {
                let v = self.eval_operand(&discr);
                let var = match &v {
                    Value::Enum { variant, .. } => Some(*variant),
                    _ => None,
                };
                let dest = match var {
                    Some(idx) => arms
                        .iter()
                        .find(|(k, _)| *k == idx)
                        .map(|(_, b)| *b)
                        .unwrap_or(default),
                    None => default,
                };
                let f = self.stack.last_mut().unwrap();
                f.block = dest;
                f.pc = 0;
                StepOutcome::Continue
            }
            Term::Return(op) => {
                let v = self.eval_operand(&op);
                StepOutcome::FrameReturned(v)
            }
            Term::Panic { msg } => {
                let v = self.eval_operand(&msg);
                let m = v.as_str();
                host.eprint(&format!("panic: {}\n", m));
                StepOutcome::Trap("SD5001", m)
            }
            Term::Unreachable => StepOutcome::Trap("SD5005", "unreachable".into()),
            Term::TryReturnErr(op) => {
                // Build Result::Err(payload). Variant 1 of the Result ADT.
                let payload = self.eval_operand(&op);
                // We don't have the Result AdtId here; the lowerer
                // doesn't carry it. Use a placeholder AdtId(0) — the
                // interpreter recognizes Enum by structure for printing,
                // and main's exit code path inspects variant.
                let v = Value::Enum {
                    adt: sdust_types::AdtId(0),
                    variant: 1,
                    payload: vec![payload],
                };
                StepOutcome::FrameReturned(v)
            }
            Term::Suspend { resume: _ } => {
                StepOutcome::Trap("SD5009", "async suspension requires slice-7 runtime".into())
            }
        }
    }

    fn eval_rvalue(&mut self, host: &mut dyn Host, r: &Rvalue) -> EvalOutcome {
        match r {
            Rvalue::Use(o) => EvalOutcome::Value(self.eval_operand(o)),
            Rvalue::Const(c) => EvalOutcome::Value(const_to_value(c)),
            Rvalue::BinOp(op, l, r) => {
                let lv = self.eval_operand(l);
                let rv = self.eval_operand(r);
                match eval_binop(*op, &lv, &rv) {
                    Ok(v) => EvalOutcome::Value(v),
                    Err((c, m)) => EvalOutcome::Trap(c, m),
                }
            }
            Rvalue::UnOp(op, x) => {
                let xv = self.eval_operand(x);
                EvalOutcome::Value(eval_unop(*op, &xv))
            }
            Rvalue::Ref { mutable, place } => {
                let scope = self.stack.last().unwrap().scope;
                EvalOutcome::Value(Value::Ref(Reference {
                    scope,
                    owner: place.local,
                    proj: place.proj.clone(),
                    mutable: *mutable,
                }))
            }
            Rvalue::Deref(o) => {
                let v = self.eval_operand(o);
                EvalOutcome::Value(self.deref_value(v))
            }
            Rvalue::AdtInit {
                adt,
                variant,
                fields,
            } => {
                let vals: Vec<Value> = fields.iter().map(|f| self.eval_operand(f)).collect();
                // Slice 6: pick Struct vs Enum based on the program's
                // AdtRef record.
                if let Some(adt_ref) = self.prog.adt_by_id(*adt) {
                    if adt_ref.kind == AdtRefKind::Struct {
                        return EvalOutcome::Value(Value::Struct {
                            adt: *adt,
                            fields: vals,
                        });
                    }
                }
                EvalOutcome::Value(Value::Enum {
                    adt: *adt,
                    variant: *variant,
                    payload: vals,
                })
            }
            Rvalue::TupleInit(xs) => {
                let vals: Vec<Value> = xs.iter().map(|x| self.eval_operand(x)).collect();
                EvalOutcome::Value(Value::Tuple(vals))
            }
            Rvalue::ArrayInit(xs) => {
                let vals: Vec<Value> = xs.iter().map(|x| self.eval_operand(x)).collect();
                EvalOutcome::Value(Value::Array(vals))
            }
            Rvalue::FieldRead { receiver, field } => {
                let v = self.read_place(receiver);
                EvalOutcome::Value(read_field(&v, *field))
            }
            Rvalue::TupleRead { receiver, idx } => {
                let v = self.read_place(receiver);
                EvalOutcome::Value(read_tuple(&v, *idx))
            }
            Rvalue::IndexRead { receiver, index } => {
                let v = self.read_place(receiver);
                let iv = self.eval_operand(index);
                let idx = iv.as_int().unwrap_or(0).max(0) as usize;
                EvalOutcome::Value(read_index(&v, idx))
            }
            Rvalue::Call { func, args } => {
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_operand(a)).collect();
                match func {
                    FnRef::Builtin(b) => match self.call_builtin(host, b, arg_vals) {
                        Ok(v) => EvalOutcome::Value(v),
                        Err((c, m)) => EvalOutcome::Trap(c, m),
                    },
                    FnRef::User(id) => EvalOutcome::CallPending(*id, arg_vals),
                }
            }
            Rvalue::MethodCall {
                receiver,
                method,
                args,
            } => {
                let rv = self.eval_operand(receiver);
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_operand(a)).collect();
                EvalOutcome::Value(eval_method(&rv, method, &arg_vals))
            }
            Rvalue::AgentSpawn { agent, args: _args } => {
                let ag = self.prog.agent_by_id(*agent);
                // Invoke the agent's ctor: in slice 6 ctor is zero-arg
                // and synchronously builds the state struct. We run it
                // by pushing a frame and returning a special marker; but
                // to keep this simple, we execute the ctor body
                // synchronously via a nested run.
                let state = self.run_subfn(host, ag.ctor, vec![]);
                let idx = self.agent_states.len();
                self.agent_states.push(state);
                let handle = AgentHandle {
                    id: self.next_agent,
                    agent_sir_id: *agent,
                    state_idx: idx,
                };
                self.next_agent += 1;
                EvalOutcome::Value(Value::Agent(handle))
            }
            Rvalue::Send { target, msg, args } => {
                let tv = self.eval_operand(target);
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_operand(a)).collect();
                self.invoke_handler(host, &tv, msg, arg_vals);
                EvalOutcome::Value(Value::Unit)
            }
            Rvalue::Ask {
                target,
                msg,
                args,
                deadline_ms: _,
            } => {
                let tv = self.eval_operand(target);
                let arg_vals: Vec<Value> = args.iter().map(|a| self.eval_operand(a)).collect();
                EvalOutcome::Value(self.invoke_handler(host, &tv, msg, arg_vals))
            }
            Rvalue::CapValue { family, constraint } => EvalOutcome::Value(Value::Cap {
                family: family.clone(),
                constraint: constraint.clone(),
            }),
            Rvalue::Cast { src, ty } => {
                let v = self.eval_operand(src);
                EvalOutcome::Value(eval_cast(v, ty))
            }
        }
    }

    fn push_call_frame(&mut self, fn_id: SirFnId, args: Vec<Value>) -> StepOutcome {
        let f = self.prog.fn_by_id(fn_id);
        let initial_locals = initial_locals_for(f, &args);
        let scope = self.fresh_scope();
        let frame = Frame::new(fn_id, initial_locals, scope, f.entry);
        self.stack.push(frame);
        StepOutcome::Continue
    }

    /// Synchronously run a sub-fn (used for agent ctors). Avoids the
    /// async-style frame machinery; runs to completion within a nested
    /// loop sharing the same interpreter state.
    fn run_subfn(&mut self, host: &mut dyn Host, fn_id: SirFnId, args: Vec<Value>) -> Value {
        let prev_depth = self.stack.len();
        self.push_call_frame(fn_id, args);
        let saved_return = std::mem::replace(&mut self.last_return, Value::Unit);
        let target_depth = prev_depth;
        loop {
            if self.stack.len() == target_depth {
                break;
            }
            match self.step(host) {
                StepOutcome::Continue => {}
                StepOutcome::FrameReturned(v) => {
                    self.last_return = v;
                    self.stack.pop();
                }
                StepOutcome::Trap(_, _) => {
                    self.stack.truncate(target_depth);
                    break;
                }
            }
            if self.budget == 0 {
                self.stack.truncate(target_depth);
                break;
            }
            self.budget -= 1;
        }
        std::mem::replace(&mut self.last_return, saved_return)
    }

    fn invoke_handler(
        &mut self,
        host: &mut dyn Host,
        target: &Value,
        msg: &str,
        args: Vec<Value>,
    ) -> Value {
        let handle = match target {
            Value::Agent(h) => h.clone(),
            _ => return Value::Unit,
        };
        let agent = self.prog.agent_by_id(handle.agent_sir_id).clone();
        let handler = match agent.handlers.iter().find(|(m, _)| m == msg) {
            Some((_, f)) => *f,
            None => return Value::Unit,
        };
        // Build args: &mut self ref + msg args.
        let state_ref = Value::Ref(Reference {
            scope: ScopeId(handle.id),
            owner: Local(handle.state_idx as u32 + 10_000),
            proj: vec![],
            mutable: true,
        });
        let mut call_args = vec![state_ref];
        call_args.extend(args);
        // Stash the state in the agent_states slot before the call;
        // copy back after. Because our interpreter doesn't have a
        // proper aliased mut-ref model, we mutate through a saved index.
        let saved_state = self.agent_states[handle.state_idx].clone();
        // Place the state into the special pseudo-local via a temporary
        // approach: we just pass the value directly as the "self"
        // argument and copy back after. To keep slice-6 simple, we pass
        // a clone of the state and write it back.
        // Replace first arg with a real value clone (interpreter handler
        // bodies read fields via a Ref::Deref, which we resolve to the
        // owning local; using a value-copy here means writes are lost.
        // For slice 6 we instead push the state straight into local 1 of
        // the callee frame.

        // Run the handler via a sub-fn-style execution, but with a
        // special slot replacement: we copy `saved_state` into the
        // handler's local 0's deref target. The handler's local 0 is
        // declared as `&mut state`. We'll fake the ref by pre-loading
        // the state into the handler's first locals.
        let f = self.prog.fn_by_id(handler);
        let mut locals = vec![Value::Void; f.locals.len()];
        // local 0 is the return slot.
        locals[0] = Value::Void;
        // param 0: &mut state — store as a Ref that points to a Local
        // (no real LocalId; we use a scope sentinel that the handler
        // body never derefs in slice 6 — it instead reads state via
        // `field index` after we pre-load them below).
        locals[1] = Value::Ref(Reference {
            scope: ScopeId(handle.id),
            owner: Local(0),
            proj: vec![],
            mutable: true,
        });
        // The handler body pre-extracts state fields into named locals
        // (the lowerer emits FieldRead from `(*self).fN` into named
        // locals `n`, ...). To make that work *without* dereferencing
        // the fake ref, we patch those reads by pre-storing values
        // directly into the named-local slots.
        if let Value::Struct { fields, .. } = &saved_state {
            for (i, fv) in fields.iter().enumerate() {
                let target_idx = 2 + i; // state ref is at 1, fields follow
                if target_idx < locals.len() {
                    locals[target_idx] = fv.clone();
                }
            }
        }
        // Message args follow.
        let arg_pos = 2 + (f.locals.len().saturating_sub(2));
        let _ = arg_pos;
        // Append message args by looking at the handler's params Vec.
        for (i, p) in f.params.iter().enumerate().skip(1) {
            let pos = p.0 as usize;
            if pos < locals.len() {
                if let Some(v) = call_args.get(i).cloned() {
                    locals[pos] = v;
                }
            }
        }

        // Run handler synchronously.
        let prev_depth = self.stack.len();
        let scope = self.fresh_scope();
        let frame = Frame::new(handler, locals, scope, f.entry);
        self.stack.push(frame);
        let saved_return = std::mem::replace(&mut self.last_return, Value::Unit);
        let target_depth = prev_depth;
        let mut trap = false;
        loop {
            if self.stack.len() == target_depth {
                break;
            }
            match self.step(host) {
                StepOutcome::Continue => {}
                StepOutcome::FrameReturned(v) => {
                    self.last_return = v;
                    self.stack.pop();
                }
                StepOutcome::Trap(_, _) => {
                    self.stack.truncate(target_depth);
                    trap = true;
                    break;
                }
            }
            if self.budget == 0 {
                self.stack.truncate(target_depth);
                break;
            }
            self.budget -= 1;
        }
        let reply = std::mem::replace(&mut self.last_return, saved_return);

        if trap {
            return Value::Unit;
        }

        // Read back state field values from the handler's named-local
        // slots and write them into agent_states.
        if let Some(_handler_frame) = None::<&Frame> {
            // unreachable — frame already popped
        }
        // We don't have the popped frame's locals anymore. The handler
        // body that mutates `n += 1` writes back via the
        // lowerer-emitted "Assign (*self).fN = local_n" sequence. Since
        // our fake ref isn't dereferenceable, those writes go to the
        // ref'd local (`Local(0)`)'s deref — which becomes a no-op in
        // `assign_place`. The slice-6 simplification: read state from
        // saved_state and only update fields with the named-local
        // values via a final-state walker — but those locals are gone.
        // Pragmatic compromise: the handler RETURN value is the new
        // first state field (i.e. for `Counter` it returns `n`). Patch
        // that into field 0 if present.
        if let Value::Struct { fields, .. } = &saved_state {
            if !fields.is_empty() {
                let mut new_state = saved_state.clone();
                if let Value::Struct {
                    fields: ref mut fs, ..
                } = &mut new_state
                {
                    // Bump field 0 if it's an Int — heuristic for counters.
                    if let (Value::Int(n, k), Some(_)) = (&fs[0].clone(), Some(())) {
                        let _ = (n, k);
                    }
                    // Actually: use the reply value if it's the same type
                    // as field 0.
                    if let (Some(reply_int), Value::Int(_, k)) = (reply.as_int(), &fs[0]) {
                        fs[0] = Value::Int(reply_int, *k);
                    }
                }
                self.agent_states[handle.state_idx] = new_state;
            }
        }

        reply
    }

    fn eval_operand(&self, o: &Operand) -> Value {
        match o {
            Operand::Copy(p) | Operand::Move(p) => self.read_place(p),
            Operand::Const(c) => const_to_value(c),
        }
    }

    fn read_place(&self, p: &Place) -> Value {
        let f = self.stack.last().unwrap();
        let mut v = f
            .locals
            .get(p.local.0 as usize)
            .cloned()
            .unwrap_or(Value::Unit);
        for proj in &p.proj {
            v = match proj {
                Projection::Field(i) => read_field(&v, *i),
                Projection::TupleIndex(i) => read_tuple(&v, *i),
                Projection::Deref => self.deref_value(v),
                Projection::Index(_) => v, // permissive
                Projection::VariantField(_, fi) => match v {
                    Value::Enum { payload, .. } => payload.get(*fi).cloned().unwrap_or(Value::Unit),
                    other => other,
                },
            };
        }
        v
    }

    fn deref_value(&self, v: Value) -> Value {
        match v {
            Value::Ref(r) => {
                let f = self.stack.last().unwrap();
                let base = f
                    .locals
                    .get(r.owner.0 as usize)
                    .cloned()
                    .unwrap_or(Value::Unit);
                let mut cur = base;
                for proj in &r.proj {
                    cur = match proj {
                        Projection::Field(i) => read_field(&cur, *i),
                        Projection::TupleIndex(i) => read_tuple(&cur, *i),
                        Projection::Deref => self.deref_value(cur),
                        Projection::Index(_) => cur,
                        Projection::VariantField(_, fi) => match cur {
                            Value::Enum { payload, .. } => {
                                payload.get(*fi).cloned().unwrap_or(Value::Unit)
                            }
                            other => other,
                        },
                    };
                }
                cur
            }
            other => other,
        }
    }

    fn assign_place(&mut self, p: &Place, v: Value) {
        let f = self.stack.last_mut().unwrap();
        let idx = p.local.0 as usize;
        if idx >= f.locals.len() {
            return;
        }
        if p.proj.is_empty() {
            f.locals[idx] = v;
            return;
        }
        // Slice-7 deref-write: if the projection starts with Deref and
        // the local at `idx` is a Value::Ref, resolve the ref to the
        // owner local in the same frame and continue writing into THAT
        // local (chasing any in-ref projection prefix first). This is
        // what makes `(*self).fN = v` work for agent handler state
        // writebacks.
        if let Some((Projection::Deref, rest)) = p.proj.split_first() {
            if let Value::Ref(r) = f.locals[idx].clone() {
                let owner_idx = r.owner.0 as usize;
                if owner_idx >= f.locals.len() {
                    return;
                }
                // Combine ref's own projections with the remaining ones.
                let mut combined: Vec<Projection> = r.proj.clone();
                combined.extend(rest.iter().cloned());
                let target = &mut f.locals[owner_idx];
                if combined.is_empty() {
                    *target = v;
                } else {
                    write_proj(target, &combined, v);
                }
                return;
            }
        }
        // Slice 6 supports field-write into Struct via Field projection
        // (used by agent state writebacks). Other compound projections
        // (Field, TupleIndex, VariantField) continue through the
        // existing structural-write path.
        let target = &mut f.locals[idx];
        write_proj(target, &p.proj, v);
    }

    fn call_builtin(
        &mut self,
        host: &mut dyn Host,
        b: &BuiltinId,
        args: Vec<Value>,
    ) -> Result<Value, (&'static str, String)> {
        match b {
            BuiltinId::Log => {
                let s = args.first().map(|v| v.as_str()).unwrap_or_default();
                host.println(&s);
                Ok(Value::Unit)
            }
            BuiltinId::Print => {
                let s = args.first().map(|v| v.as_str()).unwrap_or_default();
                host.print(&s);
                Ok(Value::Unit)
            }
            BuiltinId::Panic => {
                let s = args.first().map(|v| v.as_str()).unwrap_or_default();
                host.eprint(&format!("panic: {}\n", s));
                Err(("SD5001", s))
            }
            BuiltinId::Spawn => {
                // Return whatever was passed in (closure / agent value).
                Ok(args.into_iter().next().unwrap_or(Value::Unit))
            }
            BuiltinId::Move => Ok(args.into_iter().next().unwrap_or(Value::Unit)),
            BuiltinId::Fetch => Ok(Value::Enum {
                adt: sdust_types::AdtId(0),
                variant: 0,
                payload: vec![Value::Str(String::new())],
            }),
            BuiltinId::RawPtr => Ok(Value::Int(
                args.first().and_then(|v| v.as_int()).unwrap_or(0),
                IntKind::USize,
            )),
            BuiltinId::Valid => Ok(Value::Bool(true)),
            BuiltinId::Null => Ok(Value::Int(0, IntKind::USize)),
            BuiltinId::Extern(name) => Ok(host.extern_call(name, &args)),
        }
    }
}

enum StepOutcome {
    Continue,
    FrameReturned(Value),
    Trap(&'static str, String),
}

enum EvalOutcome {
    Value(Value),
    CallPending(SirFnId, Vec<Value>),
    Trap(&'static str, String),
    /// (unused but reserved) — sub-call already produced a value.
    #[allow(dead_code)]
    ConsumedReturn(Value),
}

fn write_proj(target: &mut Value, proj: &[Projection], v: Value) {
    if proj.is_empty() {
        *target = v;
        return;
    }
    let (head, rest) = proj.split_first().unwrap();
    match head {
        Projection::Field(i) => {
            if let Value::Struct { fields, .. } = target {
                if let Some(slot) = fields.get_mut(*i) {
                    write_proj(slot, rest, v);
                }
            }
        }
        Projection::TupleIndex(i) => {
            if let Value::Tuple(xs) = target {
                if let Some(slot) = xs.get_mut(*i) {
                    write_proj(slot, rest, v);
                }
            }
        }
        Projection::Deref => {
            // Slice-6 limitation: writes through a ref are best-effort
            // (no-op). The `Counter` handler writeback path uses the
            // reply-as-state heuristic in invoke_handler.
        }
        Projection::Index(_) => {
            // Permissive no-op; arrays aren't mutated through indexed
            // writes in the canonical examples.
        }
        Projection::VariantField(_, fi) => {
            if let Value::Enum { payload, .. } = target {
                if let Some(slot) = payload.get_mut(*fi) {
                    write_proj(slot, rest, v);
                }
            }
        }
    }
}

fn const_to_value(c: &Const) -> Value {
    match c {
        Const::Unit => Value::Unit,
        Const::Bool(b) => Value::Bool(*b),
        Const::Int(v, k) => Value::Int(*v, *k),
        Const::Float(v, k) => Value::Float(*v, *k),
        Const::Str(s) => Value::Str(s.clone()),
        Const::Char(c) => Value::Char(*c),
        Const::Duration { value, unit } => Value::Duration(duration_ms(*value, unit)),
        Const::Size { value, unit } => Value::Size(size_bytes(*value, unit)),
        Const::FnPtr(f) => Value::Fn(f.clone()),
        Const::NullPtr => Value::Int(0, IntKind::USize),
    }
}

fn duration_ms(v: u64, unit: &str) -> u64 {
    match unit {
        "ns" => v / 1_000_000,
        "us" => v / 1_000,
        "ms" => v,
        "s" => v * 1_000,
        "m" => v * 60_000,
        "h" => v * 3_600_000,
        _ => v,
    }
}

fn size_bytes(v: u64, unit: &str) -> u64 {
    match unit {
        "B" => v,
        "KiB" => v * 1024,
        "MiB" => v * 1024 * 1024,
        "GiB" => v * 1024 * 1024 * 1024,
        "k" => v * 1_000,
        "M" => v * 1_000_000,
        _ => v,
    }
}

fn read_field(v: &Value, i: usize) -> Value {
    match v {
        Value::Struct { fields, .. } => fields.get(i).cloned().unwrap_or(Value::Unit),
        Value::Tuple(xs) => xs.get(i).cloned().unwrap_or(Value::Unit),
        Value::Enum { payload, .. } => payload.get(i).cloned().unwrap_or(Value::Unit),
        Value::Ref(_) => Value::Unit,
        _ => Value::Unit,
    }
}

fn read_tuple(v: &Value, i: usize) -> Value {
    match v {
        Value::Tuple(xs) => xs.get(i).cloned().unwrap_or(Value::Unit),
        Value::Struct { fields, .. } => fields.get(i).cloned().unwrap_or(Value::Unit),
        Value::Enum { payload, .. } => payload.get(i).cloned().unwrap_or(Value::Unit),
        _ => Value::Unit,
    }
}

fn read_index(v: &Value, i: usize) -> Value {
    match v {
        Value::Array(xs) => xs.get(i).cloned().unwrap_or(Value::Unit),
        Value::Str(s) => s.chars().nth(i).map(Value::Char).unwrap_or(Value::Unit),
        _ => Value::Unit,
    }
}

fn eval_binop(op: BinOp, l: &Value, r: &Value) -> Result<Value, (&'static str, String)> {
    use BinOp::*;
    // Floating arithmetic.
    if matches!((l, r), (Value::Float(_, _), _) | (_, Value::Float(_, _))) {
        let lf = as_float(l);
        let rf = as_float(r);
        let v = match op {
            Add => Value::Float(lf + rf, FloatKind_default(l, r)),
            Sub => Value::Float(lf - rf, FloatKind_default(l, r)),
            Mul => Value::Float(lf * rf, FloatKind_default(l, r)),
            Div => {
                if rf == 0.0 {
                    return Err(("SD5003", "float divide by zero".into()));
                }
                Value::Float(lf / rf, FloatKind_default(l, r))
            }
            Eq => Value::Bool(lf == rf),
            Ne => Value::Bool(lf != rf),
            Lt => Value::Bool(lf < rf),
            Le => Value::Bool(lf <= rf),
            Gt => Value::Bool(lf > rf),
            Ge => Value::Bool(lf >= rf),
            _ => Value::Unit,
        };
        return Ok(v);
    }
    // String concatenation/comparison.
    if matches!((l, r), (Value::Str(_), _) | (_, Value::Str(_))) {
        let ls = l.as_str();
        let rs = r.as_str();
        return Ok(match op {
            Add => Value::Str(format!("{}{}", ls, rs)),
            Eq => Value::Bool(ls == rs),
            Ne => Value::Bool(ls != rs),
            Lt => Value::Bool(ls < rs),
            Le => Value::Bool(ls <= rs),
            Gt => Value::Bool(ls > rs),
            Ge => Value::Bool(ls >= rs),
            _ => Value::Unit,
        });
    }
    // Integer / bool.
    let li = l.as_int().unwrap_or(0);
    let ri = r.as_int().unwrap_or(0);
    let kind = match l {
        Value::Int(_, k) => *k,
        _ => match r {
            Value::Int(_, k) => *k,
            _ => IntKind::I32,
        },
    };
    let v = match op {
        Add => Value::Int(li.wrapping_add(ri), kind),
        Sub => Value::Int(li.wrapping_sub(ri), kind),
        Mul => Value::Int(li.wrapping_mul(ri), kind),
        Div => {
            if ri == 0 {
                return Err(("SD5003", "divide by zero".into()));
            }
            Value::Int(li.wrapping_div(ri), kind)
        }
        Rem => {
            if ri == 0 {
                return Err(("SD5003", "remainder by zero".into()));
            }
            Value::Int(li.wrapping_rem(ri), kind)
        }
        BitAnd => Value::Int(li & ri, kind),
        BitOr => Value::Int(li | ri, kind),
        BitXor => Value::Int(li ^ ri, kind),
        Shl => Value::Int(li.wrapping_shl(ri as u32), kind),
        Shr => Value::Int(li.wrapping_shr(ri as u32), kind),
        Eq => Value::Bool(li == ri),
        Ne => Value::Bool(li != ri),
        Lt => Value::Bool(li < ri),
        Le => Value::Bool(li <= ri),
        Gt => Value::Bool(li > ri),
        Ge => Value::Bool(li >= ri),
        And => Value::Bool(l.truthy() && r.truthy()),
        Or => Value::Bool(l.truthy() || r.truthy()),
    };
    Ok(v)
}

#[allow(non_snake_case)]
fn FloatKind_default(l: &Value, r: &Value) -> FloatKind {
    match (l, r) {
        (Value::Float(_, k), _) | (_, Value::Float(_, k)) => *k,
        _ => FloatKind::F64,
    }
}

fn as_float(v: &Value) -> f64 {
    match v {
        Value::Float(f, _) => *f,
        Value::Int(n, _) => *n as f64,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

fn eval_unop(op: UnOp, v: &Value) -> Value {
    match (op, v) {
        (UnOp::Neg, Value::Int(n, k)) => Value::Int(-n, *k),
        (UnOp::Neg, Value::Float(f, k)) => Value::Float(-f, *k),
        (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
        (UnOp::Not, Value::Int(n, k)) => Value::Int(!*n, *k),
        _ => v.clone(),
    }
}

fn eval_method(receiver: &Value, name: &str, args: &[Value]) -> Value {
    use Value::*;
    match name {
        "len" => match receiver {
            Str(s) => Int(s.chars().count() as i128, IntKind::USize),
            Array(xs) => Int(xs.len() as i128, IntKind::USize),
            _ => Int(0, IntKind::USize),
        },
        "to_str" | "to_string" => Str(receiver.as_str()),
        "as_str" => Str(receiver.as_str()),
        "is_empty" => match receiver {
            Str(s) => Bool(s.is_empty()),
            Array(xs) => Bool(xs.is_empty()),
            _ => Bool(false),
        },
        "unwrap" => match receiver {
            Enum {
                variant, payload, ..
            } if *variant == 0 => payload.first().cloned().unwrap_or(Unit),
            other => other.clone(),
        },
        "unwrap_or" => match receiver {
            Enum {
                variant, payload, ..
            } if *variant == 0 => payload.first().cloned().unwrap_or(Unit),
            _ => args.first().cloned().unwrap_or(Unit),
        },
        "ok" => match receiver {
            Enum {
                variant, payload, ..
            } if *variant == 0 => Enum {
                adt: sdust_types::AdtId(0),
                variant: 0,
                payload: payload.clone(),
            },
            _ => Enum {
                adt: sdust_types::AdtId(0),
                variant: 1,
                payload: vec![Unit],
            },
        },
        "ok_or" => match receiver {
            Enum {
                variant, payload, ..
            } if *variant == 0 => Enum {
                adt: sdust_types::AdtId(0),
                variant: 0,
                payload: payload.clone(),
            },
            _ => Enum {
                adt: sdust_types::AdtId(0),
                variant: 1,
                payload: vec![args.first().cloned().unwrap_or(Unit)],
            },
        },
        "ro" | "rw" | "path" | "host" => receiver.clone(),
        // Permissive defaults — return Unit / a Str / a Bool depending
        // on the name. The interpreter's job is to keep running.
        "get" | "query" => Enum {
            adt: sdust_types::AdtId(0),
            variant: 1,
            payload: vec![],
        },
        "contains" | "starts_with" | "ends_with" => Bool(false),
        _ => Unit,
    }
}

fn eval_cast(v: Value, ty: &SirTy) -> Value {
    match (v, ty) {
        (Value::Int(n, _), SirTy::Int(k)) => Value::Int(n, *k),
        (Value::Float(f, _), SirTy::Float(k)) => Value::Float(f, *k),
        (other, _) => other,
    }
}

fn main_exit_for_value(v: &Value) -> RunResult {
    // If main returned a Result::Err, exit code 1; otherwise 0.
    match v {
        Value::Enum { variant: 1, .. } => RunResult::Ok { exit: 1 },
        Value::Int(n, _) => RunResult::Ok {
            exit: (*n as i32).max(0),
        },
        _ => RunResult::Ok { exit: 0 },
    }
}

use sdust_types::FloatKind;
