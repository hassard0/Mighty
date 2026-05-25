# Slice 6 — MtyIR + Interpreter (plan)

**Design:** `docs/superpowers/specs/2026-05-24-slice6-sir-interpreter-design.md`
**Slice:** 6 — Phase 3
**Target:** `v0.6.0-sir`

This plan is task-numbered to support parallel agent dispatch. Tasks
with the same group ID may run concurrently; later groups depend on
earlier ones.

---

## Group A — Foundation (sequential)

### T1. Create `mty-sir` crate skeleton

- Add `crates/mty-sir/{Cargo.toml,src/lib.rs}`
- Workspace: add to `Cargo.toml` members list
- Dependencies: `mty-hir`, `mty-types`, `mty-borrow`,
  `mty-diagnostics`, `la-arena`, `serde`
- `lib.rs` exports `pub mod sir; pub mod lower; pub mod interp; pub mod
  dump;`

### T2. Define MtyIR data types (`sir.rs`)

- `SirFnId`, `SirAdtId`, `Local`, `BlockId`, `ArenaId` newtype wrappers
- `Program { fns: Vec<Function>, adts: Vec<AdtRef>, agents: Vec<Agent> }`
- `Function { id, name, params, locals, blocks, entry, ret_ty, effects,
  hir_fn, span }`
- `Block { id, stmts: Vec<Stmt>, terminator: Term }`
- `Stmt`, `Rvalue`, `Operand`, `Place`, `Projection`, `Term`, `Const`,
  `FnRef`, `BuiltinId`, `EffectOp`, `SirTy`
- `LocalDecl { name, ty, mutable, source }`
- Re-export `CapFamily`/`CapConstraint` from `mty-types`
- Add Display impls behind `dump.rs`

### T3. Add MtyIR-specific diagnostic codes MT5001..MT5050

- Edit `crates/mty-diagnostics/src/codes.rs`: add constants from D17
- Add explain() arms for each code
- Add a `// Runtime: MT5001..MT5099` section comment

---

## Group B — Lowering (parallelizable)

### T4. Lowering framework (`lower/mod.rs`)

- `LowerCtx` holds: `&Package`, `&TypedPackage`, `&mut Program`,
  per-fn `BlockBuilder`, local map, scope stack, drop list, arena stack
- `pub fn lower_package(pkg, typed) -> Program`
- Walk top-level items; for each `Item::Fn` with a body, allocate a
  `Function` and call `lower_fn`

### T5. Expression lowering (`lower/exprs.rs`)

- One function per HIR expr variant returns `Operand`
- For statements that don't produce a value, return `Operand::Const(Unit)`
- Borrow / move / copy decisions read `typed.expr_ty` and the
  borrow-checker copy-predicate (`sdust_types::is_field_copy`)
- Calls: distinguish builtin (`log`, `panic`, `print`, `spawn`, `move`,
  `fetch`) from user fn (by `FnDefId`)
- Method calls: emit `Rvalue::MethodCall` (interpreter resolves at run
  time via builtin method table or trait dispatch)
- Send / Ask: emit `Rvalue::Send` / `Rvalue::Ask`
- Deadline: collect duration value, attach to enclosing Ask
- `?`: per D10 — synthesize switch + return-err
- Spawn: emit `Rvalue::AgentSpawn`
- Arena: emit `ArenaPush` + lower body + `ArenaPop`
- Budget / Sandbox / Unsafe / TaskScope: lower body inline
  (entries become metadata vectors stored on the surrounding stmt; no
  enforcement)

### T6. Pattern lowering (`lower/pats.rs`)

- `lower_pat_match(operand, pat, success_block, fail_block, bindings)`
- Wildcard: jump to success
- Literal: BinOp Eq, If
- Binding: assign + recurse into sub-pat
- Tuple/Struct/Enum: project fields + recursive matches
- Range: BinOp Le/Ge + If
- Used by both `match` arms and `if let`

### T7. Block + control flow lowering (`lower/blocks.rs`)

- `if/else`: emit `Term::If` to two blocks merging in a `phi` local
- `match`: chain `lower_pat_match` for each arm, default trap or fall-through
- `for x in iter`: lower iter to `Vec` (for slice 6 use `Array(...)`), emit
  loop over indices
- `while`: header-block + If + body-block + back-edge
- `loop`: header-block + body + back-edge; `break`/`continue` not in HIR
  yet, fallback to error-on-encounter (no example uses them)
- `return e`: `Term::Return(operand)`
- Block tail: result of tail expr becomes the block value

### T8. Item lowering (`lower/items.rs`)

- `lower_fn(fn_id)` — fn signature, body block
- `lower_struct` — synthesize an `AdtRef { id, kind=Struct, fields }`
- `lower_enum` — synthesize `AdtRef { kind=Enum, variants }`
- `lower_agent` — emit synthesized constructor + per-handler `Function`s
- Skip Protocol / Trait / Impl / Use / Mod / Extern at MtyIR layer (their
  signatures already live in DefMap; impl methods are lowered as plain
  fns)
- For `extern { fn foo(...) }` declarations, register the fn name as a
  `BuiltinId::Extern(name)` entry

### T9. HIR type → MtyIR type translation (`lower/types.rs`)

- `SirTy` mirrors a subset of `TyData`: Bool, Int, Float, Char, Str,
  String, Bytes, Unit, Never, Tuple, Array, Ref, Adt, Fn, Cap, Dyn, Error
- `from_ty(ty: TyId, arena: &TyArena) -> SirTy`
- Generics: monomorphization is post-v0.1; for slice 6 carry params as
  `SirTy::Param(usize)` placeholders that the interpreter never inspects

---

## Group C — Interpreter (depends on B)

### T10. Value + Frame types (`interp/value.rs`)

- `Value` enum per D16
- `Reference { scope_id, owner, proj, mutable }`
- `Frame { fn_id, locals: Vec<Value>, scope_id, block: BlockId }`
- `AgentHandle { id: u64, fn_table: HashMap<String, SirFnId>,
  state: usize }` (index into a state table the interpreter owns)

### T11. Host trait (`interp/host.rs`)

- `pub trait Host { fn effect_call(...); fn extern_call(name, args); fn
  print(s); fn eprint(s); }`
- `pub struct DefaultHost { stdout: Vec<u8>, stderr: Vec<u8>,
  effect_log: Vec<EffectCall> }` — buffers all output so tests can read it
- A `RealHost` writes to actual stdout/stderr for the CLI

### T12. Interpreter core (`interp/run.rs`)

- `pub fn run(prog: &Program, host: &mut dyn Host) -> RunResult`
- Find a `main` fn (by name)
- Push initial frame; execute terminator-driven loop
- Per Stmt: dispatch Assign, Drop, ArenaPush/Pop, EffectInvoke
- Per Term: Goto, If, SwitchInt, SwitchVariant, Return, Panic, TryReturnErr,
  Unreachable
- Step budget: configurable (default 1M); exceeded → MT5009 placeholder

### T13. Rvalue evaluation (`interp/eval.rs`)

- BinOp: arithmetic + comparison + logical (short-circuit handled in lowering)
- UnOp: Neg, Not, Deref
- StructInit / EnumInit / TupleInit / ArrayInit
- FieldAccess / TupleIndex / IndexAccess
- Borrow: produce Reference
- Call: dispatch to user fn or builtin
- MethodCall: try inherent impl table from typed.def_map, then
  trait-impl table, then builtin methods (`len`, `to_str`, `ok_or` etc.)
- Cast: integer widening/truncation; float convs
- AgentSpawn / Send / Ask: see T14

### T14. Agent dispatch (`interp/agents.rs`)

- AgentSpawn: allocate state by invoking the synthesized constructor fn,
  store in `agent_states: Vec<Value>`, return `Value::Agent(handle)`
- Send (`!`): look up handler by message name; call synchronously with
  `&mut state` + message args; ignore reply
- Ask (`?`): same as Send but return the reply value
- Deadline: discarded in slice 6 (recorded in trace for tests)
- Missing handler: MT5020

### T15. Built-in fns (`interp/builtins.rs`)

- `log(s)` → host.println(s)
- `print(s)` → host.print(s)
- `panic(msg)` → MT5001 trap
- `spawn(x)` → wrap x in an AgentHandle (synthesized agent for user fn)
- `move(x)` → identity
- `fetch(url)` → host.extern_call("fetch", ...) (default Ok(""))
- `null` → Value::Int(0)
- `raw_ptr(addr)` → Value::Int(addr)
- `valid(ptr, len)` → Value::Bool(true)

### T16. Built-in methods (`interp/methods.rs`)

- `.len` on Str/Array/Bytes
- `.to_str` / `.to_string`
- `.get` on Map-shaped Adt (returns Some/None on Array-of-tuples placeholder)
- `.ok_or(err)` on Option-shaped Adt
- `.contains`, `.starts_with`, `.ends_with`, `.parse`, `.split`, `.trim`,
  `.unwrap`, `.unwrap_or`, `.iter`, `.map`, `.filter`, `.collect`, `.fold`
- Permissive: unknown methods return `Value::Unit` and log a debug trace,
  so example lowering doesn't crash

---

## Group D — CLI + dump + driver wiring

### T17. MtyIR dump (`dump.rs`)

- `dump_program(p) -> String` — pretty-print like MIR:
  ```
  fn main() -> Unit {
    let _0: Unit
    let _1: Str

    bb0:
      _1 := const "hello, Mighty"
      _0 := call log(move _1)
      return move _0
  }
  ```
- Stable ordering (fn id ascending; blocks already ordered)

### T18. Driver hook (`crates/mty-driver/src/pipeline.rs`)

- Add `pub fn lower_to_sir(typed: &TypedPackage, pkg: &Package) ->
  sdust_sir::Program`
- Add `pub fn run_program(prog: &Program) -> i32` thin wrapper around
  `interp::run` with a `RealHost`

### T19. `mty dump --sir` (`crates/mty-cli/src/cmd/dump.rs`)

- Add `--sir` flag; when set, run pipeline → MtyIR → dump_program → stdout

### T20. `mty run` (`crates/mty-cli/src/cmd/run.rs`)

- New file, registered in `mod.rs`
- Run lower + typed + borrow + MtyIR; abort if errors
- Build Program, invoke interpreter with RealHost
- Forward stdout/stderr; exit code = interpreter's exit code

### T21. CLI subcommand wiring (`crates/mty-cli/src/main.rs`)

- Add `Run { path, args }` and `--sir` flag wiring

---

## Group E — Tests + examples

### T22. MtyIR unit tests

- `crates/mty-sir/src/sir.rs` — round-trip Const printing
- `crates/mty-sir/src/lower/mod.rs` — smoke test lowering `fn id(x:
  I32) -> I32 { x }`
- Cover If / Match / `?` / Arena / Send / Spawn / Method call shapes

### T23. Examples lower smoke (`tests/examples_sir_lower.rs`)

- For each of 20 examples: `lower → check → MtyIR`. Assert no panic and
  the Program contains ≥ 1 function (or 0 only for examples with no
  fn item).

### T24. Runnable examples (`tests/examples_interp.rs`)

- Subset that has a runnable main: 01 hello, 02 (call area indirectly),
  03 (call generic with array), 05 (call classify), 07 (Echoer agent),
  08 (Counter agent), 12 (turn), 16 (assert_eq macro — slice 6 keeps
  this lowering-only; macros are unexpanded), etc.
- Verify stdout matches expected.
- Examples requiring runtime: 10 supervisor, 18 sandbox, 19 backend,
  20 frontend, 11 budget, 13 capabilities, 14 extern c, 15 extern js
  — lowering-only; add a `// run: skipped — needs slice 7 runtime`
  banner comment if not already present

### T25. Conformance harness

- `tests/conformance/runtime/<name>/{input.sd,expected.txt}`
- `crates/mty-driver/tests/conformance_runtime.rs` discovers
  subdirectories and runs each pair
- Initial cases (≥ 5):
  1. `hello` — `log("hello")` → "hello\n"
  2. `arithmetic` — `log(format(1+2*3))` (use a simple int-to-str
     helper or hardcoded string)
  3. `if_chain` — branches over a literal cond
  4. `match_enum` — Option/Result match printing
  5. `for_sum` — accumulate a `Vec`-like array and print
  6. `result_propagation` — `?` returning early on Err
  7. `agent_echo` — Ping → reply printed
  8. `nested_match` — nested arms

### T26. Regression test for 274-pass corpus

- Re-run `cargo test --workspace` and assert ≥ 274 prior tests still pass

---

## Group F — Docs

### T27. `docs/internals/sir.md`

- Architecture, basic-block form, locals, projections, terminators,
  examples of HIR→MtyIR transforms (hello, if, match, ?, arena, agent)

### T28. `docs/internals/interpreter.md`

- Value model, frames, drop semantics, references, Host trait,
  built-ins, determinism guarantees, step budget

### T29. `docs/reference/cli/mty-run.md`

- Usage, exit codes, arg passing, env var handling (none), examples

### T30. Update `docs/getting-started.md`

- Add `mty run` section right after `mty check`

### T31. Update `docs/spec/v0.1-amendments.md`

- Append A31..A35 (per design D9, D12, D13, D14, D18)

### T32. SLICE6.md

- Mirror SLICE5.md structure: what landed, examples summary,
  amendments, stats, deferrals, file index

### T33. Update README roadmap

- Mark slice 6 done; note `v0.6.0-sir` tag; queue slice 7

---

## Group G — Ship

### T34. Full test sweep

- `cargo test --workspace` → all green
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`

### T35. SLICE5.md amendments

- Update deferrals list: remove "MtyIR / interpreter — slice 6" (delivered)
- Add "field-level borrow tracking" reaffirmed for slice 7 if not done

### T36. Run all 20 examples through `mty check` + `mty run`

- For runnable subset, confirm exit codes
- For lowering-only subset, confirm `mty dump --sir` succeeds

### T37. Commit slice-6 implementation

- Single commit (or 2 commits: code, docs)
- Message follows slice-5 format

### T38. Tag `v0.6.0-sir` and push

- `git tag v0.6.0-sir -m "Slice 6: MtyIR + interpreter"`
- `git push origin main`
- `git push origin v0.6.0-sir`

---

## Dispatch order

1. Sequential: **T1, T2, T3**
2. Parallel (Group B): **T4, T5, T6, T7, T8, T9** dispatched in waves
   based on internal dependencies — T4 first, then T5/T6/T7/T8/T9
   together
3. Sequential then parallel (Group C): **T10, T11**, then **T12, T13,
   T14, T15, T16** together
4. Parallel (Group D): **T17, T18, T19, T20, T21** (T18 depends on T17)
5. Parallel (Group E): **T22, T23, T24, T25, T26**
6. Parallel (Group F): **T27–T33**
7. Sequential (Group G): **T34, T35, T36, T37, T38**

Because the slice leader is a single agent (not a parallel orchestrator
this time), tasks will be executed in dependency order in a single
session, batching independent file writes when possible. The plan
remains task-numbered so re-entry after interrupts is straightforward.
