# Slice 6 — MtyIR + Interpreter (design)

**Date:** 2026-05-24
**Slice:** 6 (Phase 3 from spec §31.4)
**Target tag:** `v0.6.0-sir`

## Goal

Give Mighty a working evaluator. After slice 6, `mty run examples/01_hello.sd`
prints `hello, Mighty` and exits 0. MtyIR is the mid-level representation
defined in spec §24.4; the interpreter walks MtyIR directly. Native and Wasm
codegen are deferred to slice 8.

## Non-goals (deferred)

- Concurrent task scheduler, real mailboxes, real supervisors → **slice 7**
- LLVM/Cranelift codegen, Wasm codegen → **slice 8**
- Optimizations on MtyIR (DCE, inlining, escape analysis) → post-v0.1
- Polonius / NLL extensions → post-v0.1
- Effect-row polymorphism → post-v0.1
- Real `extern c` / `extern js` calls → post-v0.1 (interpreter stubs)
- AIR (agent IR) as a separate IR layer — slice 6 inlines AIR concerns
  into MtyIR for examples that need them, and the runtime in slice 7 will
  drive whatever AIR shape is needed; we don't pre-build an AIR crate now

## Architecture decisions

### D1 — New crate `mty-sir`

Mirrors the slice-3/4/5 pattern. The crate exposes:

- `sir::Program` — collection of `Function`s + `Const`s indexed by
  `SirFnId`.
- `sir::Function` — locals, blocks, terminators.
- `lower::lower_package(pkg, typed) -> Program` — HIR→MtyIR lowering.
- `interp::run(prog, host) -> ExitCode` — interpreter entry.
- `dump::dump_program(prog) -> String` — pretty-printer.

It depends on `mty-hir`, `mty-types`, `mty-borrow`, and
`mty-diagnostics`. It does **not** depend on `mty-driver`; the driver
exposes a `lower_to_sir` helper that chains it on after borrow check.

### D2 — MtyIR shape: basic-block form with explicit moves and copies

We pick **basic-block + per-block straight-line statements**, not SSA
with phi nodes. Rationale:

- Phi-style SSA pays back in optimization-heavy backends; slice 6 has no
  optimizer.
- The borrow checker already produced ownership decisions; MtyIR records
  them as `Move` vs `Copy` opcodes and we don't need value-renumbering.
- A straight-line form composes naturally with a stack-machine
  interpreter and a future LLVM lowering that uses LLVM's own SSA.

Each function has:

```
Function {
  id: SirFnId,
  name: String,
  params: Vec<Local>,          // locals 0..params.len()
  locals: Vec<LocalDecl>,      // includes params
  blocks: Vec<Block>,
  entry: BlockId,
  ret_ty: SirTy,
  effects: Vec<EffectId>,
  hir_fn: Option<FnId>,        // for diagnostics
  span: SourceSpan,
}
Block { id, stmts: Vec<Stmt>, terminator: Term }
```

`LocalDecl` records `name` (for diagnostics), `ty`, `mutable`, and
`source: LocalSource` (`Param | UserLet | Temp | DropFlag`).

### D3 — Statements

```
enum Stmt {
  Assign(Local, Rvalue),       // local := rvalue
  Drop(Local),                 // explicit drop (non-Copy owned values)
  StorageLive(Local),
  StorageDead(Local),
  ArenaPush(ArenaId),          // enter `arena name { ... }`
  ArenaPop(ArenaId),
  EffectInvoke {               // effect-marked operation; bypasses Rvalue
      effect: EffectId,
      op: EffectOp,
      args: Vec<Operand>,
      out: Option<Local>,
  },
  Nop,
}
```

### D4 — Rvalues

```
enum Rvalue {
  Use(Operand),                // Operand::Copy or Operand::Move
  Const(Const),                // literal
  BinOp(BinOp, Operand, Operand),
  UnOp(UnOp, Operand),
  Borrow { mutable: bool, place: Place },
  Deref(Operand),
  StructInit { adt: AdtId, variant: usize, fields: Vec<(usize, Operand)> },
  EnumInit { adt: AdtId, variant: usize, payload: Vec<Operand> },
  TupleInit(Vec<Operand>),
  ArrayInit(Vec<Operand>),
  FieldAccess { receiver: Place, field: usize },
  TupleIndex { receiver: Place, idx: usize },
  IndexAccess { receiver: Place, index: Operand },
  Call { func: FnRef, args: Vec<Operand> },
  MethodCall { receiver: Operand, method: String, args: Vec<Operand> },
  AgentSpawn { agent: AdtId, args: Vec<Operand> },
  Send { target: Operand, msg: String, args: Vec<Operand> },
  Ask { target: Operand, msg: String, args: Vec<Operand>, deadline_ms: Option<u64> },
  CapValue { family: CapFamily, constraint: CapConstraint },
  Cast { src: Operand, ty: SirTy },
}
```

`Operand` is `Copy(Place) | Move(Place) | Const(Const)`.

`Place` is `Local + projections`, but slice 6 uses a flat form
`Place { local: Local, proj: Vec<Projection> }` where `Projection ::=
Field(usize) | TupleIndex(usize) | Deref | Index(Local)`.

### D5 — Terminators

```
enum Term {
  Goto(BlockId),
  If { cond: Operand, then: BlockId, else_: BlockId },
  SwitchInt { discr: Operand, arms: Vec<(i128, BlockId)>, default: BlockId },
  SwitchVariant { discr: Operand, arms: Vec<(usize, BlockId)>, default: BlockId },
  Return(Operand),
  Panic { msg: Operand },
  Unreachable,
  TryReturnErr(Operand),       // for `?` propagation: build Err and return
  Suspend { resume: BlockId }, // async placeholder, unused by slice 6
}
```

### D6 — FnRef and built-ins

`FnRef` is either `User(SirFnId)` or `Builtin(BuiltinId)`. Built-ins
covered by slice 6:

| Built-in | Signature                          | Notes                          |
|----------|------------------------------------|--------------------------------|
| `log`    | `fn(Str) -> Unit`                  | prints to stdout + newline     |
| `print`  | `fn(Str) -> Unit`                  | prints without newline         |
| `panic`  | `fn(Str) -> Never`                 | traps with MT5001              |
| `spawn`  | `fn(T) -> AgentRef[T]`             | slice 6: returns opaque handle |
| `move`   | `fn(T) -> T`                       | identity (compiles to Move)    |
| `fetch`  | `fn(Url) -> Str!NetErr`            | host stub; default returns Ok("")|

Effect built-ins (`net.get`, `fs.read`, etc.) are dispatched via a
**host registry** the interpreter consults. The default host returns
deterministic stub values (empty string / zero) and increments a counter;
tests can register their own.

### D7 — Const

`Const` mirrors HIR literals plus a `Unit` and `Bool`:

```
enum Const {
  Unit,
  Bool(bool),
  Int(i128, IntKind),
  Float(f64, FloatKind),
  Str(String),
  Char(char),
  Duration { value: u64, unit: String },
  Size { value: u64, unit: String },
}
```

### D8 — Borrow values at runtime

The interpreter represents a borrow as a *handle* `Ref { owner: Local,
proj: Vec<Projection>, mutable: bool }`. Reading a borrow re-reads from
the owner; this preserves the slice-4 invariant that the borrow points
into a live local. We don't track aliasing at run time — the borrow
checker already proved no overlap. A borrow whose owner has been dropped
yields MT5002 at access time.

### D9 — Arena lifetimes

`arena <name> { body }` lowers to `ArenaPush(id)` at the entry of `body`
and `ArenaPop(id)` after the tail expression. At runtime, the
interpreter keeps a stack of arena scopes; any heap-like allocations
(currently `Vec`/`Map` placeholders for slice 6 are inline so this is a
no-op for correctness, but the scope is still tracked so MtyIR dumps and
the future runtime have hooks). The interpreter does **not** trap arena
escape at run-time — it relies on borrow-check MT3010 (Amendment **A31**:
runtime arena enforcement is a slice-7 obligation).

### D10 — `?` operator lowering

`expr?` lowers to:

```
%tmp := <expr>
SwitchVariant %tmp { Ok => goto bb_ok; Err => goto bb_err }
bb_ok:  %unwrapped := move %tmp.Ok.0; goto bb_cont
bb_err: %wrapped := EnumInit Result::Err(move %tmp.Err.0); Return %wrapped
bb_cont:
```

We synthesize the enclosing fn's `Err` type by widening the inner `Err`
into the outer error union, which the type checker already validated.
If the outer return type is not a `Result`, the lowerer emits no MtyIR for
that fn (it would have errored in typeck; MT2010).

### D11 — Match lowering

We compile match arms by chained `If` terminators, falling back to
`SwitchVariant` when all arms are simple enum constructors. Slice 6
keeps it simple — patterns are evaluated top-down:

- Wildcard → unconditional jump.
- Literal → equality check via `Const` + `If`.
- Binding (no sub-pat) → bind and jump.
- Enum constructor → `SwitchVariant` with payload extraction.
- Struct constructor → field accesses + sub-pat tests AND-folded.
- Range → two `BinOp` + `If`.

We do not check exhaustiveness at MtyIR-lowering time (typeck MT2015
already does). If the runtime falls off the end (e.g. typeck disabled),
it traps MT5005 *unreachable_match*.

### D12 — Agents in slice 6

`agent Counter: Count { on Inc() -> { n += 1; n } }` lowers to:

- a synthetic struct `Counter` with the state fields,
- one `Function` per handler (taking `&mut self` + message args),
- a `Function` named `__Counter::__new` constructing the initial state.

`spawn AgentExpr(args)` lowers to `Rvalue::AgentSpawn` which the
interpreter executes by allocating a state record and returning an
`AgentRef` value carrying a pointer. `target!Msg(args)` (send) and
`target?Msg(args)` (ask) are dispatched to the handler **synchronously,
in-thread, deterministically** in slice 6 (Amendment **A32**: slice-6
agents are direct-call; mailbox queuing arrives in slice 7). `@2s`
deadlines are recorded but never trip during slice-6 interpretation
(another slice-7 obligation).

### D13 — Effects at run time

Effect calls (`fs.read(path)`, `net.get(url)`, etc.) are detected during
lowering by inspecting the receiver path. They lower to
`Stmt::EffectInvoke`. The interpreter's host trait
`Host { effect_call(family, op, args) -> Value }` defaults to deterministic
stubs (empty string, 0, Unit) so examples run without panicking. Test
hosts can override. Amendment **A33**: slice-6 effects = host-callback;
slice-7+ wire real syscalls.

### D14 — Budgets and sandboxes in slice 6

`budget { ... } run { body }` and `sandbox Name with { ... } { body }`
lower to `body` directly. The entries are stored as `Vec<(String,
Const)>` metadata on the enclosing block but the interpreter does not
enforce them. The borrow checker already validated the static
structure; runtime enforcement is slice 7. Amendment **A34**.

### D15 — `extern { fn foo(...) }` calls

Treated as built-ins with a "stub" body: the interpreter looks them up
in the host's `extern_fn` table; absent entries return the appropriate
zero value (`Unit`, `0`, `""`, `Ok(zero)`). This keeps examples 06 and
11 runnable without writing real bodies.

### D16 — Runtime values

```
enum Value {
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
  Struct { adt: AdtId, fields: Vec<Value> },
  Enum { adt: AdtId, variant: usize, payload: Vec<Value> },
  Ref(Reference),
  Fn(SirFnId),
  Agent(AgentHandle),
  Cap { family: CapFamily, constraint: CapConstraint },
  Err,                          // poison for unevaluated paths
}
```

`Reference` is `{ scope_id: u64, owner: Local, proj: Vec<Projection>, mutable: bool }`
where `scope_id` ties the reference to a specific frame; a stale `scope_id`
traps MT5002.

### D17 — Diagnostics SD5xxx (runtime)

| Code   | Kind                           |
|--------|--------------------------------|
| MT5001 | runtime_panic (explicit)       |
| MT5002 | use_after_drop (stale ref)     |
| MT5003 | division_by_zero               |
| MT5004 | integer_overflow (debug only)  |
| MT5005 | unreachable_match              |
| MT5006 | unhandled_error_result         |
| MT5007 | arena_escape_runtime           |
| MT5008 | uncallable_builtin             |
| MT5009 | budget_exceeded (placeholder)  |
| MT5010 | sandbox_violation (placeholder)|
| MT5020 | agent_handler_missing          |
| MT5021 | send_to_dead_agent             |
| MT5050 | extern_fn_unimpl               |

Runtime diagnostics are reported via `Diagnostic` with `severity =
Error` and a non-overlapping range (`5001..5099`). They do **not** include
source labels in slice 6 — they print a single message line and the
interpreter exits with code 1.

### D18 — Determinism

The slice-6 scheduler is trivially deterministic (single-threaded,
synchronous). The interpreter:

- iterates `HashMap`s only via `BTreeMap` or pre-sorted vectors;
- never reads system time;
- never uses RNG;
- effect-call ordering is the program's lexical order.

Amendment **A35**: slice-6 interpretation is deterministic; non-determinism
appears only when slice-7's work-stealing scheduler is enabled.

### D19 — CLI shape

```
mty run <file>         # compile + execute; exit code from main
mty run <file> -- arg1 arg2   # forward args to main as &[Str]
mty dump --sir <file>  # print MtyIR text
```

The `dump --sir` flag joins `--ast --cst --hir`. The `run` subcommand
short-circuits with exit 1 if any prior phase reported errors.

### D20 — Test layout

- `crates/mty-sir/src/` — unit tests for lowering specific shapes.
- `crates/mty-driver/tests/sir_lower_examples.rs` — every example
  lowers without panic, snapshot the MtyIR for a chosen subset.
- `crates/mty-driver/tests/interp_examples.rs` — runnable examples
  produce expected stdout.
- `tests/conformance/runtime/<name>/input.sd` + `expected.txt`
  pairs driven by a single `runtime_conformance.rs` test.

### D21 — Build-time targets

- 274 existing tests stay green.
- Add ~40-60 MtyIR tests + ~10 interpreter tests + 5+ conformance tests.
- No clippy warnings.

## Risks + mitigations

| Risk                                          | Mitigation                                                            |
|-----------------------------------------------|------------------------------------------------------------------------|
| Lowering blows up on weird HIR shapes         | Per-shape unit tests + 20-example smoke test                          |
| Interpreter loops forever on agent recursion  | Hard step budget (1M ops) trips MT5009 placeholder; configurable      |
| `?` propagation across nested calls           | Lowered into per-call check; verified by conformance test             |
| Pattern-match arity drift                     | Lowerer asserts `payload.len() == variant.fields.len()` else MT5005   |
| Borrow value handles leak across drops        | Each fn frame stamps a `scope_id` on issued refs                      |
| Snapshot tests get noisy                      | Use `insta` with explicit snapshot files; review diffs before commit  |

## Open questions (resolved)

- **Q1**: Should we build AIR? → No, inline AIR into MtyIR (D1 note).
- **Q2**: SSA vs basic-block? → Basic-block (D2).
- **Q3**: Mailbox in slice 6? → No, sync dispatch (D12).
- **Q4**: Budget enforcement? → Metadata only, slice 7 enforces (D14).
- **Q5**: Effects = panic / log? → Host-stub callbacks (D13).

## Acceptance

- `mty run examples/01_hello.sd` prints `hello, Mighty` and exits 0.
- `mty run` works for the 5+ examples with a runnable `main()`.
- All 20 examples lower to MtyIR without internal errors.
- 5+ runtime conformance tests pass.
- MtyIR dumps are deterministic across runs.
- 274 + new tests pass, no clippy warnings.
- Tag `v0.6.0-sir` pushed.
