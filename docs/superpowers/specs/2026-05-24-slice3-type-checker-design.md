# Mighty Slice 3 Design — Type Checker MVP

**Date:** 2026-05-24
**Status:** Approved (autonomous build — user away, slice-leader = Claude)
**Source spec:** `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` (Mighty Language Specification v0.1)
**Slice maps to:** Spec §31.3 Phase 2 — Types. First slice that gives `mty check` real semantic meaning.
**Prior slice:** `v0.2.0-phase1-polish` (commit `0368831`), summary in `SLICE2.md`.
**Repo:** `C:\Users\ihass\mighty` (remote `hassard0/stardust`).

---

## 1. Goal

Add a working type checker to Mighty. After this slice, `mty check` performs lex → parse → HIR-lower → name-resolve → type-check, and the 20 canonical examples (plus a healthy negative-test corpus) all type-check clean. The type system implements:

- Resolved types `Ty` (distinct from `HirType`) with arena allocation
- Hindley-Milner inference with bidirectional checking and constraint-style unification
- First-order generics with explicit (`::[T]`) and inferred instantiation
- Synthetic `std.core` prelude carrying built-ins: `Option[T]`, `Result[T, E]`, primitive aliases, and `log`/`panic`/`spawn`-shaped intrinsics used by the examples
- `T!E` and `T!{A, B}` desugar to `Result[T, E]` / `Result[T, A|B]` at type-checking time
- `?` operator type-checks against the enclosing fn's return `Result[_, E]`
- Effect/capability signatures are *parsed and carried in `Ty`* but **not enforced** — that's slice 5

The acceptance gate is:

- `cargo test --workspace` green (174 → 250+ tests)
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo fmt --all -- --check` clean
- All 20 examples `mty check` clean (now meaning fully type-checked)
- A negative corpus (`tests/typeck_neg/`) of ~15 hand-written .sd files exercising each SD2xxx code

## 2. Non-goals for slice 3

These belong to later slices:

- Ownership / borrow / affine checking — slice 4
- Effect closure + capability narrowing enforcement — slice 5
- Trait coherence + dispatch — slice 4 (objects) / slice 5+ (coherence)
- Type-checking inside macros — slice 6+
- Match exhaustiveness errors (only warning in slice 3) — slice 5
- Higher-rank polymorphism, GATs, dependent types — post-v0.1
- Const-generic expressions — post-v0.1
- MtyIR lowering, codegen, runtime — slices 6+

## 3. Architecture

### 3.1 Crate layout

Add a new crate `mty-types`. Rationale: the type system is large enough that putting it in `mty-hir` would force unrelated consumers (the formatter, the dumper) to compile against the inference engine. A separate crate keeps the dependency graph honest: `mty-types` depends on `mty-hir`, `mty-diagnostics`. The driver depends on `mty-types`.

```
mty-syntax → mty-ast → mty-hir → mty-types
                                          ↑
                                    mty-driver → mty-cli
```

`mty-fmt` continues to depend only on `mty-syntax` (CST-only).

### 3.2 `mty-types` module structure

```
crates/mty-types/
  Cargo.toml
  src/
    lib.rs              — re-exports
    ty.rs               — Ty, TyId, arena, IntKind/FloatKind
    interner.rs         — type interning (so unification can compare TyIds)
    prelude.rs          — synthetic std.core: Option, Result, String, primitives, log/panic/spawn
    resolve.rs          — name resolution: HirType → ResolvedTy; path → Def
    infer.rs            — TyVar, InferCtx, unify, occurs check
    check.rs            — bidirectional check_expr / synth_expr
    items.rs            — item-level: check fn/struct/enum bodies; pub-signature validation
    diag.rs             — SD2xxx diagnostic builders
  tests/
    primitives.rs       — literal inference, arithmetic
    generics.rs         — first/some/none, turbofish
    result.rs           — T!E sugar, ? operator
    examples.rs         — drives all 20 canonical examples
    negatives.rs        — negative corpus
```

### 3.3 `Ty` representation

```rust
pub type TyId = la_arena::Idx<TyData>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TyData {
    Bool,
    Int(IntKind),
    Float(FloatKind),
    Char,
    Str,            // immutable slice/ref
    String,         // owned
    Bytes,
    Unit,
    Never,
    Tuple(Vec<TyId>),
    Array { elem: TyId, len: Option<u64> },     // const-len for sized array; None for slice
    Ref { mutable: bool, inner: TyId },
    Fn { params: Vec<TyId>, ret: TyId, effects: Vec<EffectId> },
    Adt(AdtId, Vec<TyId>),                       // struct/enum with type args
    Var(TyVarId),                                // inference variable
    Param(ParamId),                              // generic parameter slot
    Error,                                       // poisoned; supresses cascades
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum IntKind { I8, I16, I32, I64, I128, U8, U16, U32, U64, U128, USize, ISize, IntInfer }
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FloatKind { F32, F64, FloatInfer }
```

`IntInfer` / `FloatInfer` are integer/float literal types that default to `I32` / `F64` if not constrained by context (Haskell/Rust default rule).

Types are interned: identical `TyData` returns identical `TyId`. This makes equality cheap.

### 3.4 ADT defs

```rust
pub struct AdtDef {
    pub name: String,
    pub kind: AdtKind,                  // Struct | Enum
    pub generics: Vec<ParamDef>,
    pub variants: Vec<VariantDef>,      // structs have exactly one variant with field-named payload
}
pub struct VariantDef {
    pub name: String,                   // for structs: same as ADT
    pub fields: Vec<FieldDef>,
}
pub struct FieldDef {
    pub name: Option<String>,           // None for tuple variants like Enum.Variant(T, U)
    pub ty: TyId,                       // may reference Param slots
}
pub struct ParamDef { pub name: String, pub bounds: Vec<TyId> }   // bounds unused in slice 3
```

### 3.5 Name resolution

A new module `mty-types::resolve` walks the HIR top-level and produces:

```rust
pub struct DefMap {
    pub adts: ArenaMap<AdtId, AdtDef>,
    pub fns:  ArenaMap<FnDefId, FnDef>,         // top-level + agent methods
    pub type_aliases: HashMap<String, TyId>,
    pub by_name: HashMap<String, DefRef>,       // top-level lookup
}
pub enum DefRef { Adt(AdtId), Fn(FnDefId), Param(ParamId), Builtin(BuiltinDef) }
```

The prelude is merged into `by_name` before user items, so user code can shadow but built-ins are always present. Path resolution understands single-segment names (`Option`, `Some`, `log`, `String`) and dotted paths (`std.json`, `std.http`) — for slice 3, dotted paths starting with a known stdlib package resolve to a stub `Builtin::OpaqueModule`; member access on opaque modules returns `Ty::Error` *without* emitting a diagnostic (we tolerate `std.json.encode` etc. because the stdlib doesn't exist yet — see §4 below).

Type-position paths resolve via `resolve::resolve_hir_type(hir_ty) -> TyId`. Expression-position paths resolve via `resolve_value_path(segments) -> ValueRef` where `ValueRef = Local | Fn | EnumCtor | StructCtor | Builtin | Error`.

Generics on type paths are checked for arity. Type aliases are eagerly expanded (no recursion check needed — declared aliases must be acyclic, and we don't check the cycle in slice 3 because no example uses recursive aliases).

### 3.6 Inference engine

Standard HM with these structures:

```rust
pub struct InferCtx<'a> {
    pub types: &'a mut TypeArena,
    pub adts: &'a DefMap,
    pub diagnostics: &'a mut Vec<Diagnostic>,
    pub subst: Substitution,            // TyVarId -> TyId
    pub locals: Vec<Scope>,
    pub return_ty: TyId,                // for ? and return
    pub effect_ctx: Vec<EffectId>,      // parsed only, unused
}

pub struct Substitution(Vec<Option<TyId>>);
```

**Unification** (`unify(a, b)`):

1. Resolve `a`, `b` through the substitution to their representatives.
2. If both are `Var(v1)`, `Var(v2)` with `v1 == v2`: ok.
3. If `Var(v)`: occurs-check `v` not in `other`; bind `subst[v] = other`.
4. If both `Adt(d1, args1)`, `Adt(d2, args2)` with `d1 == d2`: zip-unify args.
5. If both `Ref { mutable: m1, inner: i1 }`, `Ref { mutable: m2, inner: i2 }` with `m1 == m2`: unify inners.
6. If both `Fn { params: p1, ret: r1, .. }`, `Fn { params: p2, ret: r2, .. }`: arity-check, zip-unify params, unify rets. Effects ignored.
7. If both `Tuple(xs)`, `Tuple(ys)`: arity-check, zip-unify.
8. If both `Array { elem: e1, len: l1 }`, `Array { elem: e2, len: l2 }`: unify elems; len ok if either is None or equal.
9. `Int(IntInfer)` unifies with any concrete `Int(_)`; concrete unifies only with same kind. Same for floats.
10. `Never` unifies with any (it's the bottom type).
11. `Error` unifies with anything (poison).
12. Anything else: type mismatch — emit `MT2001` with both pretty-printed types.

**Defaulting**: after item-body inference, walk leftover `IntInfer` → `I32`, `FloatInfer` → `F64`. Unresolved generic `Var` after defaulting → `MT2003 cannot_infer_type`.

### 3.7 Bidirectional checking

Two modes:

- `synth_expr(e) -> TyId` — bottom-up, used when there's no expected type
- `check_expr(e, expected: TyId)` — pushes the expected type down

Choice of mode per expression kind:

| Expr | Synth | Check |
|---|---|---|
| Literal | yes (returns IntInfer / FloatInfer / Bool / Str / Char) | yes (unify against expected) |
| Path | yes | check by unify |
| Call | synth callee, check args against param types | unify return with expected |
| MethodCall | synth receiver, look up method on its type (slice 3: only ADT-defined methods + a small builtin table for `len`, `to_str`, `get`, `read`, `embed`, `post`, `query`, `ok_or` etc — see §4) |
| Field | synth receiver, look up field | unify field's ty |
| Binary | synth lhs+rhs, unify, return | unify result |
| If | check both branches against expected (or join via unify) | |
| Match | check all arms against expected | |
| Block | check tail against expected | |
| Lambda | synth from params + body (or check against expected `Fn`) | |
| Struct literal | check field exprs against field types | unify with `Adt(struct_id, args)` |
| `?` | requires enclosing return = `Result[_, E]`; expr must be `Result[T, E]`; produces `T` | |
| `Send`/`Ask`/`Deadline` | slice 3 returns `Ty::Error` quietly (these are agent runtime constructs; full check is slice 5) | |
| `Spawn`/`Detach`/`Join` | slice 3: agent → opaque `AgentRef[T]` ty; return type is `T` | |

For expressions involving constructs we don't fully type yet (sandbox, budget, agent message handlers' message-arg types from a `protocol`), we **synthesize `Ty::Error`** and continue. The driver collects only the first error per expression chain.

### 3.8 Generic instantiation

At a call site:

1. Look up the fn's `FnDef { generics: Vec<ParamDef>, params, ret, effects }`.
2. If a turbofish provided N type args: check arity matches `generics.len()`; substitute into `params`/`ret`.
3. Else: fresh `TyVar` for each generic; substitute into `params`/`ret`.
4. Unify each arg expression's synth type against the substituted param type.
5. Unify expected return (if checking) against substituted ret.
6. Continue with subst.

`Some(x)` and `None`: special-cased as constructors of `Option[T]`. `Some(x)` synthesizes a fresh `TyVar` for `T`, unifies it with `x`'s type, returns `Adt(option_id, [T])`. `None` requires an expected type (so the var can be solved); standalone `None` defaults to `Adt(option_id, [Var])` and the var resolves via context.

`Ok(x)` / `Err(x)`: similar for `Result[T, E]`.

### 3.9 `T!E` and `?`

Lowering already produces `HirType::Result { ok, err }`. The resolver maps `HirType::Result { ok, err } → Adt(result_id, [ok, err])`. `HirType::Union(_)` (slice-2 form of `T!{A,B}`) maps to `Adt(result_id, [ok, AnonUnion(A,B)])` — but anonymous error unions are a v0.2 feature, so for slice 3 we accept the syntax but resolve the error type to `Ty::Error` (no diagnostic). This keeps example 04 compiling.

`?` semantics:

1. Expr must have type `Adt(result_id, [t, e])`.
2. Enclosing fn return must have type `Adt(result_id, [_, e'])`. (If not, `MT2010 question_outside_result`.)
3. Unify `e` with `e'`. (If mismatch, `MT2011 question_error_mismatch`.)
4. Result of `expr?` is `t`.

### 3.10 Public signature validation

A pass over top-level fns:

- If `is_pub`: every param must have an explicit `ty`. (`HirParam.ty.is_some()`.) If missing → `MT2020 pub_param_needs_type`.
- If `is_pub` and return type missing → already MT0021 from parser.
- If `is_pub` and any generic param appears in fn body's local type inference but not in the signature → not enforceable without inference, so skip in slice 3.

### 3.11 Diagnostic codes (MT2001..MT2099)

```
MT2001 type_mismatch                  — expected T, found U
MT2002 unresolved_type                — type name `Foo` does not name a type
MT2003 cannot_infer_type              — cannot infer type for binding `x`
MT2004 wrong_generic_arity            — type `Vec` expects 1 arg, got 0
MT2005 wrong_arg_count                — fn expects N args, got M
MT2006 unknown_field                  — struct `Foo` has no field `bar`
MT2007 unknown_method                 — type `T` has no method `m`
MT2008 not_callable                   — value of type T is not callable
MT2009 unknown_variant                — enum `Foo` has no variant `Bar`
MT2010 question_outside_result        — `?` requires fn returning Result[_, _]
MT2011 question_error_mismatch        — `?` error type mismatch
MT2012 wrong_variant_arity            — variant `Some` expects 1 payload, got 0
MT2013 missing_struct_field           — struct `Foo` initializer missing field `x`
MT2014 duplicate_struct_field         — duplicate field `x` in struct initializer
MT2015 non_exhaustive_match           — warning: match not exhaustive (missing variants)
MT2016 unreachable_match_arm          — warning: unreachable arm
MT2017 binop_type_mismatch            — operator `+` not defined on T, U
MT2018 if_branch_mismatch             — if/else branches have incompatible types
MT2019 return_type_mismatch           — fn returns T, body produces U
MT2020 pub_param_needs_type           — pub fn parameters require explicit types
MT2021 unresolved_value               — name `foo` does not refer to any value
MT2022 not_a_struct                   — value of type T cannot be initialized with struct literal
MT2023 generic_arg_mismatch           — type arg N: expected kind K
MT2024 lambda_arity_mismatch          — lambda has N params, expected M
MT2025 cannot_take_ref                — cannot take reference to non-place expression
```

Plus `MT2026..MT2099` reserved for future type errors.

### 3.12 Prelude (`std.core`)

Synthesized by `prelude::build_prelude(&mut TypeArena, &mut DefMap)`. Defines:

**Types:**
- `Option[T] = Some(T) | None`
- `Result[T, E] = Ok(T) | Err(E)`
- `String` (alias to built-in `Ty::String`)
- `Bytes` (alias to built-in `Ty::Bytes`)
- All primitive aliases (`Bool`, `I8..I128`, `U8..U128`, `USize`, `ISize`, `F32`, `F64`, `Char`, `Str`, `Unit`, `Never`)
- Opaque modules for `std.http`, `std.json`, `std.dom`, `std.trace` so `use std.http` and member access compile

**Values (builtin fns):**
- `log: fn(Str) -> Unit effect io`
- `panic: fn(Str) -> Never`
- `spawn: fn[T](T) -> AgentRef[T]` (special — `T` must be `agent`-shaped; slice 3 lets `T` be any ADT)
- `move: fn[T](T) -> T` (identity for typeck)
- `Some/None/Ok/Err` — added as enum variant constructors
- A small "magic methods" table for things examples need: `len`, `to_str`, `get`, `ok_or`, `query`, `set_text`, `read`, `write`, `post`, `embed`, `encode`, `read` (Fs), `serve`, `on`, `ok`. These are typed as `fn(self, ...) -> Var` — i.e. the return type is a fresh inference variable so the example doesn't fail on a return-type mismatch. If a method isn't in the table and isn't user-defined, emit `MT2007`.

**Opaque modules:**
- `std.http`, `std.json`, `std.dom`, `std.trace` are typed as `Module`. Field access on a Module returns `Ty::Error` silently if the member name is unknown. Known stubs: `http.ok`, `http.serve`, `http.Handler`, `json.encode`, `dom.set_text`, `dom.on`, `trace.span`.

**Other identifiers in examples needing some treatment:**
- `Url`, `Page`, `IoErr`, `NetErr`, `ParseErr`, `FetchErr`, `Logger`, `Fetcher`, `Lowered`, `RunErr`, `Fs`, `Path`, `Net`, `Model`, `Dom`, `MainErr`, `SearchErr`, `Json`, `Json!SearchErr`, `Map`, `Config`, `ConfigErr` — these are names referenced in the examples without declarations. Add them to the prelude as opaque ADTs with no fields (zero-arity). User-defined types of the same name shadow them.

The opaque-ADT trick is what lets examples like 04, 13, 19 compile without forcing the user to write a stdlib. Document this in `docs/internals/typeck.md` as the "prelude tolerance" strategy.

### 3.13 Effect / capability signatures

`Ty::Fn { effects: Vec<EffectId> }` carries the effect set. `EffectId` interns effect names (`io`, `net`, `model`, `spawn`, `dom`, `clock`, `time`, etc.). For slice 3, we *parse* them out of the HIR (`HirFn::effects`) and put them in `Ty::Fn`. We do not check that callers narrow them. Slice 5 adds enforcement.

Capability parameters (`agent X(net): Y`) lower to constructor params of type `Adt(net_cap_id, _)`. Slice 3 types them as the relevant opaque prelude type. No narrowing check.

## 4. Example conformance plan

For each example, list what type-checker features it exercises and what (if any) prelude stubs it needs:

| Ex | Features | Prelude needs |
|---|---|---|
| 01_hello | `log(Str)` | `log` |
| 02_struct_enum | struct, enum, type alias, match with enum patterns, F64 arithmetic | UserId alias, primitives |
| 03_generic_fn | generic fn, `&[T]`, `Option[&T]`, `.len`, `&xs[0]`, `Some/None` | Option, `.len` magic |
| 04_result_propagation | `T!E`, `?`, `Ok/Err`, multi-error union | Result, fetch/parse opaque |
| 05_match_expr | range pattern `1..10`, wildcard, str literal | — |
| 06_for_while_loop | `&[I32]`, `for x in xs`, `?` outside Result fn (this might fail; see §5.1) | work/ready/step opaque |
| 07_agent_echo | protocol, agent, on-handler with ret expr | agent typing minimal |
| 08_agent_state | agent state init, `n += 1`, block tail | I64 inference |
| 09_send_ask | send, ask, deadline | Logger/Fetcher/Url opaque |
| 10_supervisor | supervisor decl with strategy, spawn, on_fail with restart/backoff | spawn, Planner/Fetcher opaque |
| 11_budget_block | budget entries, run expr | job/RunErr opaque |
| 12_arena | arena turn, arena turn: short form, ? | tokenize/parse/lower opaque |
| 13_capabilities | agent with cap params | Fs/Path/Net opaque |
| 14_extern_c | extern c block, export c fn | — (extern block; check fn sig only) |
| 15_extern_js | extern js with effect | — |
| 16_macro | macro decl (not typechecked) | — |
| 17_unsafe | unsafe block, requires clauses, *U8 pointer | raw_ptr, USize, U8 |
| 18_sandbox | sandbox expr, run expr | job opaque |
| 19_backend_service | package, use, agent with cache, if let Some, multiple deadlines, http.serve | std.http, std.json, std.trace, Net, Model, SearchErr, Json |
| 20_frontend_component | export fn, lambda, agent method, dom intrinsics | std.dom, Dom |

**Tricky case** (example 06): `work(item)?` inside `for` inside a fn returning `()`. The `?` requires the enclosing fn to return `Result[_, _]`. Spec interpretation: slice 3 emits `MT2010` and the example fails. **Resolution**: amend example 06 to return `Unit!WorkErr`, or treat the `?` as a slice-3 special case "permissive" (emit warning, not error). **Decision**: amend the example. Add `WorkErr` to prelude opaques.

Similarly, example 11 has `Result!RunErr` — that desugars to `Result[Result, RunErr]` literally. **Resolution**: amend to `Unit!RunErr`. Document in spec amendments.

Examples 14, 16, 17 contain constructs (extern body fn sig, macro body, `requires` clauses, raw pointers) that aren't fully checked in slice 3. They must still parse and the fn's surface signature must validate. Macro bodies are skipped (treated as opaque tokens — already true at HIR level).

### 4.1 Negative test corpus

`tests/typeck_neg/*.sd` plus a `tests/typeck_neg.rs` driver that asserts each emits the expected diagnostic code:

- `mismatch_let.sd` — `let x: I32 = "hi"` → MT2001
- `mismatch_call.sd` — `log(42)` → MT2001
- `unresolved_type.sd` — `fn f(x: NoSuch) -> Unit` → MT2002
- `wrong_arity.sd` — `Some(1, 2)` → MT2012
- `unknown_field.sd` — `User { id: 1, missing: 2 }` → MT2006 (or MT2014)
- `pub_no_type.sd` — `pub fn f(x) -> Unit` → MT2020
- `q_outside_result.sd` — `fn f() -> Unit { foo()? }` → MT2010
- `q_err_mismatch.sd` — `Result[T,A]?` in `Result[U,B]` fn → MT2011
- `unknown_variant.sd` — `match x { Foo.Bar => 1 }` where no `Foo.Bar` → MT2009
- `binop_mismatch.sd` — `1 + "x"` → MT2017 (or MT2001)
- `wrong_generic_arity.sd` — `Option[I32, Str]` → MT2004
- `not_callable.sd` — `let n = 1; n()` → MT2008
- `unresolved_value.sd` — `let x = no_such_name` → MT2021
- `lambda_arity.sd` — `fn(a) {}` checked against `fn(I32, I32) -> Unit` → MT2024
- `return_mismatch.sd` — `fn f() -> I32 { "hi" }` → MT2019

15 negative cases × 1 expected code = 15 tests.

## 5. Interpretation calls (slice-3 owns these)

These are choices the spec doesn't fully nail down. They get the BOLD treatment per the autonomous-build mandate.

### 5.1 `?` in `Unit`-returning fns

Spec §17 implies `?` only inside `Result`-returning fns. We enforce this strictly. Examples are amended (06, 11). Document in `v0.1-amendments.md` A7.

### 5.2 Integer literal default

Spec doesn't specify. We adopt Rust's rule: unsuffixed integer literals default to `I32`, floats to `F64`. Suffixed literals (`42_u64`, `3.14_f32`) take the suffixed type. Document in `v0.1-amendments.md` A8.

### 5.3 String literals

`"hi"` synthesizes as `Str`. `String("hi")` is a call to the `String` constructor (in prelude, `String: fn(Str) -> String`). This means `String` is both a type *and* a fn-value — that's fine, value-namespace and type-namespace are separate. Document A9.

### 5.4 Method resolution table

Slice 3 cannot do full trait-dispatch method resolution. We keep a small **builtin method table** (`prelude::BUILTIN_METHODS`) mapping `(receiver_shape, method_name) → fn ty`. For example `(Array(_, _), "len") → fn(&Self) -> USize`. Unknown methods on user types check `impl` blocks (already in HIR). Unknown methods on opaque types fall back to `Ty::Var` (silently). This is the "tolerance" mechanism. Document A10.

### 5.5 `move x` semantics

Type-checks as identity. Ownership-move semantics ship in slice 4.

### 5.6 Agent body type-checking

Agent message handlers' parameter types come from the *declared protocol*. If the agent's protocol is unresolved (e.g. `agent X: SomeProtocol` where `SomeProtocol` is opaque), handler params synthesize as `Ty::Var` and their body type-checks against `Ty::Error` (return permissive). When the protocol *is* resolved, params take the declared protocol-message arg types and the handler body's tail must unify with the protocol's reply type.

### 5.7 `run <expr>` and `arena`/`budget`/`sandbox` blocks

These wrap an expression/block. Their type = the body's type. Slice 3 does not constrain *where* they appear (the placement check is slice 5).

### 5.8 `effect` clauses

Parsed effects are stored on `Ty::Fn`. No call-site narrowing check. Mismatched effects: silently accept.

### 5.9 `dyn Trait` types

`HirType::Path { segments: ["dyn", ...] }` — actually `TYPE_DYN`. For slice 3 we resolve `dyn Foo` as `Ty::Error` (no check, no diagnostic). Slice 4+ handles trait objects.

### 5.10 Capability typing

`Net`, `Fs`, `Clock`, `Dom`, `Model` → opaque ADTs in the prelude. No narrowing. Member access on a capability returns a fresh `Ty::Var` (permissive). Slice 5 makes them real capability types with narrowing semantics.

## 6. Incremental adoption strategy

Tasks are ordered so the workspace compiles and 174 tests stay green after every commit. The type checker is built behind a function (`sdust_types::check_package(pkg) -> Vec<Diagnostic>`) and wired into the driver as the *final* pipeline stage in the last task. Until that wiring lands, `mty check` behaves exactly as it does in slice 2 (parse + lower).

Order:

1. Scaffold `mty-types` crate.
2. `Ty`, `TyArena`, `IntKind`, `FloatKind`, `intern`.
3. `AdtDef`, `FnDef`, `DefMap`.
4. Prelude builder.
5. Type resolution (HirType → TyId).
6. Value-path resolution.
7. `InferCtx`, `Substitution`, `unify`, `occurs_check`.
8. Bidirectional `check_expr` / `synth_expr` for literals, paths, blocks, ifs.
9. Calls + generic instantiation.
10. Struct literals, field access, method calls.
11. Match + patterns.
12. `?`.
13. Item-level checking + pub-signature validation.
14. Agent/protocol handler shape.
15. Diagnostic builders.
16. Wire into `mty-driver`.
17. Example sweep + amendments.
18. Negative test corpus.
19. Docs + tour updates.

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Inference blows up on complex example | Permissive fallback to `Ty::Var` + `Ty::Error` keeps the checker non-fatal for unmodelled constructs |
| Generic instantiation gets `Ty::Var` left over | Defaulting pass after each item; `MT2003` for the rare residual |
| Method-resolution table is brittle | Document table location; new built-in methods are one-line additions |
| Examples cite stdlib that doesn't exist | Opaque prelude modules + opaque types make this tractable; we don't try to define std at all |
| `?` in `Unit` examples fail | Amend the examples; document |
| Subagents take too long | Use sonnet (not opus) for implementer tasks; pre-decompose the plan into small tasks (≤2hr each); skip per-task code-review |

## 8. Test budget

- 174 tests preserved
- Per-module unit tests: ~40 (interner, prelude, resolve, unify, check primitives, check generics, check Result, check items)
- Examples driver: 20 tests (one per .sd file)
- Negative corpus: 15 tests
- Round-trip / regression smoke: 5 tests

**Target:** 250+ total tests after slice 3.
