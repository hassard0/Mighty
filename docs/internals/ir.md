# MtyIR — Mighty Mid-Level IR

> Compiler internals doc — current of slice 6 (`v0.6.0-sir`).

MtyIR (the **S**tardust mid-level **IR**) is the basic-block representation
the compiler hands to its execution and codegen backends. Slice 6 ships
MtyIR plus a tree-walking interpreter; later slices will reuse MtyIR as the
input to LLVM / Cranelift / Wasm lowering.

## Pipeline position

```
Source (.sd)
  → CST / AST       (mty-syntax, mty-ast)
  → HIR             (mty-hir)
  → type check      (mty-types)
  → borrow check    (mty-borrow)
  → MtyIR             (mty-sir, this doc) ←
  → interpreter     (mty-sir::interp, slice 6)
  → codegen         (slice 8)
```

MtyIR consumes the *typed + borrow-checked* HIR — the lowerer assumes
every name resolves, every type is known (or `Error`-poisoned), and
every move/copy decision is already pinned down. The lowerer never
emits new diagnostics; if upstream phases reported errors the MtyIR is
still produced (best-effort) but should not be executed.

## Shape

A `Program` is a vector of `Function`s, a vector of `AdtRef`s describing
the program's data types, and a vector of `Agent`s describing each user
agent's state-struct + handler fns.

Each `Function` is laid out MIR-style:

```
fn id: name(params...) -> ret_ty {
  let _0: ret_ty            // return slot
  let _1: ParamTy           // param 1
  let _2: LocalTy           // user let or temp
  ...

  bb0:
    _3 := const "hi"
    _4 := call @log(move _3)
    return move _4
}
```

Slice 6 uses **basic blocks + per-block straight-line statements**, not
SSA with phi nodes. Rationale: the borrow checker has already decided
every move vs copy, so we don't need value-renumbering; and the slice-6
interpreter is a stack machine that prefers explicit reads.

## Locals

Local `_0` is always the **return slot**. Locals `_1..=params.len()` are
the **parameters**. Beyond that come user bindings (`UserLet`) and
compiler temporaries (`Temp`).

Each `LocalDecl` records its `name` (for diagnostics), `ty`, `mutable`,
and `source` (one of `Return | Param | UserLet | Temp | DropFlag`).

## Statements

| Stmt              | Meaning                                                    |
|-------------------|------------------------------------------------------------|
| `Assign(p, rv)`   | `p := rv`                                                  |
| `Drop(l)`         | Conceptual drop of an owned value at scope exit            |
| `StorageLive(l)`  | Liveness marker (interpreter no-op; used by future passes) |
| `StorageDead(l)`  | Liveness marker                                            |
| `ArenaPush(a)`    | Enter `arena <name> { ... }`                               |
| `ArenaPop(a)`     | Leave the arena                                            |
| `EffectInvoke{}`  | Dispatch an effect-system call (`fs.read`, `net.get`, …)   |
| `Nop`             | Reserved                                                   |

## Rvalues

| Rvalue             | Notes                                                      |
|--------------------|------------------------------------------------------------|
| `Use(op)`          | Forward a `Copy`/`Move`/`Const` operand                    |
| `Const(c)`         | Literal value                                              |
| `BinOp(op, l, r)`  | Arithmetic / comparison / logical                          |
| `UnOp(op, x)`      | Negation / not                                             |
| `Ref{mutable, p}`  | Take a reference                                           |
| `Deref(op)`        | Dereference a reference                                    |
| `AdtInit{}`        | Construct a struct or enum value                           |
| `TupleInit(xs)`    | Build a tuple                                              |
| `ArrayInit(xs)`    | Build an array                                             |
| `FieldRead{}`      | `recv.fN`                                                  |
| `TupleRead{}`      | `recv.N`                                                   |
| `IndexRead{}`      | `recv[index]`                                              |
| `Call{}`           | Static fn call (user fn or builtin)                        |
| `MethodCall{}`     | Dynamic method dispatch                                    |
| `AgentSpawn{}`     | Slice-6 synchronous agent spawn                            |
| `Send/Ask{}`       | Slice-6 synchronous message dispatch                       |
| `CapValue{}`       | Capability literal                                         |
| `Cast{}`           | Numeric / pointer cast                                     |

## Terminators

| Term                 | Meaning                                                |
|----------------------|--------------------------------------------------------|
| `Goto(b)`            | Unconditional branch                                   |
| `If{cond, then, e}`  | Conditional branch                                     |
| `SwitchInt{}`        | Int-valued switch                                      |
| `SwitchVariant{}`    | Enum-tag-valued switch                                 |
| `Return(op)`         | Return a value                                         |
| `Panic{msg}`         | Trap with MT5001                                       |
| `Unreachable`        | Trap with MT5005                                       |
| `TryReturnErr(op)`   | Synthesize `Result::Err(op)` and return                |
| `Suspend{resume}`    | Async suspension placeholder (slice-7 traps MT5009)    |

## Places + projections

A `Place` is a `Local` with a list of `Projection`s:

```
Place { local: _3, proj: [Field(0), TupleIndex(1), Deref] }
// reads: (*(_3.f0).1)
```

Slice-6 projections: `Field(i)`, `TupleIndex(i)`, `Deref`,
`Index(local)`, `VariantField(variant, field)`.

## Capabilities + effects

Capabilities live in MtyIR as `SirTy::Cap{family, constraint}` and as
`Rvalue::CapValue{}` literals. Their constraint algebra is carried
verbatim from `sdust_types::CapConstraint` (A23). Effect calls lower
to `Stmt::EffectInvoke{effect, op, args, out}` with `op =
EffectOp::GenericCall{path, method}`.

## Arenas

`arena <name> { body }` emits `ArenaPush(a)` before the body's first
statement and `ArenaPop(a)` after the body's tail. The interpreter
tracks a stack of arenas per frame; allocations inside slice 6 are
inline-by-value so there is no real cleanup work yet (the future
runtime will free arena-allocated blocks here).

## `?` lowering

```
expr?
≡
%t := <expr>
switch_variant %t {
  variant 0 => bb_ok,   // Result::Ok payload at proj VariantField(0, 0)
  variant 1 => bb_err,
}
bb_ok:  result := move %t.v0.f0
bb_err: try_return_err move %t.v1.f0
```

`try_return_err` produces `Result::Err(payload)` and immediately returns
from the enclosing fn. The lowerer does **not** widen `err` types across
nested fn boundaries — the type checker already validated that the
inner err type is in the outer fn's err set.

## Agents

Each user `agent Name { state... on Msg(...) -> ... }` lowers to:

- a synthetic state ADT `__Name::State` whose fields mirror the agent's
  `state` declarations,
- one synthetic constructor fn `__Name::__new` that returns the state
  struct with each field initialized,
- one fn per handler `__Name::on_<Msg>` taking `&mut state` plus the
  message params.

`spawn Name(args)` lowers to `Rvalue::AgentSpawn{agent_id, args}`. The
interpreter executes the constructor synchronously, allocates a state
slot in `agent_states`, and returns a `Value::Agent(handle)`.

`target!Msg(args)` lowers to `Rvalue::Send{}`; `target?Msg(args)` to
`Rvalue::Ask{}`. Slice 6 dispatches these synchronously to the matching
handler fn (Amendment **A32**). Mailboxes and the work-stealing
scheduler arrive in slice 7.

## What slice 6 deliberately doesn't do

| Concern              | Status                                                       |
|----------------------|--------------------------------------------------------------|
| Async scheduler      | Slice 7 — `Suspend` traps MT5009 until then                  |
| Monomorphization     | Post-v0.1 — `SirTy::Param(name)` carries through unchanged   |
| DCE / inlining       | Post-v0.1                                                    |
| LLVM / Cranelift     | Slice 8                                                      |
| Wasm component model | Slice 8                                                      |
| Real arena allocator | Slice 7 (push/pop hooks already in place)                    |
| Field-level borrow   | Slice 7 (slice-4 still tracks at local granularity)          |

## File index

- `crates/mty-sir/src/sir.rs` — data types
- `crates/mty-sir/src/dump.rs` — text dump
- `crates/mty-sir/src/lower/mod.rs` — lowering entry
- `crates/mty-sir/src/lower/items.rs` — fn / struct / enum / agent
- `crates/mty-sir/src/lower/exprs.rs` — expression lowering
- `crates/mty-sir/src/lower/pats.rs` — pattern matching
- `crates/mty-sir/src/lower/ty.rs` — type translation
- `crates/mty-sir/src/interp/` — interpreter
- `docs/internals/interpreter.md` — interpreter design notes
