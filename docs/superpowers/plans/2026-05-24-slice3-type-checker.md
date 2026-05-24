# Slice 3 — Type Checker MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working Hindley-Milner type checker for Stardust that handles all 20 canonical examples, with first-order generics, the `Result[T,E]` sugar, `?` propagation, and pub-signature validation. Effect/capability signatures are *carried* but not enforced. Ownership/borrow checking is deferred to slice 4.

**Architecture:** New `sdust-types` crate. Types are interned arena values. Name resolution maps HIR types/paths to a `DefMap`. Inference is bidirectional with constraint-style unification + occurs check. Prelude provides built-ins (Option, Result, primitives, intrinsic methods/types referenced by examples).

**Tech Stack:** Rust 1.82, la-arena, rustc-hash (transitive). Same workspace as slice 2.

---

## File Structure

**Create:**
- `crates/sdust-types/Cargo.toml`
- `crates/sdust-types/src/lib.rs` — re-exports
- `crates/sdust-types/src/ty.rs` — `TyData`, `TyId`, `IntKind`, `FloatKind`, `TyArena`, interner
- `crates/sdust-types/src/defs.rs` — `AdtDef`, `FnDef`, `DefMap`, `DefRef`, `BuiltinDef`
- `crates/sdust-types/src/prelude.rs` — build `std.core` (Option, Result, primitives, intrinsics, opaque modules + types)
- `crates/sdust-types/src/resolve.rs` — `resolve_hir_type`, `resolve_value_path`, build `DefMap` from HIR
- `crates/sdust-types/src/infer.rs` — `InferCtx`, `Substitution`, `unify`, `occurs_check`, defaulting
- `crates/sdust-types/src/check.rs` — `check_expr` / `synth_expr`, statement checking, block tail
- `crates/sdust-types/src/items.rs` — top-level item checker; pub-signature validator
- `crates/sdust-types/src/diag.rs` — SD2xxx diagnostic constructors
- `crates/sdust-types/tests/primitives.rs`
- `crates/sdust-types/tests/generics.rs`
- `crates/sdust-types/tests/result_question.rs`
- `crates/sdust-types/tests/examples.rs`
- `crates/sdust-types/tests/negatives.rs`
- `tests/typeck_neg/*.sd` — 15 negative test inputs
- `docs/internals/typeck.md` — internals doc
- `SLICE3.md` — slice summary

**Modify:**
- `Cargo.toml` — add `crates/sdust-types` to members
- `crates/sdust-driver/Cargo.toml` — depend on `sdust-types`
- `crates/sdust-driver/src/pipeline.rs` — add `type_check(pkg) -> Vec<Diagnostic>` stage
- `crates/sdust-cli/src/cmd/check.rs` — invoke type-check stage
- `crates/sdust-diagnostics/src/codes.rs` — add SD2001..SD2025 + explain entries
- `crates/sdust-hir/src/lower/items.rs` — capture struct/enum/fn generics into HIR (currently dropped); preserve `effect` clauses (already done); add `agent` ctor param types
- `crates/sdust-hir/src/nodes.rs` — fn/struct/enum `generics: Vec<HirGenericParam>` (was `Vec<String>`)
- `examples/06_for_while_loop.sd` — return `Unit!WorkErr` so `?` is legal
- `examples/11_budget_block.sd` — return `Unit!RunErr` (was `Result!RunErr`)
- `docs/spec/v0.1-amendments.md` — add A7 (`?` strictly), A8 (integer defaults), A9 (`String` as type+fn), A10 (built-in method table)
- `docs/reference/cli/sdust-check.md` — new
- `docs/reference/diagnostics.md` — new SD2xxx section
- `docs/tour/01-hello.md` through `docs/tour/12-extern.md` — add "Type errors you might see" subsections (where relevant)
- `README.md` — roadmap mark slice 3 shipped
- `SLICE2.md` — strike closed deferrals

---

## Task 1: Scaffold `sdust-types` crate

**Files:** `crates/sdust-types/Cargo.toml`, `crates/sdust-types/src/lib.rs`, root `Cargo.toml`

Add the crate to the workspace. Dependencies: `sdust-hir`, `sdust-diagnostics`, `la-arena`. Re-export the top-level entry point.

```rust
// lib.rs
pub mod check;
pub mod defs;
pub mod diag;
pub mod infer;
pub mod items;
pub mod prelude;
pub mod resolve;
pub mod ty;

pub use defs::*;
pub use ty::*;

use sdust_diagnostics::Diagnostic;
use sdust_hir::Package;

/// Full type-check entry point. Builds prelude, resolves names, type-checks
/// every fn body, validates pub signatures. Returns a Vec of diagnostics
/// (errors + warnings).
pub fn check_package(pkg: &Package) -> Vec<Diagnostic> {
    items::check(pkg)
}
```

- [ ] Create `crates/sdust-types/Cargo.toml`
- [ ] Create `crates/sdust-types/src/lib.rs` with module skeletons
- [ ] Update root `Cargo.toml` workspace members
- [ ] `cargo build -p sdust-types` succeeds

## Task 2: `Ty` representation + interner

**Files:** `crates/sdust-types/src/ty.rs`

Define `TyData`, `TyId`, `IntKind`, `FloatKind`. Implement `TyArena` with a hashmap interner so equal `TyData` collapses to the same `TyId`. Provide pretty-printer `pretty_ty(ty: TyId, arena: &TyArena) -> String`.

`TyVarId` is a separate `u32` newtype. Inference variables live in a `Substitution` (Vec<Option<TyId>>), not in the arena (so substitution-update doesn't need to mutate Ty objects).

- [ ] Add `TyData` enum, `IntKind`, `FloatKind`, `EffectId`
- [ ] `TyArena { types: Arena<TyData>, intern: HashMap<TyData, TyId> }` with `intern(TyData) -> TyId`
- [ ] Prebuilt `TyId`s for primitives via `TyArena::new_with_primitives()`
- [ ] `pretty_ty`
- [ ] Unit tests: interning works, pretty-print covers all variants

## Task 3: `AdtDef`, `FnDef`, `DefMap`

**Files:** `crates/sdust-types/src/defs.rs`

```rust
pub struct AdtDef { name, kind: AdtKind, generics: Vec<ParamDef>, variants: Vec<VariantDef> }
pub struct VariantDef { name, fields: Vec<FieldDef> }
pub struct FieldDef { name: Option<String>, ty: TyId }
pub struct FnDef { name, generics, params: Vec<(String, TyId)>, ret: TyId, effects: Vec<EffectId>, is_pub: bool, body: Option<sdust_hir::BlockId> }
pub struct DefMap { adts, fns, by_name, type_aliases, builtin_methods }
pub enum DefRef { Adt(AdtId), Fn(FnDefId), Variant(AdtId, usize), Builtin(BuiltinDef) }
pub enum BuiltinDef { OpaqueModule(String), OpaqueType(AdtId), Fn(FnDefId) }
```

- [ ] Define all structs + enums
- [ ] `DefMap::lookup(name) -> Option<DefRef>`
- [ ] `DefMap::lookup_path(segments) -> Option<DefRef>` — handles `std.http` style multi-segment paths via `OpaqueModule` traversal
- [ ] Unit tests for lookups

## Task 4: Prelude builder

**Files:** `crates/sdust-types/src/prelude.rs`

Implement `build_prelude(arena: &mut TyArena, defs: &mut DefMap) -> PreludeIds`. Adds:

- All primitive type aliases (`Bool`, `I8..I128`, `U8..U128`, `USize`, `ISize`, `F32`, `F64`, `Char`, `Str`, `String`, `Bytes`, `Unit`, `Never`)
- `Option[T]`, `Result[T, E]` as ADTs
- Opaque modules: `std`, `std.core`, `std.http`, `std.json`, `std.dom`, `std.trace`
- Opaque types referenced in examples: `Url`, `Page`, `IoErr`, `NetErr`, `ParseErr`, `FetchErr`, `Logger`, `Fetcher`, `Lowered`, `RunErr`, `Fs`, `Path`, `Net`, `Model`, `Dom`, `MainErr`, `SearchErr`, `Json`, `Map`, `Config`, `ConfigErr`, `WorkErr`, `Planner`, `Tokens`, `Ast`, `AgentRef`
- Builtin fns: `log: fn(Str) -> Unit effect io`, `panic: fn(Str) -> Never`, `spawn: fn[T](T) -> AgentRef[T]`, `move: fn[T](T) -> T`, `raw_ptr: fn(USize) -> *U8`, `null: U8` (a value), `valid: fn(*U8, USize) -> Bool`, `fetch: fn(Url) -> Page!NetErr`, `parse: fn(Str) -> I32!ParseErr` — wait, `parse` collides with example 04's user `fn parse`. **Resolution:** only register `parse` etc. if user hasn't shadowed. Use a "weak intrinsic" mechanism: prelude items are added after user items; user names shadow.
- Builtin method table: a `HashMap<(BuiltinReceiverShape, String), BuiltinMethod>` where `BuiltinReceiverShape` enumerates: Array, Slice, String, Bytes, Opaque(AdtId). Methods include `len`, `to_str`, `get(key) -> Option[V]`, `ok_or(err) -> Result[Self, E]`, `read`, `write`, `embed`, `post`, `query`, `set_text`, `on`, `encode`, `ok`, `serve`, `restart`, `backoff`.

To keep this manageable, methods that aren't shape-specific (e.g. `.encode`, `.embed`) get a permissive type `fn(&self, ...args: any) -> Var` — the args take a "wildcard" type that unifies with anything, and the return is a fresh inference variable.

- [ ] Implement prelude builder
- [ ] Implement weak-intrinsic shadowing
- [ ] Implement builtin method table
- [ ] Unit tests for prelude contents

## Task 5: Type resolution (HirType → TyId)

**Files:** `crates/sdust-types/src/resolve.rs`

```rust
pub fn resolve_hir_type(ty: &HirType, pkg: &Package, defs: &DefMap, arena: &mut TyArena, locals: &ParamScope, diag: &mut Vec<Diagnostic>) -> TyId
```

Cases:
- `HirType::Path { segments, generics }` — look up in `defs`; if generic param in scope, return `Param`; if ADT, check arity, build `Adt(id, resolved_generics)`; if type alias, return its `TyId` (eagerly expanded); if not found, emit `SD2002` and return `Ty::Error`.
- `HirType::Borrow { mutable, inner }` → `Ref { mutable, inner: resolve(inner) }`
- `HirType::Tuple(xs)` → `Tuple(xs.map(resolve))`
- `HirType::Array { elem, len }` → `Array { elem: resolve(elem), len: const_eval_len(len) }`. `const_eval_len` returns `None` if `len.is_none()` or len can't be const-evaluated (slice 3: only literal integers).
- `HirType::Fn { params, ret }` → `Fn { params, ret, effects: [] }`
- `HirType::Result { ok, err }` → `Adt(result_id, [resolve(ok), resolve(err)])`
- `HirType::Union(_)` → `Adt(result_id, [_, Ty::Error])` (slice 3 doesn't model union-of-errors)
- `HirType::Unit` → `Unit`
- `HirType::Unknown` → fresh `Ty::Var`

`ParamScope` holds generic params in scope (`Vec<(String, ParamId)>`).

- [ ] Implement `resolve_hir_type`
- [ ] Tests: primitives, generics, Result, Borrow, Fn, Array
- [ ] `SD2002` emitted for unknown types
- [ ] `SD2004` emitted for arity mismatch

## Task 6: Generic param capture in HIR

**Files:** `crates/sdust-hir/src/nodes.rs`, `crates/sdust-hir/src/lower/items.rs`, `crates/sdust-hir/src/lower/agents.rs`

Currently fn/struct/enum/agent/protocol/trait `generics: Vec<String>` — but lowering writes `vec![]`! Walk the CST for `GENERIC_PARAMS` and capture identifier list. Same for trait impl methods.

- [ ] Add `lower_generics(node: SyntaxNode) -> Vec<String>` helper
- [ ] Call it from `lower_fn`, `lower_struct`, `lower_enum`, `lower_type_alias`, `lower_protocol`, `lower_trait`, `lower_impl`
- [ ] Tests: 03_generic_fn dump shows `T`; struct `Box[T]` dump shows `T`

## Task 7: Build DefMap from HIR

**Files:** `crates/sdust-types/src/resolve.rs`

```rust
pub fn build_def_map(pkg: &Package, arena: &mut TyArena) -> (DefMap, Vec<Diagnostic>)
```

Walks `pkg.top_level`. Two passes:
1. **Declare**: for each ADT, allocate an `AdtId` and a placeholder `AdtDef { name, kind, generics: [], variants: [] }`. For each fn, allocate `FnDefId` placeholder.
2. **Resolve**: with all names visible, fill in generics + variants + fn params/ret.

This two-pass approach lets struct A reference struct B even if B is declared later. The prelude is built first (Task 4) so its defs are already in `DefMap` when user items are added.

Type aliases: resolve eagerly. (Slice 3 doesn't allow cyclic aliases; no check.)

Agents: register the agent as an opaque ADT named `<AgentName>`. Methods on the agent become FnDefs.

- [ ] Two-pass implementation
- [ ] Agent handling
- [ ] Impl-block method handling
- [ ] Test against 02_struct_enum, 03_generic_fn

## Task 8: InferCtx + Substitution + unify

**Files:** `crates/sdust-types/src/infer.rs`

```rust
pub struct Substitution(Vec<Option<TyId>>);
impl Substitution {
    pub fn fresh_var(&mut self) -> TyVarId;
    pub fn resolve(&self, ty: TyId, arena: &TyArena) -> TyId;     // walk vars to representative
    pub fn bind(&mut self, var: TyVarId, ty: TyId);
}

pub fn unify(a: TyId, b: TyId, subst: &mut Substitution, arena: &mut TyArena, span: SourceSpan, diag: &mut Vec<Diagnostic>) -> Result<(), ()>
pub fn occurs_check(var: TyVarId, ty: TyId, subst: &Substitution, arena: &TyArena) -> bool
```

`unify` walks both args through `resolve`, then dispatches on the resolved pair. Implements all the cases from design §3.6. Emits `SD2001` (type_mismatch) on failure.

- [ ] Substitution + resolve
- [ ] `unify` with all variant cases
- [ ] Occurs check
- [ ] `IntInfer`/`FloatInfer` defaulting at end of fn body
- [ ] Unit tests: 15+ unification scenarios

## Task 9: Bidirectional expr checking — literals, paths, blocks, if

**Files:** `crates/sdust-types/src/check.rs`

Two functions:
```rust
pub fn synth_expr(cx: &mut Cx, expr: ExprId) -> TyId
pub fn check_expr(cx: &mut Cx, expr: ExprId, expected: TyId)
```

Where `Cx` holds the inference ctx + def map + arena + locals scope + return type.

Implement for:
- Literals: int → `Int(IntInfer)`, float → `Float(FloatInfer)`, str → `Str`, char → `Char`, bool → `Bool`, duration → `Duration` (opaque), size → `Size` (opaque)
- Path: look up in locals scope first, then `defs.by_name`. Resolve to `TyId`.
- Block: synth tail (or unit if no tail). Check stmts in order.
- If: check cond is `Bool`. Synth then-branch; if else-branch exists, unify; else result is `Unit`.
- IfLet: same shape; bind pattern against scrutinee.
- Return: unify expr ty with `cx.return_ty`. Result is `Never`.

- [ ] Implement `synth_expr`/`check_expr` for these
- [ ] Local scope chain (push/pop on block)
- [ ] Pattern binding pushes locals
- [ ] Tests against 01_hello, 05_match_expr

## Task 10: Calls + generic instantiation

**Files:** `crates/sdust-types/src/check.rs`

For `HirExpr::Call { callee, args }`:
1. Synth `callee` type.
2. If callee resolves to a `Fn { params, ret, .. }` with no `Param` slots: arity-check, unify each arg.
3. If callee resolves to a generic fn: instantiate with fresh vars (or turbofish args from `PathGeneric`).
4. If callee is `Path(["Some"])` etc: special-case Option/Result constructors.
5. If callee is not callable: `SD2008`.

For `HirExpr::PathGeneric { segments, generics }`: resolve segments + check generic args.

- [ ] Implement Call
- [ ] Implement PathGeneric
- [ ] Implement Some/None/Ok/Err special-casing
- [ ] Tests against 03_generic_fn, 04_result_propagation

## Task 11: Struct literals, field access, method calls

**Files:** `crates/sdust-types/src/check.rs`

- `Struct { path, fields }`: resolve path to ADT, instantiate generics, type-check each field expr against declared field type, error if missing/duplicate/unknown fields.
- `Field { receiver, name }`: synth receiver, resolve field name on type.
- `MethodCall { receiver, method, args }`:
  - Synth receiver type.
  - Look up user-defined impl methods (slice 3 doesn't have full coherence; just match `impl T { fn method }` or `impl Trait for T { fn method }`).
  - If not found, consult builtin method table.
  - If still not found and receiver is `Ty::Error` or `Ty::Var`: silently return fresh var.
  - Else `SD2007`.

- [ ] Struct literal checking
- [ ] Field access
- [ ] Method call resolution
- [ ] Tests

## Task 12: Match + patterns

**Files:** `crates/sdust-types/src/check.rs`

`Match { scrutinee, arms }`:
1. Synth scrutinee type S.
2. For each arm: check pattern against S (binds locals); check body against expected.
3. If expected unknown: arms must unify to a common type.
4. Compute (warning-only) exhaustiveness.

Pattern handling (`check_pattern(pat, ty)`):
- `Wildcard` — ok, binds nothing
- `Literal` — unify literal type with `ty`
- `Binding { name, sub }` — push local with `ty`, recurse if `sub`
- `Tuple(xs)` — `ty` must be `Tuple(...)`, zip
- `Struct { path, fields }` — `ty` must be the named struct, check each
- `Enum { path, args }` — `ty` must be the enum, args zip variant payload
- `Range { lo, hi, .. }` — endpoints unify with `ty`
- `Ref { mutable, inner }` — `ty` must be `Ref { mutable, _ }`, recurse on inner

- [ ] Pattern checking
- [ ] Match arm unification
- [ ] Exhaustiveness warning (slice-3: warning only)
- [ ] Tests against 02_struct_enum, 05_match_expr

## Task 13: `?` operator + Result handling

**Files:** `crates/sdust-types/src/check.rs`

`HirExpr::Question(inner)`:
1. Synth inner type.
2. Resolve: must be `Adt(result_id, [t, e])`. If not, `SD2010`.
3. Check `cx.return_ty` resolved is `Adt(result_id, [_, e'])`. If not, `SD2010`.
4. Unify e with e'. If fail, `SD2011`.
5. Result is `t`.

- [ ] Implement
- [ ] Tests against 04_result_propagation

## Task 14: Item-level checking + pub-signature validator

**Files:** `crates/sdust-types/src/items.rs`

```rust
pub fn check(pkg: &Package) -> Vec<Diagnostic>
```

1. Build `TyArena`, build prelude, build `DefMap`.
2. Validate pub signatures: walk `defs.fns`; if pub, every param must have explicit ty (i.e. `HirParam.ty.is_some()`).
3. For each fn with a body: type-check body against ret.
4. For each agent: type-check state initializers, methods, and message handlers.
5. Return collected diagnostics.

For handlers: bind handler-arg names to `Ty::Var` (or to the protocol-message arg type if resolved). Check body's tail unifies with the protocol's reply type (or `Ty::Var`).

- [ ] Pub-signature check
- [ ] Fn body checking
- [ ] Agent body checking
- [ ] Supervisor (slice 3: skip the strategy/child decls — they're not exprs in the normal sense)

## Task 15: Diagnostic codes + builders

**Files:** `crates/sdust-types/src/diag.rs`, `crates/sdust-diagnostics/src/codes.rs`

Add `SD2001..SD2025` constants. Add `explain` entries. Diag builder functions like `mismatch(expected, found, span) -> Diagnostic`.

- [ ] Add codes
- [ ] Add explain entries
- [ ] Builder fns

## Task 16: Wire into `sdust-driver`

**Files:** `crates/sdust-driver/Cargo.toml`, `crates/sdust-driver/src/pipeline.rs`, `crates/sdust-driver/src/lib.rs`

```rust
pub fn type_check(pkg: &Package) -> Vec<Diagnostic> { sdust_types::check_package(pkg) }
```

Update `pipeline.rs` to expose `check(pkg) -> Vec<Diagnostic>`. `lower()` returns the same as before; CLI calls `lower` then `type_check`, concatenating diagnostics.

- [ ] Add dep
- [ ] Re-export
- [ ] Tests against 01_hello smoke

## Task 17: Wire into CLI `sdust check`

**Files:** `crates/sdust-cli/src/cmd/check.rs`, `crates/sdust-cli/Cargo.toml`

```rust
let (pkg, mut diags) = lower(&parsed);
diags.extend(type_check(&pkg));
```

- [ ] Update check.rs
- [ ] CLI integration test
- [ ] Make sure error-vs-warning split works (warnings don't fail the build; only errors return non-zero)

## Task 18: Sweep examples

**Files:** `examples/06_for_while_loop.sd`, `examples/11_budget_block.sd`

- 06: change `fn process(items: &[I32])` → `fn process(items: &[I32]) -> Unit!WorkErr` so `?` is legal.
- 11: change `Result!RunErr` → `Unit!RunErr`.

Run `sdust check` on every example; iterate until all pass.

- [ ] Apply example tweaks
- [ ] Run all 20
- [ ] Adjust prelude method-table for whatever the examples need

## Task 19: Negative test corpus

**Files:** `tests/typeck_neg/*.sd`, `crates/sdust-types/tests/negatives.rs`

15 files, each producing exactly one SD2xxx code (or marked as multi-error tolerant).

The driver:
```rust
#[test] fn mismatch_let() { expect(typeck_neg("mismatch_let.sd"), &["SD2001"]) }
```

- [ ] Write 15 .sd files
- [ ] Driver harness
- [ ] All tests green

## Task 20: Docs

**Files:** `docs/internals/typeck.md`, `docs/reference/cli/sdust-check.md`, `docs/reference/diagnostics.md`, `docs/tour/0X-*.md`, `docs/spec/v0.1-amendments.md`, `README.md`, `SLICE2.md`, `SLICE3.md`

- Internals doc covers: arch, Ty model, inference algo, prelude/tolerance, current limits
- CLI ref: `sdust check` now type-checks
- Diagnostics ref: SD2xxx table
- Tour pages where relevant get a "Type errors you might see" subsection
- Amendments: A7-A10
- README roadmap update
- SLICE2 deferrals closed
- SLICE3 summary

- [ ] All docs written
- [ ] No links broken

## Task 21: Final acceptance

- [ ] `cargo test --workspace` green (250+ tests)
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] All 20 examples `sdust check` clean
- [ ] Tag `v0.3.0-typeck`; push tag

---

## Execution notes for the slice leader

- Skip per-task review. Rely on `cargo test --workspace` after each task.
- Use sonnet for implementer tasks (parser/typeck logic).
- Use haiku for cargo-toml edits, code-formatting, simple text moves.
- After tasks 5/8/12/16 (each a major milestone) push to main.
- If the test count plateaus or drops, inspect — do *not* mass-disable tests.
- If a parser bug blocks an example, fix the parser inline (small fix only).
- Budget: ~6 hours subagent runtime. If running long after task 18, ship PARTIAL.
