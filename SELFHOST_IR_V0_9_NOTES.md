# SELFHOST_IR_V0_9_NOTES

This file catalogues the language gaps + interpretation calls encountered
while porting MtyIR (mid-level IR) lowering to Mighty for v0.9.

Live status:
- `selfhost/ir/lib.mty` — 26 LOC, `mty check` clean (package + intent
  doc).
- `selfhost/ir/nodes.mty` — 122 LOC, `mty check` clean (data-shape
  spec mirroring `crates/mty-ir/src/ir.rs`).
- `selfhost/ir/lower.mty` — ~530 LOC, `mty check` clean, `cargo test -p
  mty-driver --test selfhost_ir` passes 7/7 live tests on examples
  01-03 (04 + 05 ignored, see below).

## v0.9 production matrix (what the Mighty IR lowerer covers)

The Rust IR lowerer in `crates/mty-ir/src/lower/` is ~2500 LOC across
5 files (ctx/items/exprs/pats/ty). Replicating all of it in Mighty
would be 4-5 KLOC and exceed the v0.9 5-hour budget by 3-4x. We ship
the productions exercised by examples 01-03 (and partially 04-05):

| Feature group | Mighty IR coverage | Rust IR coverage | Status |
|---|---|---|---|
| fn decls (sig + body) | YES | YES | shipped |
| struct decls (ADT emit) | YES (name + n_variants) | YES (full ADT defs) | shipped-subset |
| enum decls (ADT emit) | YES (name + n_variants) | YES (full variant fields + types) | shipped-subset |
| type aliases | NO | YES | deferred (no IR emission) |
| use/mod decls | NO (skipped) | NO (skipped in IR) | parity |
| extern blocks | NO | YES (BuiltinId::Extern) | deferred |
| literals (Int/Float/Str/Char/Bool/Unit) | YES (Const rvalue) | YES | shipped |
| path expressions | YES (Use rvalue) | YES (resolved via def_map) | shipped-subset |
| binary expressions | YES (BinOp rvalue) | YES | shipped |
| unary expressions | YES (UnOp rvalue) | YES | shipped |
| call expressions | YES (Call rvalue + EffectInvoke stmt for log/print/panic) | YES (full builtin dispatch) | shipped-subset |
| method call expressions | YES (MethodCall rvalue) | YES (DomOp dispatch + cap routing) | shipped-subset |
| field expressions | YES (FieldRead rvalue) | YES | shipped |
| index expressions | YES (IndexRead rvalue) | YES | shipped |
| tuple expressions | YES (TupleInit rvalue) | YES | shipped |
| array expressions | YES (ArrayInit rvalue) | YES | shipped |
| struct expressions | YES (AdtInit rvalue, no per-field walk) | YES (per-field AdtInit) | shipped-subset |
| borrow expressions | YES (Ref rvalue) | YES | shipped |
| cast expressions | YES (Cast rvalue) | YES | shipped |
| if/else expressions | YES (If terminator + Goto/join blocks) | YES (full join construction) | shipped-subset |
| while expressions | YES (header + body + after blocks) | YES | shipped-subset |
| loop expressions | YES (body + after blocks) | YES | shipped-subset |
| for expressions | YES (header + body + after, no iter-protocol) | YES (Option-yielding next() desugar) | shipped-subset |
| return expressions | YES (Return terminator) | YES | shipped |
| break expressions | YES (Goto terminator) | YES (resolves loop frame) | shipped-subset |
| continue expressions | YES (Goto terminator) | YES (resolves loop frame) | shipped-subset |
| match expressions | YES (one block per arm + SwitchInt) | YES (SwitchVariant for ADT discriminants) | shipped-subset |
| block expressions | YES | YES | shipped |
| ? operator (Question) | NO (Use rvalue only) | YES (SwitchVariant + TryReturnErr) | deferred |
| agent decls | NO | YES (state struct + ctor + handlers) | deferred (post-v0.9) |
| protocol decls | NO | NO (parsed but no IR emission) | parity |
| supervisor decls | NO | NO | parity |
| send / ask expressions | NO | YES (Send / Ask rvalues) | deferred |
| spawn expressions | NO | YES (AgentSpawn rvalue) | deferred |
| arena scopes | NO | YES (ArenaPush/Pop stmts) | deferred |
| budget blocks | NO | NO (host-side) | parity |
| sandbox blocks | NO | NO (host-side) | parity |
| cap value literals | NO | YES (CapValue rvalue) | deferred |
| async suspension | NO | YES (Suspend terminator) | deferred |
| lambda expressions | NO | YES (synthesized fn) | deferred |
| if-let expressions | NO | YES (desugar) | deferred |
| pattern matching | NO (just block order) | YES (full pat lowering) | deferred |
| StorageLive/Dead insertion | YES (per Let) | YES (precise scope-tracked) | shipped-subset |
| Drop insertion | NO | YES (borrow-checker-driven) | deferred |
| temp local allocation | NO (no rvalue → local linkage) | YES | deferred |

## Bootstrap test coverage

`cargo test -p mty-driver --test selfhost_ir`:

```
test selfhost_ir_compiles ........... ok
test selfhost_ir_lib_compiles ....... ok
test selfhost_ir_nodes_compiles ..... ok
test selfhost_ir_hello_world ........ ok
test selfhost_ir_example_01 ......... ok
test selfhost_ir_example_02 ......... ok
test selfhost_ir_example_03 ......... ok
test selfhost_ir_example_04 ......... ignored (v0.9 — ? operator + TryReturnErr)
test selfhost_ir_example_05 ......... ignored (v0.9 — range patterns + match guards)
```

The diff is **lenient** at v0.9: we verify (a) every fn the Rust IR
produces also gets produced by the Mighty IR, (b) every Mighty-emitted
fn ends with a Return terminator, (c) BB-counts diverge by at most 20.
Stricter per-BB statement-kind diffs land in v1.0 work once the v0.9
deferred shapes (Drop insertion, temp local linkage, pattern lowering)
are wired in.

## Gaps encountered (and workarounds applied)

### 1. Single-file compile blocks module layout (carried from v0.6/v0.8)

**Symptom:** `mty check` still compiles one `.mty` file at a time; cross-
file `use selfhost_ir.IrFn` resolution is not wired up.

**Workaround:** Consolidate runnable code into `lower.mty`; keep
`lib.mty` + `nodes.mty` as documentation files exercising `package`
declarations. Same workaround as v0.4 lexer / v0.6 parser / v0.8 HIR.

**Recommended language fix:** Land `mty-pkg` cross-file resolution
(open since v0.7).

### 2. No la_arena equivalent — arenas as Vec<T> with USize IDs (carried)

**Symptom:** Rust IR uses `la_arena::Arena<T>` keyed by `Idx<T>`.
Mighty doesn't have it; parametric newtypes that erase to `USize`
aren't in the v0.8 type system.

**Workaround:** All IDs are bare `USize`. `SENTINEL_NONE = 4294967295`
stands in for `Option<Id>`. Host allocator generates incrementing IDs
on `ir_emit_*` calls; the Mighty side passes them around opaquely.

**Recommended language fix:** v0.10 should ship newtype syntax (e.g.
`type IrFnId = USize newtype`).

### 3. No rvalue → local linkage emit

**Symptom:** The Rust IR lowerer assigns every rvalue to a fresh temp
local before consuming it (so `let x = a + b` becomes `_tmp =
BinOp(Add, a, b); x = Use(_tmp)`). The v0.9 self-host emits the
rvalue marker into the event stream but doesn't open a temp; this is
why the per-BB statement-kind diff is intentionally not asserted.

**Workaround:** Emit `ir_emit_rvalue(kind)` events; the bootstrap
test counts them informationally only.

**Recommended language fix:** Land a 4th sink event
`ir_emit_assign_temp(rvalue_kind, ty_kind)` plus per-fn temp allocator
state on the Mighty side. Estimated +150 LOC.

### 4. Match-arm pattern lowering deferred

**Symptom:** The Rust IR translates each match arm into a Switch
terminator + per-arm BB + guard checks. The v0.9 self-host emits
`SwitchInt` (regardless of discriminant type) + one BB per arm body,
but doesn't lower the patterns themselves into binding/guard stmts.
This is why example 02's `area` fn produces 11 BBs in Rust vs 4 in
Mighty.

**Workaround:** Bootstrap test accepts up to 20-block delta per fn.

**Recommended language fix:** Port `crates/mty-ir/src/lower/pats.rs`
(348 LOC) to Mighty. Needs additional HIR-query bridge for arm
patterns + guards. Estimated +400 LOC.

### 5. ? operator (Question expr) → TryReturnErr deferred

**Symptom:** `expr?` lowers in the Rust IR to a SwitchVariant on the
Result discriminant followed by a `TryReturnErr` terminator on the
Err arm. The v0.9 self-host emits a `Use` rvalue and continues. This
is why example 04 is `#[ignore]`'d.

**Workaround:** None — examples that rely on `?` are deferred.

**Recommended language fix:** Add `ir_emit_terminator("TryReturnErr")`
on the Mighty side and detect `Question` exprs explicitly. Estimated
+30 LOC self-host + +20 LOC bridge.

### 6. Agent / send / ask / spawn deferred

**Symptom:** Slice-7 surfaces (agents, mailboxes, send/ask, spawn,
arenas, budget, sandbox) all lower into IR via the Rust pipeline.
The v0.9 self-host doesn't emit any of those.

**Workaround:** None — these are post-v0.9 work. Examples 07-15 are
already not in the v0.9 scope.

**Recommended language fix:** Slice-by-slice: agent (+200 LOC),
send/ask (+100 LOC), arena (+80 LOC), budget/sandbox (+60 LOC).

### 7. Temp local allocation deferred

**Symptom:** Rust IR allocates temps for intermediate computation;
the Mighty side only emits param + return locals.

**Workaround:** Bootstrap diff doesn't compare local counts.

**Recommended language fix:** Combined with gap #3 (rvalue → local
linkage).

### 8. Drop insertion deferred

**Symptom:** Rust IR inserts `Drop(local)` statements at scope exit
based on the borrow checker's analysis. The v0.9 self-host doesn't
have a borrow analyzer; it just emits `StorageLive` per Let.

**Workaround:** None — drops are absent from the event stream.

**Recommended language fix:** Port (a portion of) the borrow-tracking
analysis to Mighty. Estimated +300 LOC and depends on gap #3.

## Post-v0.9 roadmap

Ordered roughly by impact-per-LOC:

1. **rvalue → local linkage + temp allocator** (gap #3, 7) — unblocks
   precise per-BB statement-kind diff. ~150 LOC.
2. **Match-arm pattern lowering** (gap #4) — unblocks tighter BB-count
   diff on examples 02 + 05. ~400 LOC.
3. **? operator (TryReturnErr)** (gap #5) — un-ignores example 04.
   ~50 LOC total.
4. **Cross-file modules** (gap #1) — language fix that unblocks
   splitting selfhost packages cleanly. Big project (mty-pkg work).
5. **Agent + send/ask** (gap #6) — un-ignores examples 07-09. ~300
   LOC.
6. **Arena + budget + sandbox** (gap #6) — un-ignores examples 11-12,
   18. ~150 LOC.
7. **Drop insertion** (gap #8) — required for byte-identical diff with
   Rust IR. ~300 LOC, depends on (1).

## Why ship a subset?

Self-hosting is a milestone of intent as much as runtime behavior.
Shipping `lower.mty` now:

- locks in MtyIR's *semantic surface* in Mighty syntax, letting future
  versions diff against a fixed spec when they reorganize the Rust
  implementation;
- exercises the language at IR-level complexity and surfaces the gaps
  (cataloged above) before they accumulate further;
- is faithful to the brief's "ship a SUBSET if you hit gaps, document
  them" working agreement;
- means the v1.0 follow-up is bounded: tackle gaps 1-8 in order and
  every example flips green deterministically.
