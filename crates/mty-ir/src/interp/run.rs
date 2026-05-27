//! Interpreter core: drive a `Program` through its `main` fn.

use super::host::Host;
use super::value::*;
use crate::ir::*;
use mty_types::IntKind;

/// Default step budget — each stmt + each terminator counts as one step.
pub const DEFAULT_STEP_BUDGET: u64 = 1_000_000;

/// Default per-step CPU budget translated into "instructions". The
/// runtime's `BudgetTracker::cpu` is a `Duration`; the SIR interpreter
/// translates that into a step count via `cpu_ns / DEFAULT_STEP_NS` (see
/// `crate::interp::value::Frame`). Surfaced here as a constant so tests
/// can pin the trip point deterministically.
pub const DEFAULT_STEP_NS: u64 = 1_000;

/// Default memory ceiling (bytes) when no caller-provided cap is set.
/// `0` means "unlimited" — Gap-4 charging only trips when a finite
/// `mem_budget` is supplied via [`run_fn_with_resource_budget`].
pub const DEFAULT_MEM_BUDGET: u64 = 0;

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
    /// Memory budget exceeded (Gap-4 v0.5).
    MemBudgetExceeded { used: u64, limit: u64 },
}

impl RunResult {
    pub fn exit_code(&self) -> i32 {
        match self {
            RunResult::Ok { exit } => *exit,
            RunResult::Trap { .. } => 1,
            RunResult::NoMain => 2,
            RunResult::BudgetExceeded => 3,
            RunResult::MemBudgetExceeded { .. } => 4,
        }
    }
}

/// Run `prog` starting at the fn named `main`. The host receives all
/// output. Returns a `RunResult`.
pub fn run(prog: &Program, host: &mut dyn Host) -> RunResult {
    let Some(mainf) = prog.fn_by_name("main") else {
        return RunResult::NoMain;
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
    let Some(f) = prog.fn_by_name(name) else {
        return Err(RunResult::NoMain);
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
    let Some(f) = prog.fn_by_name(name) else {
        return Err(RunResult::NoMain);
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

/// v0.5 dogfood Gap-4: run with a paired step + memory budget. Returns
/// [`RunResult::MemBudgetExceeded`] on alloc-over-cap and
/// [`RunResult::BudgetExceeded`] on step-over-cap. `mem_budget == 0`
/// means "no memory cap" (same as [`run_fn_with_budget`]).
pub fn run_fn_with_resource_budget(
    prog: &Program,
    name: &str,
    args: Vec<Value>,
    host: &mut dyn Host,
    step_budget: u64,
    mem_budget: u64,
) -> Result<Value, RunResult> {
    let Some(f) = prog.fn_by_name(name) else {
        return Err(RunResult::NoMain);
    };
    let mut interp = Interp::new(prog, step_budget);
    interp.mem_budget = mem_budget;
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
    handler: IrFnId,
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

    // The lowerer (see crates/mty-ir/src/lower/items.rs) emits, at
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
    /// v0.5 Gap-4: cumulative bytes of "allocation-shape" rvalues
    /// (Struct / Tuple / Array / Str literal payload) charged so far.
    mem_used: u64,
    /// v0.5 Gap-4: ceiling for `mem_used`. `0` = unlimited.
    mem_budget: u64,
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
            mem_used: 0,
            mem_budget: 0,
            last_frame_locals: None,
        }
    }

    /// Charge `bytes` against the memory budget. Returns Err with the
    /// over-cap message when the call would exceed `mem_budget` (no-op
    /// when `mem_budget == 0`).
    fn charge_mem(&mut self, bytes: u64) -> Result<(), (&'static str, String)> {
        let new = self.mem_used.saturating_add(bytes);
        self.mem_used = new;
        if self.mem_budget > 0 && new > self.mem_budget {
            return Err((
                "MT5009",
                format!(
                    "memory budget exceeded: used {} B > cap {} B",
                    new, self.mem_budget
                ),
            ));
        }
        Ok(())
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
            // v0.5 Gap-4: surface MemBudgetExceeded between steps so
            // a charge inside `eval_rvalue` propagates as a typed
            // RunResult (instead of being smuggled through the trap
            // channel — which keeps the MT5009 trap message intact).
            if self.mem_budget > 0 && self.mem_used > self.mem_budget {
                return RunResult::MemBudgetExceeded {
                    used: self.mem_used,
                    limit: self.mem_budget,
                };
            }
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
                        // Run the callee synchronously via the same
                        // nested-loop path that agent ctors use. This
                        // avoids the broken "roll back PC and rely on
                        // last_return" protocol which infinitely
                        // re-fired the same Call statement.
                        self.run_subfn(host, fn_id, args)
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
                StepOutcome::Trap("MT5001", m)
            }
            Term::Unreachable => StepOutcome::Trap("MT5005", "unreachable".into()),
            Term::TryReturnErr(op) => {
                // Build Result::Err(payload). Variant 1 of the Result ADT.
                let payload = self.eval_operand(&op);
                // We don't have the Result AdtId here; the lowerer
                // doesn't carry it. Use a placeholder AdtId(0) — the
                // interpreter recognizes Enum by structure for printing,
                // and main's exit code path inspects variant.
                let v = Value::Enum {
                    adt: mty_types::AdtId(0),
                    variant: 1,
                    payload: vec![payload],
                };
                StepOutcome::FrameReturned(v)
            }
            Term::Suspend { resume: _ } => {
                StepOutcome::Trap("MT5009", "async suspension requires slice-7 runtime".into())
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
                // v0.5 Gap-4: charge memory roughly proportional to the
                // value footprint. We treat each scalar field as 16 B
                // (matches `size_of::<Value>` on 64-bit) plus a 24 B
                // header for the Struct/Enum wrapper itself.
                let bytes = 24 + estimate_payload_bytes(&vals);
                if let Err((c, m)) = self.charge_mem(bytes) {
                    return EvalOutcome::Trap(c, m);
                }
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
                let bytes = 16 + estimate_payload_bytes(&vals);
                if let Err((c, m)) = self.charge_mem(bytes) {
                    return EvalOutcome::Trap(c, m);
                }
                EvalOutcome::Value(Value::Tuple(vals))
            }
            Rvalue::ArrayInit(xs) => {
                let vals: Vec<Value> = xs.iter().map(|x| self.eval_operand(x)).collect();
                let bytes = 24 + estimate_payload_bytes(&vals);
                if let Err((c, m)) = self.charge_mem(bytes) {
                    return EvalOutcome::Trap(c, m);
                }
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

    fn push_call_frame(&mut self, fn_id: IrFnId, args: Vec<Value>) -> StepOutcome {
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
    fn run_subfn(&mut self, host: &mut dyn Host, fn_id: IrFnId, args: Vec<Value>) -> Value {
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

        // Slice-7 path: delegate to run_handler_isolated. It creates a
        // throwaway interpreter with a properly-aliased self-ref so
        // (*self).fN writes go through. We snapshot the state, run, and
        // write the new state back.
        let state_in = self.agent_states[handle.state_idx].clone();
        let (rr, new_state, reply) = run_handler_isolated(self.prog, handler, state_in, args, host);
        if matches!(rr, RunResult::Ok { .. }) {
            self.agent_states[handle.state_idx] = new_state;
        }
        return reply;

        // ---- slice-6 legacy path retained below for reference ----
        // (unreachable; left in place as documentation for the
        // historical state-passing hack until slice-8 cleanup.)
        #[allow(unreachable_code)]
        {
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
                Err(("MT5001", s))
            }
            BuiltinId::Spawn => {
                // Return whatever was passed in (closure / agent value).
                Ok(args.into_iter().next().unwrap_or(Value::Unit))
            }
            BuiltinId::Move => Ok(args.into_iter().next().unwrap_or(Value::Unit)),
            BuiltinId::Fetch => Ok(Value::Enum {
                adt: mty_types::AdtId(0),
                variant: 0,
                payload: vec![Value::Str(String::new())],
            }),
            BuiltinId::RawPtr => Ok(Value::Int(
                args.first().and_then(|v| v.as_int()).unwrap_or(0),
                IntKind::USize,
            )),
            BuiltinId::Valid => Ok(Value::Bool(true)),
            BuiltinId::Null => Ok(Value::Int(0, IntKind::USize)),
            BuiltinId::Extern(name) => {
                // v0.25 Track E: receiver-less stdlib constructors
                // (`String.new`, `Vec.with_capacity`, ...) lower
                // through the path-call fall-through as
                // `BuiltinId::Extern("Type.ctor")`. The host doesn't
                // know about them, so we synthesise the value here
                // before delegating.
                if let Some(v) = try_stdlib_ctor(name, &args) {
                    Ok(v)
                } else {
                    Ok(host.extern_call(name, &args))
                }
            }
            BuiltinId::DomOp(op) => {
                // v0.6 — Dom builtin calls go through the host's extern
                // table as `dom.<op>` so headless test runs (without a
                // wasm32-web JS host) get a deterministic default
                // (typically `Value::Unit`). Real DOM dispatch is the
                // wasm32-web backend's job — see
                // `emit_call` → `emit_dom_call` in mty-codegen-wasm.
                let qualified = format!("dom.{op}");
                Ok(host.extern_call(&qualified, &args))
            }
            BuiltinId::CanvasOp(op) => {
                // v0.24 — Canvas builtin calls mirror the v0.6 DomOp
                // pattern. The interpreter routes through the host's
                // extern table as `canvas.<snake_name>` so headless
                // test runs without a wasm32-web JS host get a
                // deterministic default. Real Canvas dispatch is the
                // wasm32-web backend's job — see `emit_canvas_call`
                // in `crates/mty-codegen-wasm/src/web_lower.rs`.
                let qualified = format!("canvas.{}", op.as_snake());
                Ok(host.extern_call(&qualified, &args))
            }
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
    CallPending(IrFnId, Vec<Value>),
    Trap(&'static str, String),
    /// (unused but reserved) — sub-call already produced a value.
    #[allow(dead_code)]
    ConsumedReturn(Value),
}

/// v0.25 Track E: recognise the `Type.ctor()` shapes that lower as
/// `BuiltinId::Extern("Type.ctor")` (the fall-through path in
/// `lower::exprs::resolve_callee`) and synthesise the matching value.
///
/// Returning `None` lets the call fall through to the host's extern
/// table — that keeps `extern fn _foo(...)` declarations working.
fn try_stdlib_ctor(name: &str, args: &[Value]) -> Option<Value> {
    use Value::*;
    let usize_arg = |i: usize| -> usize {
        args.get(i)
            .and_then(|v| v.as_int())
            .map(|n| n.max(0) as usize)
            .unwrap_or(0)
    };
    let str_arg = |i: usize| -> Option<String> {
        match args.get(i) {
            Some(Str(s)) => Some(s.clone()),
            _ => None,
        }
    };
    let ok = |v: Value| Value::Enum {
        adt: mty_types::AdtId(0),
        variant: 0,
        payload: vec![v],
    };
    let err = || Value::Enum {
        adt: mty_types::AdtId(0),
        variant: 1,
        payload: vec![Unit],
    };
    match name {
        // ---- String constructors ----
        "String.new" => Some(Str(String::new())),
        "String.with_capacity" => Some(Str(String::with_capacity(usize_arg(0)))),
        "String.from_str" => Some(Str(str_arg(0).unwrap_or_default())),
        "String.from_utf8" => match args.first() {
            Some(Str(s)) => match std::str::from_utf8(s.as_bytes()) {
                Ok(_) => Some(ok(Str(s.clone()))),
                Err(_) => Some(err()),
            },
            Some(Array(xs)) => {
                let bytes: Vec<u8> = xs
                    .iter()
                    .filter_map(|v| match v {
                        Int(n, _) => Some(*n as u8),
                        _ => None,
                    })
                    .collect();
                match String::from_utf8(bytes) {
                    Ok(s) => Some(ok(Str(s))),
                    Err(_) => Some(err()),
                }
            }
            _ => Some(err()),
        },
        // ---- Vec[T] constructors ----
        "Vec.new" => Some(Array(Vec::new())),
        "Vec.with_capacity" => Some(Array(Vec::with_capacity(usize_arg(0)))),
        _ => None,
    }
}

/// v0.5 Gap-4 helper: rough byte size of a [`Value`] for memory
/// budget charging. Numbers are deliberately approximate — the goal
/// is "deterministic, monotonic charging that catches runaway
/// allocations", not bit-perfect accounting.
fn estimate_payload_bytes(vs: &[Value]) -> u64 {
    let mut total: u64 = 0;
    for v in vs {
        total = total.saturating_add(estimate_value_bytes(v));
    }
    total
}

fn estimate_value_bytes(v: &Value) -> u64 {
    match v {
        Value::Unit | Value::Void => 8,
        Value::Bool(_) | Value::Char(_) => 8,
        Value::Int(_, _) | Value::Float(_, _) | Value::Duration(_) | Value::Size(_) => 16,
        Value::Str(s) => 24 + s.len() as u64,
        Value::Tuple(xs) | Value::Array(xs) => {
            24 + xs.iter().map(estimate_value_bytes).sum::<u64>()
        }
        Value::Struct { fields, .. } => 24 + fields.iter().map(estimate_value_bytes).sum::<u64>(),
        Value::Enum { payload, .. } => 24 + payload.iter().map(estimate_value_bytes).sum::<u64>(),
        Value::Ref(_) | Value::Fn(_) | Value::Agent(_) | Value::Cap { .. } => 16,
    }
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
                    return Err(("MT5003", "float divide by zero".into()));
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
                return Err(("MT5003", "divide by zero".into()));
            }
            Value::Int(li.wrapping_div(ri), kind)
        }
        Rem => {
            if ri == 0 {
                return Err(("MT5003", "remainder by zero".into()));
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
        Value::Bool(true) => 1.0,
        Value::Bool(false) => 0.0,
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
    // --- helpers that pull a borrowed &str out of receiver / args ---
    fn arg_str(args: &[Value], i: usize) -> Option<String> {
        match args.get(i)? {
            Str(s) => Some(s.clone()),
            Char(c) => Some(c.to_string()),
            other => Some(other.as_str()),
        }
    }
    fn arg_usize(args: &[Value], i: usize) -> Option<usize> {
        args.get(i)
            .and_then(|v| v.as_int())
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
    }
    fn some(v: Value) -> Value {
        Enum {
            adt: mty_types::AdtId(0),
            variant: 0,
            payload: vec![v],
        }
    }
    fn none() -> Value {
        Enum {
            adt: mty_types::AdtId(0),
            variant: 1,
            payload: vec![],
        }
    }

    match name {
        // ---------------- v0.5 iterator protocol ----------------
        //
        // Caller (the SIR `for`-loop lowering) passes the current index
        // in `args[0]`. Returns `(exhausted: Bool, element: Value)`.
        // The lowering bumps the index between calls and tests field 0
        // to decide whether to enter the body or exit. Supported
        // iterables:
        //   * `Tuple(lo, hi, inclusive_marker)` from range lowering
        //     (`lo..hi` => exclusive, `lo..=hi` => inclusive)
        //   * `Array(...)` for slice/Vec iteration
        // Anything else returns `(true, Unit)` so the loop exits
        // immediately rather than spinning forever.
        "__mty_iter_next" => {
            let idx = args.first().and_then(|v| v.as_int()).unwrap_or(0);
            match receiver {
                Tuple(parts) if parts.len() == 3 => {
                    let lo = parts[0].as_int().unwrap_or(0);
                    let hi = parts[1].as_int().unwrap_or(0);
                    let inclusive = matches!(&parts[2], Bool(true));
                    let cur = lo + idx;
                    let exhausted = if inclusive { cur > hi } else { cur >= hi };
                    let kind = match &parts[0] {
                        Int(_, k) => *k,
                        _ => IntKind::I32,
                    };
                    Tuple(vec![
                        Bool(exhausted),
                        if exhausted { Unit } else { Int(cur, kind) },
                    ])
                }
                Array(xs) => {
                    let i = idx.max(0) as usize;
                    if i >= xs.len() {
                        Tuple(vec![Bool(true), Unit])
                    } else {
                        Tuple(vec![Bool(false), xs[i].clone()])
                    }
                }
                _ => Tuple(vec![Bool(true), Unit]),
            }
        }

        // ---------------- length / emptiness ----------------
        "len" => match receiver {
            Str(s) => Int(s.chars().count() as i128, IntKind::USize),
            Array(xs) => Int(xs.len() as i128, IntKind::USize),
            _ => Int(0, IntKind::USize),
        },
        "to_str" | "to_string" => Str(receiver.as_str()),
        "as_str" => Str(receiver.as_str()),
        // v0.24 (Track B): conversion methods the `format!` builtin
        // expands its placeholders to. Integers / chars / bools / floats
        // get sensible formatting; everything else falls through to the
        // generic `as_str` so the runtime never traps on a missing impl.
        "to_hex_str" => match receiver {
            Int(n, _) => {
                let v = *n as i64;
                if v < 0 {
                    Str(format!("-{:x}", v.unsigned_abs()))
                } else {
                    Str(format!("{:x}", v as u64))
                }
            }
            Char(c) => Str(format!("{:x}", *c as u32)),
            Bool(b) => Str(format!("{:x}", if *b { 1 } else { 0 })),
            Str(s) => {
                let mut out = String::with_capacity(s.len() * 2);
                for byte in s.as_bytes() {
                    out.push_str(&format!("{:02x}", byte));
                }
                Str(out)
            }
            other => Str(other.as_str()),
        },
        "to_hex_upper_str" => match receiver {
            Int(n, _) => {
                let v = *n as i64;
                if v < 0 {
                    Str(format!("-{:X}", v.unsigned_abs()))
                } else {
                    Str(format!("{:X}", v as u64))
                }
            }
            Char(c) => Str(format!("{:X}", *c as u32)),
            Bool(b) => Str(format!("{:X}", if *b { 1 } else { 0 })),
            Str(s) => {
                let mut out = String::with_capacity(s.len() * 2);
                for byte in s.as_bytes() {
                    out.push_str(&format!("{:02X}", byte));
                }
                Str(out)
            }
            other => Str(other.as_str()),
        },
        "to_debug_str" => match receiver {
            Str(s) => Str(format!("{:?}", s)),
            Char(c) => Str(format!("{:?}", c)),
            other => Str(other.as_str()),
        },
        // v0.25 (Track D): binary / octal bare conversions.
        "to_bin_str" => match receiver {
            Int(n, _) => {
                let v = *n as i64;
                if v < 0 {
                    Str(format!("-{:b}", v.unsigned_abs()))
                } else {
                    Str(format!("{:b}", v as u64))
                }
            }
            Char(c) => Str(format!("{:b}", *c as u32)),
            Bool(b) => Str(format!("{:b}", if *b { 1 } else { 0 })),
            other => Str(other.as_str()),
        },
        "to_oct_str" => match receiver {
            Int(n, _) => {
                let v = *n as i64;
                if v < 0 {
                    Str(format!("-{:o}", v.unsigned_abs()))
                } else {
                    Str(format!("{:o}", v as u64))
                }
            }
            Char(c) => Str(format!("{:o}", *c as u32)),
            Bool(b) => Str(format!("{:o}", if *b { 1 } else { 0 })),
            other => Str(other.as_str()),
        },
        // v0.25 (Track D): spec-helper methods —
        // `to_{kind}_spec(sign_plus: Bool, alternate: Bool, precision: U32)`.
        // The precision sentinel `u32::MAX` (4294967295) means
        // "no precision". The expander emits literal `4294967295` for
        // the None case.
        "to_str_spec" => {
            let sign_plus = args.first().map(Value::truthy).unwrap_or(false);
            let _alternate = args.get(1).map(Value::truthy).unwrap_or(false);
            let precision = args.get(2).and_then(|v| v.as_int()).map(|n| n as i64);
            render_display_spec(receiver, sign_plus, precision)
        }
        "to_hex_str_spec" => {
            let sign_plus = args.first().map(Value::truthy).unwrap_or(false);
            let alternate = args.get(1).map(Value::truthy).unwrap_or(false);
            let precision = args.get(2).and_then(|v| v.as_int()).map(|n| n as i64);
            render_radix_spec(receiver, 16, false, "0x", sign_plus, alternate, precision)
        }
        "to_hex_upper_str_spec" => {
            let sign_plus = args.first().map(Value::truthy).unwrap_or(false);
            let alternate = args.get(1).map(Value::truthy).unwrap_or(false);
            let precision = args.get(2).and_then(|v| v.as_int()).map(|n| n as i64);
            render_radix_spec(receiver, 16, true, "0x", sign_plus, alternate, precision)
        }
        "to_bin_str_spec" => {
            let sign_plus = args.first().map(Value::truthy).unwrap_or(false);
            let alternate = args.get(1).map(Value::truthy).unwrap_or(false);
            let precision = args.get(2).and_then(|v| v.as_int()).map(|n| n as i64);
            render_radix_spec(receiver, 2, false, "0b", sign_plus, alternate, precision)
        }
        "to_oct_str_spec" => {
            let sign_plus = args.first().map(Value::truthy).unwrap_or(false);
            let alternate = args.get(1).map(Value::truthy).unwrap_or(false);
            let precision = args.get(2).and_then(|v| v.as_int()).map(|n| n as i64);
            render_radix_spec(receiver, 8, false, "0o", sign_plus, alternate, precision)
        }
        "to_debug_str_spec" => {
            // Debug ignores sign_plus/alternate/precision (no-op).
            match receiver {
                Str(s) => Str(format!("{:?}", s)),
                Char(c) => Str(format!("{:?}", c)),
                other => Str(other.as_str()),
            }
        }
        // v0.25 (Track D): width-padding tail on a string value.
        // Signature: `pad_str(width: U32, fill: Char, align: Str)`.
        "pad_str" => {
            let width = args.first().and_then(|v| v.as_int()).unwrap_or(0).max(0) as usize;
            let fill = match args.get(1) {
                Some(Char(c)) => *c,
                Some(Str(s)) => s.chars().next().unwrap_or(' '),
                _ => ' ',
            };
            let align = args
                .get(2)
                .map(Value::as_str)
                .unwrap_or_else(|| "right".into());
            let s = receiver.as_str();
            Str(pad_str(&s, width, fill, &align))
        }
        "is_empty" => match receiver {
            Str(s) => Bool(s.is_empty()),
            Array(xs) => Bool(xs.is_empty()),
            _ => Bool(false),
        },

        // ---------------- Result/Option helpers ----------------
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
                adt: mty_types::AdtId(0),
                variant: 0,
                payload: payload.clone(),
            },
            _ => Enum {
                adt: mty_types::AdtId(0),
                variant: 1,
                payload: vec![Unit],
            },
        },
        "ok_or" => match receiver {
            Enum {
                variant, payload, ..
            } if *variant == 0 => Enum {
                adt: mty_types::AdtId(0),
                variant: 0,
                payload: payload.clone(),
            },
            _ => Enum {
                adt: mty_types::AdtId(0),
                variant: 1,
                payload: vec![args.first().cloned().unwrap_or(Unit)],
            },
        },
        "ro" | "rw" | "path" | "host" => receiver.clone(),

        // ---------------- Str methods (v0.5 dogfood gap #3) ----------------
        "contains" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(needle)) => Bool(s.contains(needle.as_str())),
            (Array(xs), Some(needle)) => Bool(xs.iter().any(|v| v.as_str() == needle)),
            _ => Bool(false),
        },
        "starts_with" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(p)) => Bool(s.starts_with(p.as_str())),
            _ => Bool(false),
        },
        "ends_with" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(p)) => Bool(s.ends_with(p.as_str())),
            _ => Bool(false),
        },
        "find" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(needle)) => {
                // Return byte-index of first match as Option[USize].
                match s.find(needle.as_str()) {
                    Some(idx) => some(Int(idx as i128, IntKind::USize)),
                    None => none(),
                }
            }
            _ => none(),
        },
        "char_at" => match (receiver, arg_usize(args, 0)) {
            (Str(s), Some(i)) => match s.chars().nth(i) {
                Some(c) => some(Char(c)),
                None => none(),
            },
            _ => none(),
        },
        "slice" => match (receiver, arg_usize(args, 0), arg_usize(args, 1)) {
            (Str(s), Some(start), Some(end)) => {
                // start/end are char indices (Mighty spec §11 strings
                // are char-indexed). Skip + take.
                if start > end {
                    return none();
                }
                let sliced: String = s.chars().skip(start).take(end - start).collect();
                some(Str(sliced))
            }
            _ => none(),
        },
        "to_lower" | "to_lowercase" => match receiver {
            Str(s) => Str(s.to_lowercase()),
            Char(c) => Str(c.to_lowercase().collect()),
            _ => receiver.clone(),
        },
        "to_upper" | "to_uppercase" => match receiver {
            Str(s) => Str(s.to_uppercase()),
            Char(c) => Str(c.to_uppercase().collect()),
            _ => receiver.clone(),
        },
        "trim" => match receiver {
            Str(s) => Str(s.trim().to_string()),
            _ => receiver.clone(),
        },
        "trim_start" => match receiver {
            Str(s) => Str(s.trim_start().to_string()),
            _ => receiver.clone(),
        },
        "trim_end" => match receiver {
            Str(s) => Str(s.trim_end().to_string()),
            _ => receiver.clone(),
        },
        "split" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(sep)) if !sep.is_empty() => {
                Array(s.split(sep.as_str()).map(|p| Str(p.to_string())).collect())
            }
            (Str(s), Some(_)) => {
                // Empty separator → single-element array (mirrors Rust's
                // panic case, but we keep determinism here).
                Array(vec![Str(s.clone())])
            }
            _ => Array(vec![]),
        },
        "chars" => match receiver {
            Str(s) => Array(s.chars().map(Char).collect()),
            _ => Array(vec![]),
        },
        "bytes" => match receiver {
            Str(s) => Array(
                s.as_bytes()
                    .iter()
                    .map(|b| Int(*b as i128, IntKind::U8))
                    .collect(),
            ),
            _ => Array(vec![]),
        },
        "replace" => match (receiver, arg_str(args, 0), arg_str(args, 1)) {
            (Str(s), Some(from), Some(to)) if !from.is_empty() => {
                Str(s.replace(from.as_str(), to.as_str()))
            }
            (Str(s), _, _) => Str(s.clone()),
            _ => receiver.clone(),
        },
        "repeat" => match (receiver, arg_usize(args, 0)) {
            (Str(s), Some(n)) => Str(s.repeat(n)),
            _ => receiver.clone(),
        },

        // ---------------- String (mutable) helpers ----------------
        // Mighty spec treats `String` as the owned, mutable form; the
        // interpreter stores both `String` and `Str` as `Value::Str(_)`,
        // so push/push_str/clear behave the same. These return Unit and
        // (best-effort) mutate via the deref-write path when the call
        // is `Stmt::Assign((*self).field, MethodCall { ... })`. Inside
        // this pure helper they can only return the new value — the
        // caller is responsible for storing it back. To make
        // `s.push_str("x")` work in user code we additionally support
        // returning the new string on the value channel.
        "push" => match (receiver, args.first()) {
            (Str(s), Some(Char(c))) => {
                let mut out = s.clone();
                out.push(*c);
                Str(out)
            }
            (Array(xs), Some(v)) => {
                let mut out = xs.clone();
                out.push(v.clone());
                Array(out)
            }
            _ => receiver.clone(),
        },
        "push_str" => match (receiver, arg_str(args, 0)) {
            (Str(s), Some(t)) => {
                let mut out = s.clone();
                out.push_str(&t);
                Str(out)
            }
            _ => receiver.clone(),
        },
        "clear" => match receiver {
            Str(_) => Str(String::new()),
            Array(_) => Array(vec![]),
            _ => receiver.clone(),
        },
        "pop" => match receiver {
            Str(s) => {
                let mut t = s.clone();
                match t.pop() {
                    Some(c) => some(Char(c)),
                    None => none(),
                }
            }
            Array(xs) => match xs.last() {
                Some(v) => some(v.clone()),
                None => none(),
            },
            _ => none(),
        },

        // ---------------- Vec[T] helpers ----------------
        "get" => match (receiver, arg_usize(args, 0)) {
            (Array(xs), Some(i)) => match xs.get(i) {
                Some(v) => some(v.clone()),
                None => none(),
            },
            _ => none(),
        },
        "first" => match receiver {
            Array(xs) => match xs.first() {
                Some(v) => some(v.clone()),
                None => none(),
            },
            _ => none(),
        },
        "last" => match receiver {
            Array(xs) => match xs.last() {
                Some(v) => some(v.clone()),
                None => none(),
            },
            _ => none(),
        },
        "iter" => receiver.clone(),

        // ---------------- v0.25 (Track E): String + Vec[T] ctors / accessors ----------------
        //
        // Receiver-less constructors come in via the path-call lowering
        // route (e.g. `String.new()`). Lowering rewrites that as a call
        // whose callee falls through to `BuiltinId::Extern("String.new")`;
        // see `Interp::call_builtin` for the matching `try_stdlib_ctor`
        // hook below. The cases here cover the **method-style** receivers
        // — `s.with_capacity(n)`, `bytes.from_utf8()` — that the
        // dogfood agent uses in chained calls.
        "with_capacity" => match receiver {
            Str(_) => Str(String::with_capacity(arg_usize(args, 0).unwrap_or(0))),
            Array(_) => Array(Vec::with_capacity(arg_usize(args, 0).unwrap_or(0))),
            _ => {
                // Receiver-less form (`String.with_capacity(n)` or
                // `Vec.with_capacity(n)`): with no concrete receiver
                // value we default to an empty Str. Callers that need
                // Vec semantics will reassign to an Array on the first
                // push, which the permissive value layout supports.
                Str(String::with_capacity(arg_usize(args, 0).unwrap_or(0)))
            }
        },
        "from_str" => match arg_str(args, 0) {
            Some(s) => Str(s.to_string()),
            None => Str(String::new()),
        },
        "from_utf8" => match args.first() {
            Some(Str(s)) => match std::str::from_utf8(s.as_bytes()) {
                Ok(_) => some(Str(s.clone())),
                Err(_) => none(),
            },
            Some(Array(xs)) => {
                let bytes: Vec<u8> = xs
                    .iter()
                    .filter_map(|v| match v {
                        Int(n, _) => Some(*n as u8),
                        _ => None,
                    })
                    .collect();
                match String::from_utf8(bytes) {
                    Ok(s) => some(Str(s)),
                    Err(_) => none(),
                }
            }
            _ => none(),
        },
        "get_mut" => match (receiver, arg_usize(args, 0)) {
            // The interpreter's value model is by-clone, so `get_mut`
            // returns the same Option[T] shape as `get`. In-place writes
            // go through `Stmt::Assign` on the indexed place, not
            // through the returned reference.
            (Array(xs), Some(i)) => match xs.get(i) {
                Some(v) => some(v.clone()),
                None => none(),
            },
            (Str(s), Some(i)) => match s.chars().nth(i) {
                Some(c) => some(Char(c)),
                None => none(),
            },
            _ => none(),
        },
        "as_slice" | "as_mut_slice" => receiver.clone(),
        "capacity" => match receiver {
            Str(s) => Int(s.capacity() as i128, IntKind::USize),
            Array(xs) => Int(xs.capacity() as i128, IntKind::USize),
            _ => Int(0, IntKind::USize),
        },

        // ---------------- still-stubbed / permissive ----------------
        "query" => none(),

        _ => Unit,
    }
}

fn eval_cast(v: Value, ty: &IrTy) -> Value {
    match (v, ty) {
        (Value::Int(n, _), IrTy::Int(k)) => Value::Int(n, *k),
        (Value::Float(f, _), IrTy::Float(k)) => Value::Float(f, *k),
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

use mty_types::FloatKind;

// ---------------------------------------------------------------------------
// v0.25 Track D: format!() spec helpers
// ---------------------------------------------------------------------------

/// Sentinel "no precision specified" — matches the value the
/// `format!` expander emits as a literal U32::MAX. Keep in sync with
/// `mty_stdlib::fmt::PRECISION_NONE`.
const FMT_PRECISION_NONE: i64 = u32::MAX as i64;

/// Render `{:+}` / `{:.N}` style Display conversion for the spec
/// helper. `sign_plus` prepends `+` to non-negative numbers;
/// `precision_opt` (when not the `None` sentinel) caps the result to
/// that many chars for strings, or fixes float decimal places.
fn render_display_spec(receiver: &Value, sign_plus: bool, precision_opt: Option<i64>) -> Value {
    use Value::*;
    let precision = match precision_opt {
        Some(p) if p != FMT_PRECISION_NONE && p >= 0 => Some(p as usize),
        _ => None,
    };
    match receiver {
        Int(n, _) => {
            let v = *n;
            let body = if v < 0 {
                format!("-{}", v.unsigned_abs())
            } else if sign_plus {
                format!("+{}", v)
            } else {
                format!("{}", v)
            };
            Str(body)
        }
        Float(f, _) => {
            let body = match precision {
                Some(p) => format!("{:.*}", p, f.abs()),
                None => format!("{}", f.abs()),
            };
            let signed = if *f < 0.0 {
                format!("-{}", body)
            } else if sign_plus {
                format!("+{}", body)
            } else {
                body
            };
            Str(signed)
        }
        Str(s) => {
            let body = match precision {
                Some(p) => s.chars().take(p).collect::<String>(),
                None => s.clone(),
            };
            Str(body)
        }
        other => Str(other.as_str()),
    }
}

/// Render a `{:#x}` / `{:#b}` / `{:#o}` style integer-radix
/// conversion for the spec helper. `radix` is 2, 8, or 16;
/// `upper` controls hex-letter case; `prefix` is the alt-form prefix
/// (`"0x"`, `"0b"`, `"0o"`); `sign_plus` and `alternate` come from
/// the spec; precision pads the digit body to at least N digits with
/// leading zeros (precision is a no-op for non-integer receivers).
#[allow(clippy::too_many_arguments)]
fn render_radix_spec(
    receiver: &Value,
    radix: u32,
    upper: bool,
    prefix: &str,
    sign_plus: bool,
    alternate: bool,
    precision_opt: Option<i64>,
) -> Value {
    use Value::*;
    let precision = match precision_opt {
        Some(p) if p != FMT_PRECISION_NONE && p >= 0 => Some(p as usize),
        _ => None,
    };
    let (sign, magnitude_str) = match receiver {
        Int(n, _) => {
            let v = *n as i64;
            let mag = (v as i128).unsigned_abs();
            let body = format_radix_u128(mag, radix, upper);
            let sign = if v < 0 {
                "-"
            } else if sign_plus {
                "+"
            } else {
                ""
            };
            (sign, body)
        }
        Char(c) => {
            let body = format_radix_u128(*c as u128, radix, upper);
            (if sign_plus { "+" } else { "" }, body)
        }
        Bool(b) => {
            let body = format_radix_u128(if *b { 1 } else { 0 }, radix, upper);
            (if sign_plus { "+" } else { "" }, body)
        }
        other => return Str(other.as_str()),
    };
    let padded_body = match precision {
        Some(p) if magnitude_str.len() < p => {
            let mut s = String::with_capacity(p);
            for _ in 0..(p - magnitude_str.len()) {
                s.push('0');
            }
            s.push_str(&magnitude_str);
            s
        }
        _ => magnitude_str,
    };
    let pre = if alternate { prefix } else { "" };
    Value::Str(format!("{}{}{}", sign, pre, padded_body))
}

/// Render a non-negative integer in the requested radix (2/8/16).
fn format_radix_u128(mut n: u128, radix: u32, upper: bool) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let alphabet_lower: &[u8] = b"0123456789abcdef";
    let alphabet_upper: &[u8] = b"0123456789ABCDEF";
    let alph = if upper { alphabet_upper } else { alphabet_lower };
    let mut digits = Vec::with_capacity(32);
    let r = radix as u128;
    while n > 0 {
        let d = (n % r) as usize;
        digits.push(alph[d] as char);
        n /= r;
    }
    digits.iter().rev().collect()
}

/// Pad `s` to `width` characters with `fill`, aligning per `align`
/// (`"left"`/`"right"`/`"center"`/`"default"`). The `"default"`
/// sentinel selects right-alignment for content that looks numeric
/// (matches Rust's per-type default) and left-alignment otherwise.
fn pad_str(s: &str, width: usize, fill: char, align: &str) -> String {
    let len = s.chars().count();
    if len >= width || width == 0 {
        return s.to_string();
    }
    let total_pad = width - len;
    let resolved = if align == "default" {
        if looks_numeric(s) {
            "right"
        } else {
            "left"
        }
    } else {
        align
    };
    match resolved {
        "left" => {
            let mut out = String::with_capacity(s.len() + total_pad);
            out.push_str(s);
            for _ in 0..total_pad {
                out.push(fill);
            }
            out
        }
        "center" => {
            let left_pad = total_pad / 2;
            let right_pad = total_pad - left_pad;
            let mut out = String::with_capacity(s.len() + total_pad);
            for _ in 0..left_pad {
                out.push(fill);
            }
            out.push_str(s);
            for _ in 0..right_pad {
                out.push(fill);
            }
            out
        }
        _ => {
            // Default and "right": pad on the left. For numeric zero-pad
            // with a sign/prefix, Rust inserts the zeros *between* the
            // sign/prefix and the magnitude. We approximate that here:
            // if `fill == '0'` and `s` starts with a sign (`+`/`-`) or
            // with `0x`/`0b`/`0o`, lift the prefix and pad the tail.
            if fill == '0' {
                let (prefix, tail) = split_numeric_prefix(s);
                let tail_chars = tail.chars().count();
                let extra = width.saturating_sub(prefix.chars().count() + tail_chars);
                if extra > 0 {
                    let mut out = String::with_capacity(s.len() + extra);
                    out.push_str(prefix);
                    for _ in 0..extra {
                        out.push('0');
                    }
                    out.push_str(tail);
                    return out;
                }
                return s.to_string();
            }
            let mut out = String::with_capacity(s.len() + total_pad);
            for _ in 0..total_pad {
                out.push(fill);
            }
            out.push_str(s);
            out
        }
    }
}

/// Heuristic: does `s` look like a number? Used by the `"default"`
/// alignment sentinel to pick right-align (for numbers) vs left-align
/// (for strings). We accept an optional leading sign, an optional
/// `0x`/`0b`/`0o` prefix, then digits / a decimal point / etc.
fn looks_numeric(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    if bytes.len() >= i + 2 && bytes[i] == b'0' {
        let p = bytes[i + 1];
        if matches!(p, b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
            i += 2;
        }
    }
    if i >= bytes.len() {
        return false;
    }
    bytes[i..]
        .iter()
        .all(|&b| b.is_ascii_hexdigit() || b == b'.' || b == b'e' || b == b'E' || b == b'_')
}

/// For zero-pad alignment, split a numeric string into its sign +
/// alternate-form prefix and the magnitude tail. Returns `("", s)`
/// when no recognisable prefix is present.
fn split_numeric_prefix(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    let mut end = 0usize;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        end = 1;
    }
    if bytes.len() >= end + 2 && bytes[end] == b'0' {
        let p = bytes[end + 1];
        if matches!(p, b'x' | b'X' | b'b' | b'B' | b'o' | b'O') {
            end += 2;
        }
    }
    (&s[..end], &s[end..])
}
