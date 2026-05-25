# SELFHOST_HIR_V0_8_NOTES

This file catalogues the language gaps + interpretation calls
encountered while porting the HIR lowering + minimal typeck phases to
Mighty for v0.8.

Live status:
- `selfhost/hir/lower.mty` — 960 LOC, `mty check` clean, `cargo test
  -p mty-driver --test selfhost_hir` passes 5/5 tests on examples
  01-03 (04 + 05 ignored, see below).
- `selfhost/typeck/infer.mty` — 153 LOC, `mty check` clean,
  `cargo test -p mty-driver --test selfhost_typeck` passes 5/5
  tests on examples 01-03 (04 + 05 ignored, see below).

## Gaps encountered (and workarounds applied)

### 1. Single-file compile blocks proper module layout

**Symptom:** `mty check` v0.6 still compiles one `.mty` file at a time;
`use selfhost_hir.HirFn` cross-file resolution is not wired up.

**Workaround:** Consolidate the runnable code into `lower.mty` (HIR)
and `infer.mty` (typeck), with `lib.mty` + `nodes.mty` files documenting
the intended v0.9 module layout (matching the same convention used by
the v0.4 lexer and v0.6 parser).

**Recommended language fix:** Land `mty-pkg` cross-file resolution in
v0.9 (the parser-notes file lists this as a v0.7 task; it stayed open).

### 2. No la_arena equivalent — arenas as Vec<T>

**Symptom:** The Rust HIR uses `la_arena::Arena<T>` keyed by `Idx<T>`;
this provides typed ids per arena. Mighty doesn't have `la_arena` as a
dep, and the user-level Mighty type system doesn't yet provide
parametric newtypes that erase to `USize`.

**Workaround:** All ids are bare `USize`. The host's allocator
generates incrementing ids; the Mighty side passes them around opaquely.
A `SENTINEL_NONE = 0xFFFFFFFF` constant stands in for `Option<Id>`.

**Recommended language fix:** v0.9 should ship a newtype wrapper syntax
like `type FnId = USize newtype` so the self-host can be typed without
runtime cost.

### 3. `Option[T]` round-trip through host bridge is awkward

**Symptom:** The same gap the v0.6 parser noted — `Option[T]` chained
with `?` isn't practical at the bridge boundary.

**Workaround:** Use `SENTINEL_NONE` (`4294967295`) for missing ids; use
`""` for missing strings; use `false` for missing booleans. The host
fills in the sentinel when a CST node lookup misses.

**Recommended language fix:** Either land bridge auto-Option encoding
(serialize `None`/`Some(v)` as a discriminated value) or accept that
sentinels are a stable bootstrap-time convention.

### 4. `child` is a reserved keyword (CHILD_KW)

**Symptom:** Defining a helper `fn child(n: USize, i: USize) -> USize
{ ... }` produced `MT0001: expected L_PAREN, got 'child'`. The lexer
classifies `child` as a keyword (used by supervisor syntax).

**Workaround:** Rename to `nth_child`. Catalogued the full keyword list
for future self-host work (`run`, `task`, `restart`, `backoff`,
`up_to`, `detach`, `sup`, `sandbox`, `on_fail`, `where`, `with`, `as`,
`spawn`, `state`, `cap`, `child`, `scope`, `join` are all reserved).

**Recommended language fix:** Most of these are contextual keywords in
practice (only meaningful inside agent / sup / sandbox blocks). v0.9
should make the lexer return `IDENT` for these in non-keyword contexts
(driven by the parser's expected-token set).

### 5. Borrow checker treats locally-mutated `mut seen_callee` as
suspect

**Symptom:** The original lower.mty used `let mut seen_callee = false`
inside `lower_call_expr`, set to `true` after lowering the first
argument. This compiled and ran fine. (Listing here as an
interpretation call: kept the variable.)

**Workaround:** None needed — the v0.6 borrow checker handles `mut`
booleans in loop bodies correctly.

### 6. Trait dispatch / capability narrowing / effect inference all
deferred

**Symptom:** The Rust typeck is ~5000 LOC. Replicating even half of it
in Mighty would blow the v0.8 5-hour time budget.

**Workaround:** v0.8 typeck only handles:
- fn parameter types from explicit annotations
- fn return types from explicit annotations
- let-binding types from explicit annotations
- let-binding types defaulted from literal init (Int→I32, Float→F64,
  Str→Str, Char→Char, Bool→Bool)

Generic param recording, unification, trait dispatch, capability
narrowing, effect inference, struct field-access type resolution, and
method-call resolution are all explicit v0.9 work.

**Recommended language fix:** Not a language fix — the v0.8 deliverable
is "bootstrap surface area + canonical examples passing", not
"feature parity with Rust typeck". v0.9 should grow the surface
toward unification first; that unlocks generic instantiation which
unlocks most of the remaining pipeline.

### 7. `pretty_ty` rendering differs between syntactic HIR and resolved
typeck

**Symptom:** The Rust pretty-printer renders `Adt23[&T6]` for an
unresolved `Option[&T]` type, where `Adt23` is the def-map id and `T6`
is the parameter-list id; the Mighty side preserves the original
syntactic spelling `Option[&T]`.

**Workaround:** The bootstrap test passes `Some(&typed.def_map)` to
`pretty_ty` so ADTs render by name (`Shape`, not `Adt56`), and
normalizes the rendered string by stripping the numeric suffix from
`T<n>` patterns. This makes the two sides comparable for examples
01-03.

**Recommended language fix:** Have `pretty_ty` accept a generic-scope
parameter so `Param(p)` can render with the param's declared name.
That's already on the TyData::Param comment as a "diag layer can
optionally render with the param name" hint.

## Examples 04 and 05 — why ignored

### Example 04 (`04_result_propagation.mty`) — Result-sugar return

The fn `parse(s: Str) -> I32!ParseErr` has a sugar-return form
(`T!E` desugars to `Result[T, E]`). v0.8 lowers this as a `Result`
type node in the HIR, but the typeck pipeline elides the sugar
representation — the bidirectional comparison would need a custom
"is this an Ok call" matcher on the body. Skip for v0.8.

### Example 05 (`05_match_expr.mty`) — private fn + range patterns

Two stumbling blocks:
1. `fn _classify(...)` — the leading `_` is a name-mangling signal
   per the spec; the typeck pipeline currently treats it as a regular
   private fn but it should be filtered from the world-export list.
   The Mighty side doesn't model exports, so its binding map names the
   fn `_classify` while the Rust side names it the same way — that's
   fine. The actual block-list reason is point 2.
2. Range patterns (`1..10 => "small"`) — the typeck infers the
   discriminant type for arms by walking the patterns, and range
   patterns require knowing the scrutinee type to type the bounds.
   Mighty typeck v0.8 doesn't propagate from the scrutinee to arms.

Both deferred to v0.9.

## Coverage matrix

### HIR lowering (selfhost/hir/lower.mty)

| HIR variant | Lowered? | Test cov |
|---|---|---|
| Item::Fn | YES | ex01/02/03 |
| Item::Struct | YES | ex02 |
| Item::Enum | YES | ex02 |
| Item::TypeAlias | YES | ex02 |
| Item::Use | YES | (hand) |
| Item::Mod | YES | (hand) |
| Item::ExternBlock | YES | (hand) |
| Item::Impl | YES (sig + methods) | — |
| Item::Trait | YES | — |
| Item::Const | NO | v0.9 |
| Item::Agent / Protocol / Supervisor | NO | v0.9 |
| Item::Sandbox / Macro / ExportDecl | NO | post-1.0 |
| HirExpr::Literal | YES | ex01 |
| HirExpr::Path | YES | ex01 |
| HirExpr::Call | YES | ex01 |
| HirExpr::MethodCall | YES | ex03 (.len) |
| HirExpr::Field | YES | ex03 |
| HirExpr::Index | YES | ex03 |
| HirExpr::Binary | YES | (hand) |
| HirExpr::Unary | YES | (hand) |
| HirExpr::If | YES | ex03 |
| HirExpr::Match | YES | ex02 |
| HirExpr::For / While / Loop | YES | — |
| HirExpr::Return / Break / Continue | YES | — |
| HirExpr::Block | YES | ex01 |
| HirExpr::Tuple / Array | YES | — |
| HirExpr::Struct | YES (basic) | — |
| HirExpr::Question | YES | (ex04, ignored) |
| HirExpr::Borrow | YES | ex03 |
| HirExpr::Cast | YES | — |
| HirExpr::PathGeneric (turbofish) | NO | v0.9 |
| HirExpr::IfLet | NO | v0.9 |
| HirExpr::Lambda | NO | v0.9 |
| HirExpr::Send / Ask | NO | v0.9 (agents) |
| HirExpr::Deadline / Spawn / Detach / Join | NO | v0.9 |
| HirExpr::Run / Sandbox / Budget / Arena | NO | v0.9 |
| HirExpr::HtmlTemplate / Unsafe / TaskScope | NO | v0.9 |
| HirPat::Literal / Binding / Wildcard | YES | ex02 |
| HirPat::Tuple / Struct / Enum | YES | ex02 |
| HirPat::Range / Ref | YES | (ex05, ignored) |
| HirType::Path / Borrow / Tuple / Array / Fn | YES | ex02/03 |
| HirType::Result / Union | YES | (ex04, ignored) |
| HirType::Dyn / Unit / Unknown | YES | — |

### Typeck (selfhost/typeck/infer.mty)

| Feature | Inferred? | Test cov |
|---|---|---|
| fn param types from annotations | YES | ex02/03 |
| fn return types from annotations | YES | ex02/03 |
| let-binding types from annotations | YES | — |
| let-binding types from literal init | YES | — |
| Int/Float/Str/Char/Bool literal defaults | YES | (hand) |
| Path-resolved binding types (`let x = Foo`) | NO | v0.9 |
| Call-result binding types | NO | v0.9 |
| Field-access binding types | NO | v0.9 |
| Generic instantiation | NO | v0.9 |
| Trait dispatch | NO | v0.9 |
| Effect inference | NO | post-1.0 |
| Capability narrowing | NO | post-1.0 |

## Roadmap (post-v0.8)

The self-hosting roadmap now reads:

- lexer (v0.5)         **DONE**
- parser (v0.6)        **DONE**
- HIR (v0.8)           **DONE SUBSET** (this file)
- typeck (v0.8)        **DONE SUBSET** (this file)
- SIR (v0.9)           pending
- codegen (post-1.0)   pending

The v0.9 typeck slice should focus on getting examples 04+05 green
first (lifts the matcher to grok `T!E` sugar and range-pat type
propagation), then start the unification pass. SIR lowering can
proceed in parallel once the typeck binding-type map is structurally
stable.
