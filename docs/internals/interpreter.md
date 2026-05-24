# Stardust Interpreter — slice 6

The slice-6 interpreter (`sdust_sir::interp`) is a **tree-walking,
single-threaded, deterministic** executor for SIR. It exists so that
Stardust programs run end-to-end while the native and Wasm backends are
under construction.

## Entry points

- `run(prog, host) -> RunResult` — find `main`, build the initial
  activation, execute until completion or trap.
- `run_fn_by_name(prog, name, args, host) -> Result<Value, RunResult>` —
  invoke an arbitrary fn (used by tests).

`RunResult` is one of:

| Variant            | Process exit | Notes                                  |
|--------------------|--------------|----------------------------------------|
| `Ok { exit }`      | `exit`       | Normal completion                      |
| `Trap { … }`       | `1`          | Runtime panic, divide-by-zero, etc.    |
| `NoMain`           | `2`          | Program has no `main` fn               |
| `BudgetExceeded`   | `3`          | Step budget (default 1 000 000) hit    |

## Host trait

```rust
pub trait Host {
    fn print(&mut self, s: &str);
    fn println(&mut self, s: &str);
    fn eprint(&mut self, s: &str);
    fn effect_call(&mut self, effect: EffectId, op: &EffectOp, args: &[Value]) -> Value;
    fn extern_call(&mut self, name: &str, args: &[Value]) -> Value;
}
```

Two implementations ship in-box:

- `RealHost` writes to actual stdout/stderr (used by `sdust run`).
- `BufferHost` captures stdout + an `effect_log` + `extern_log` in
  memory (used by all interpreter tests).

Tests can implement `Host` themselves to script custom effect
responses or stub a specific extern fn.

## Value model

```
Value =
  | Unit | Bool(b) | Int(n, kind) | Float(n, kind)
  | Str(s) | Char(c) | Duration(ms) | Size(bytes)
  | Tuple(...) | Array(...)
  | Struct{adt, fields}
  | Enum{adt, variant, payload}
  | Ref(Reference) | Fn(FnRef) | Agent(AgentHandle)
  | Cap{family, constraint} | Void
```

`Void` is the interpreter's "uninitialized" marker. Reading a `Void`
local produces a poisoned-but-quiet `Unit`; assigning a value always
overwrites the slot.

References carry their owning local, projection path, mutability flag,
and a `ScopeId`. A `Reference` whose `ScopeId` no longer corresponds to
a live frame should trap with **SD5002**; slice-6 enforcement is
best-effort (the borrow checker already proves the static case).

## Frames + the step loop

Each `Frame` records its `fn_id`, `locals`, `block`, `pc`, `scope`, and
the stack of live arenas. The step loop fetches the next statement (or
the block terminator when `pc == stmts.len()`) and dispatches.

Call instructions push a new frame and rewind the parent's `pc` so the
calling Assign re-executes when the callee returns; the result lands in
`interp.last_return` and is then stored into the target Place.

## Drop semantics

Slice 6 evaluates `Stmt::Drop(local)` by overwriting the slot with
`Value::Void`. Non-Copy values are dropped at scope exit; the borrow
checker computed the drop list. The interpreter does no real
finalization (there is no native heap to free yet); future codegen will
honor `Drop` impls at this hook.

## Effect dispatch

`Stmt::EffectInvoke` is the sole route by which effects leave the
program. The interpreter resolves arg values, calls
`Host::effect_call(effect, op, args)`, and writes the host's return
value into the `out` place. The default `BufferHost` returns `Unit`.

This means slice 6 **does not** perform real I/O for `fs.read`,
`net.get`, etc. The compiler proves the effect set is declared; the
interpreter records that the call was made; the value flows through. A
test host can substitute realistic returns (per Amendment **A33**).

## Agent dispatch

`Rvalue::AgentSpawn` runs the constructor fn synchronously, stashes the
state in `interp.agent_states[idx]`, and returns
`Value::Agent(AgentHandle{ id, agent_sir_id, state_idx })`.

`Rvalue::Send` / `Rvalue::Ask` look up the handler by message name and
invoke it synchronously. The handler's reply value is propagated as the
Ask's result; sends discard the reply. A slice-6 limitation: writes to
state fields go through a heuristic "reply-as-state-field-0" path; full
mutable-ref-into-agent-state arrives in slice 7 with the real mailbox
runtime.

## Determinism

The interpreter is fully deterministic given the same `Program` and
`Host`:

- single-threaded, no work stealing;
- no system clock reads;
- no RNG (unless the host injects one);
- iteration order over locals is by ascending index;
- effect-call ordering is the program's lexical statement order.

Amendment **A35**: this determinism is the basis for slice-6
conformance tests; slice 7's scheduler will introduce a separate
*deterministic mode* (matching spec §25.5).

## Step budget

Default 1 000 000 ops. Tests may construct an interpreter with a
custom budget to detect infinite loops. Exceeding the budget returns
`RunResult::BudgetExceeded` (process exit 3, distinct from a trap).

## Diagnostic codes

| Code   | Trap                              |
|--------|-----------------------------------|
| SD5001 | `panic(msg)` or explicit Panic    |
| SD5002 | Use-after-drop                    |
| SD5003 | Divide / remainder by zero        |
| SD5004 | Integer overflow (debug-only)     |
| SD5005 | Unreachable (fell off match arms) |
| SD5006 | `main` returned `Result::Err`     |
| SD5007 | Arena escape at run time          |
| SD5008 | Unimplemented builtin             |
| SD5009 | Step budget exceeded / suspension |
| SD5010 | Sandbox violation (placeholder)   |
| SD5020 | Missing agent handler             |
| SD5021 | Send to dead agent                |
| SD5050 | Extern fn unimplemented           |

## File index

- `crates/sdust-sir/src/interp/value.rs` — `Value`, `Frame`,
  `Reference`, `AgentHandle`
- `crates/sdust-sir/src/interp/host.rs` — `Host`, `RealHost`,
  `BufferHost`
- `crates/sdust-sir/src/interp/run.rs` — step loop, eval, builtins
