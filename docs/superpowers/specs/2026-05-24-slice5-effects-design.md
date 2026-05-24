# Stardust Slice 5 Design — Effects, Capabilities, Traits, Protocol Strictness

**Date:** 2026-05-24
**Status:** Approved (autonomous build — user away, slice-leader = Claude)
**Source spec:** `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` (Stardust Language Specification v0.1) — §8 (Capabilities), §9 (Effects), §13 (Protocols), §15 (Supervisors), §16 (Budgets and Sandboxing), §19 (Traits / `dyn`).
**Slice maps to:** Spec §31.3 Phase 3 — Authority. Closes §8 + §9, hardens §13, ships first-class trait coherence + `dyn` dispatch.
**Prior slice:** `v0.4.0-borrowck` (commit `23c3c1f`), summary in `SLICE4.md`.
**Repo:** `C:\Users\ihass\stardust` (remote `hassard0/stardust`).

---

## 1. Goal

Add Stardust's **effect system** (§9), **capability narrowing + delegation**
(§8), **real trait dispatch with coherence checking** (§19), `dyn Trait`
dispatch, `derive(Copy)` on user ADTs, top-level `sandbox` items (§16.1),
and strict protocol-handler signature checks (slice-4 deferral A18).
Plus several slice-4 polish items: `move *ref` modelling for SD3009,
tighter SD3002 vs SD3008 distinction, and (stretch) field-level borrows.

After this slice the canonical 20 examples still pass and the compiler can
prove:

- Every `pub fn`'s declared `effect` clause is a superset of the body's
  inferred effects (else `SD4001 effect_undeclared`).
- Strict-profile (`profile = "core"`) fns error on any hidden `alloc`
  (`SD4002 alloc_in_core`).
- A function consuming `fs: FsReadOnly[Path("/data")]` cannot pass it to
  a fn requiring a broader `Fs` capability (`SD4010 capability_too_broad`).
- `impl Trait for Type { ... }` registers the trait/method pair in a
  coherence table; `let h: dyn Hash = user_id` requires the
  `UserId: Hash` impl in scope (`SD4023 dyn_requires_object_safe`,
  `SD4020 method_ambiguous`, `SD4021 method_not_found`).
- `#[derive(Copy)] struct Vec2 { x: F32, y: F32 }` makes Vec2 Copy if all
  fields are Copy.
- Top-level `sandbox` items parse + lower.
- Agent `on Msg(args)` handlers strictly match the protocol's declared
  signature (SD4030/31/32/33).
- Trait coherence detects overlapping method-name impls on the same
  receiver (SD4022, name-only).

The acceptance gate:

- `cargo test --workspace` green (≥ 266 baseline + new slice-5 fixtures)
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo fmt --all -- --check` clean
- All 20 canonical examples `sdust check` clean
- New SD4xxx negative corpus

## 2. Non-goals for slice 5

- Polonius / NLL — post-v0.1
- Constraint-overlap trait coherence — post-v0.1 (slice 5 only catches
  same-name method collisions on the same self-type)
- `Self`-in-method `dyn` (true object safety table) — post-v0.1
- Full `derive` macro system — slice 5 ships hardcoded handlers for
  `Copy`, `Hash`, and `Eq` derives only
- SIR / interpreter (slice 6)
- Runtime (slice 7)
- Codegen (slice 8)
- Real serializer registry for cross-agent Sendable — slice 6+
- Cross-function lifetime inference — post-v0.1
- Effect-row polymorphism (effect variables in generic signatures) — post-v0.1
- `effect` syntax beyond identifier lists (no nested effect annotations on
  closures or stored fn values)

## 3. Architecture

### 3.1 Crate layout

Extend `sdust-types` with a new module `effects.rs` for inference; add a
new module `traits.rs` to host the coherence table + dispatch resolver.
Capability typing extends `Ty` directly. No new crate.

```
sdust-syntax → sdust-ast → sdust-hir → sdust-types → sdust-borrow
                                       (+ effects, traits, caps)
```

### 3.2 New `Ty::Cap` variant

Add `TyData::Cap { family: CapFamily, constraint: CapConstraint }` to the
type arena.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapFamily { Net, Fs, Clock, Dom, Model, Custom(InternedSym) }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapConstraint {
    /// Top — accepts anything. Default for `Fs` / `Net` / etc. references.
    Any,
    /// Read-only narrowing (no writes).
    ReadOnly,
    /// Path glob narrowing — e.g. `Path("/data")` accepts only paths under
    /// `/data`.
    Path(String),
    /// Network host:port allowlist — e.g. `Host("api.example.com:443")`.
    Host(Vec<String>),
    /// Combination — set semantics (all must hold).
    And(Vec<CapConstraint>),
}
```

A capability is "narrower" than another iff its constraint set is a
superset (more restrictive). The subsumption check is `narrower ⊑ broader`.
Slice-5 MVP only models a small subset:

- `Any ⊑ Any`
- `ReadOnly ⊑ Any` (only for `Fs`)
- `Path(p) ⊑ Any` and `Path(p) ⊑ Path(prefix)` if `p` starts with `prefix`
- `Host(xs) ⊑ Any` and `Host(xs) ⊑ Host(ys)` if `xs ⊆ ys`
- `ReadOnly ⊑ Path(p)` if both apply

The narrowing rules are pure data. Slice 5 only checks subsumption at fn
call sites (SD4010).

### 3.3 Effect inference

A new pass `sdust_types::effects` walks every fn body bottom-up after
type-checking and computes the **inferred effect set** (a
`HashSet<EffectId>` plus a flag for `unsafe`/`alloc` triggered by
specific syntactic constructs).

Bottom-up rules:

- `arena name { ... }` — adds `alloc` (per spec §16).
- `spawn ...` / `spawn task ...` — adds `spawn`.
- `unsafe { ... }` — adds `unsafe`.
- `let h = detach ...` — adds `spawn`.
- `target!Msg(...)` / `target?Msg(...)` — adds `spawn` (slice 5 keeps a
  conservative read of "agent messaging causes scheduler work").
- `expr @ duration` — adds `time`.
- Calling another fn — union the callee's inferred effects (resolved by
  call-graph traversal; recursion is handled by a fixpoint over per-fn
  effect sets).
- Capability-receiver method calls — slice 5 maps:
  - `<fs:Fs>.read/.write/.list/.ro` → `fs` + (write methods adds nothing
    beyond fs)
  - `<net:Net>.get/.post/.head` → `net`
  - `<clock:Clock>.now/.sleep` → `time`
  - `<dom:Dom>.*` → `dom`
  - `<model:Model>.*` → `model`
  - `Map`/`Vec`/`HashMap` constructor calls (`Map::[K,V]{}`, `.push`, etc.)
    → `alloc`
- `panic("...")` — adds `unsafe` (per spec §17.3, panic is unrecoverable
  state; we model it as unsafe to ensure callers see it; alternate calls
  in slice 6).
- HTML template literals / `html"..."` — adds `alloc`.
- `cache[q] = out` style indexed-assignment on a non-array — adds `alloc`
  (best-effort; can refine).

Public-fn discipline:

- For every `pub fn` with an `effect ...` clause, the declared set MUST be
  a superset of the inferred set. If not, emit
  `SD4001 effect_undeclared` listing the missing effects.
- For every `pub fn` *without* an `effect` clause, the inferred set must
  be empty (other than the always-allowed `alloc` if not in strict
  profile). If non-empty, emit `SD4001` recommending the clause.

Profile gate:

- `star.toml` may declare `profile = "core"`. When set, the inferred set
  may not include `alloc` (catches hidden heap allocations in
  embedded-target code). On violation: `SD4002 alloc_in_core`.
- Default profile in slice 5 is `host` (permissive). The 20 examples use
  the host profile, so no example needs source changes for SD4002.

Fixpoint:

- For internal fns (not pub) and for recursion, slice 5 uses a simple
  fixpoint: initialize every fn's inferred set to empty, then iterate
  body-walks until no set changes. Bounded by O(n × max_call_depth).

Side table:

- `TypedPackage.fn_effects: HashMap<FnId, HashSet<EffectId>>` — every fn's
  inferred effect set (post-fixpoint, post-defaulting).

### 3.4 Capability narrowing + delegation

Capability typing in slice 5:

1. Prelude declares the five core caps with `Cap::Any` constraints:
   `Net = Cap { family: Net, constraint: Any }`
   `Fs = Cap { family: Fs, constraint: Any }`
   ...etc.
2. Narrowing constructors are built-in methods on the cap receivers:
   - `fs.ro(path: Path)` → `Cap { family: Fs, constraint: And([ReadOnly, Path(path)]) }`
   - `fs.path(path: Path)` → `Cap { family: Fs, constraint: Path(path) }`
   - `net.host(host: Str)` → `Cap { family: Net, constraint: Host([host]) }`
3. At call sites where a parameter's type is a `Cap { family: F, constraint: C }`
   and the argument's inferred type is `Cap { family: F', constraint: C' }`,
   we require `family == family' AND C' ⊑ C`. Else emit
   `SD4010 capability_too_broad`. The "broader argument" case (caller
   provides `Any` for a parameter wanting `ReadOnly`) is the trip case.
4. Path-`as` syntax (`fs as Fs`) explicitly broadens — also illegal in
   slice 5; emit `SD4010` and recommend `fs.path(...)` instead. Slice 5
   does NOT support broaden-via-cast.
5. Capability values are **affine** (cannot be cloned) by the borrow
   checker. Already enforced via slice 4 because Cap is non-Copy.
   We register Cap as `Copy = false` and `Sendable = true` (caps may
   cross agents per spec §8.1 if serializable).

Slice 5 implementation:

- Parse `fs.ro("/data")` as a normal method call. The receiver's resolved
  type is `Cap { Fs, Any }`; lookup `.ro` in the built-in capability
  method table; produces a `Cap { Fs, And([ReadOnly, Path("/data")]) }`.
- At param-type vs arg-type check sites, run `is_narrower_or_equal(arg, param)`
  and emit SD4010 on failure.
- For multi-cap functions (e.g. `fn serve(fs: Fs, net: Net)`), each cap
  is independent.

### 3.5 Trait dispatch + coherence

Trait declaration:

```sd
trait Hash {
  fn hash(self) -> U64
}
```

Implementation:

```sd
impl Hash for UserId {
  fn hash(self) -> U64 { self as U64 }
}
```

Coherence table (lives in `DefMap`):

```rust
#[derive(Default)]
pub struct TraitTable {
    /// `trait_name -> Vec<(self_adt_id, FnDefId for each method)>`
    pub impls: HashMap<String, Vec<TraitImpl>>,
    /// `(self_adt_id, method_name) -> Vec<(trait_name, FnDefId)>`
    /// Used for dispatch: at a method call site, find all impls that
    /// match (trait scope filter is slice-5 trivial: every trait in
    /// scope is visible).
    pub by_method: HashMap<(AdtId, String), Vec<(String, FnDefId)>>,
    /// Set of `(trait_name, self_adt_id)` pairs — for overlap detection.
    pub impl_keys: HashSet<(String, AdtId)>,
    /// Per-trait declared method set, for `dyn Trait` object-safety check.
    pub trait_methods: HashMap<String, Vec<TraitMethodSig>>,
}

pub struct TraitImpl {
    pub trait_name: String,
    pub self_adt: AdtId,
    pub method_fns: HashMap<String, FnDefId>,
    pub span: SourceSpan,
}

pub struct TraitMethodSig {
    pub name: String,
    pub has_self_ty: bool,   // For object-safety: bans Self in return/param.
    pub has_generics: bool,  // Bans generic methods in `dyn`.
}
```

Method dispatch (replaces slice-4 permissive index):

1. If receiver is a `Cap { family, constraint }`: look up the built-in
   cap method table (cap families have small fixed method sets).
2. Else if receiver is a user `Adt(aid, args)`:
   a. **Inherent impl**: `impl T { fn m(...) }` registers
      `impl_methods[(aid, "m")] = fdef_id`. Lookup that first.
   b. **Trait impls in scope**: `trait_table.by_method[(aid, "m")]` may
      have one or more entries.
      - Exactly one → resolve to that impl's `FnDefId`.
      - Two or more → `SD4020 method_ambiguous`. Recommend type-annotated
        call: `<T as Trait>.m(receiver)`. (Slice 5 doesn't parse this
        syntax; just reports the candidates.)
   c. Neither → `SD4021 method_not_found`. (Replaces slice-4's
      "opaque-permissive" fallback for user ADTs.)
3. Else if receiver is `dyn Trait`: look up that trait's method table; if
   the method exists, resolve to a dynamic call (slice 5 records "dyn
   dispatch" on a side table; codegen later).
4. Else if receiver is opaque (`Url`, `Page`, ...) or primitive: keep
   slice-4 built-in table (this is unchanged).

Coherence check at build time:

- For each `impl Trait for T { ... }`, check `trait_table.impl_keys.contains(&(trait, T))`.
  If yes, emit `SD4022 trait_coherence_violation`. The check is
  name-only — slice 5 doesn't detect "impl Hash for Vec[T]" vs
  "impl Hash for Vec[U64]" overlap (that needs generic-constraint
  unification; post-v0.1).
- Inherent impl shadows trait impl per (1.a). No diagnostic if both
  exist; the inherent wins.

### 3.6 `dyn Trait` dispatch

`HirType::Dyn(trait_name)` added (CST already has `TYPE_DYN`).

Parse: `dyn Trait` is a new type form, accepted everywhere a type can
appear (parameter, return, let).

Type-check at coercion site `let h: dyn Hash = user_id`:

1. Resolve `dyn Hash` to `TyData::Dyn { trait_name, methods }`.
2. The RHS expression's type must be `Adt(T, ...)` such that the trait
   table has `(Hash, T) → impl`. If no impl: `SD4023 dyn_requires_object_safe`
   (with sub-reason "no implementation"); also catch slice-5 object-safety
   conservative rules:
   - Any method with `Self` in its signature → object-unsafe
   - Any generic method → object-unsafe
3. Successful coercion records a `dyn_coercion` entry on a side table
   (`HashMap<ExprId, (TraitName, AdtId)>`).

At dispatch `h.hash()` where `h: dyn Hash`:
- Look up `Hash.hash` in trait_table. Resolve to dyn dispatch (no static
  FnDefId yet — slice 5 records the trait + method).

### 3.7 `#[derive(...)]` and `derive Copy`

Parse two surface forms:

1. `#[derive(Copy)] struct Vec2 { ... }` — bracketed attribute, common.
2. `derive Copy struct Vec2 { ... }` — leading-keyword sugar.

Slice 5 ships derive for `Copy`, `Hash`, `Eq`. Effects:

- `Copy` — register Copy via a `DefMap.user_copy: HashSet<AdtId>` set. The
  `is_copy` predicate checks this set in addition to its existing rules.
  All fields/variants must themselves be Copy; else
  `SD4040 derive_copy_field_not_copy`.
- `Hash` — register an implicit `impl Hash for T { fn hash(self) -> U64 ... }`
  in the trait table. The body isn't generated (slice 5 doesn't have
  codegen), but the impl entry exists so `dyn Hash = my_t` works.
- `Eq` — similar.

Tokenization:

- Add `#` as already-existing token. Add a `derive` keyword recognized in
  `derive Copy` position (not reserved everywhere — handled with
  identifier-tolerance at parse time).
- AST: `HirStruct.derives: Vec<String>`, `HirEnum.derives: Vec<String>`.

### 3.8 Top-level `sandbox` items (spec §16.1)

Already an expression-position form. Slice 5 adds parsing at the top
level:

```sd
sandbox ToolRun with {
  fs.read = ["/tmp"]
  net = ["api.example.com:443"]
} {
  run job(input)
}
```

Lowering: a new `HirItem::Sandbox(HirTopSandbox)` with the same fields as
the expression form, plus a name. Type-checking treats it as a unit-result
block under sandbox tolerance.

### 3.9 Strict protocol message types

Slice 4's `lookup_protocol_msg_types` returns parameter types or warns
SD2026. Slice 5 promotes the strict checks:

- **SD4030 protocol_arity_mismatch**: `on Msg(p1)` where the protocol
  declares `Msg(p1: Str, p2: U32)`. Error.
- **SD4031 protocol_param_type_mismatch**: the protocol declares
  `Msg(p1: Str)` but the body uses `p1` as I32. Slice 5 catches this via
  the type-check of the body itself once param types are bound — the
  diagnostic is the normal `SD2001` mismatch surfaced from binding.
  Slice 5 promotes the binding mismatch (param annotation provided
  vs body usage) to `SD4031` for clearer messaging.
- **SD4032 protocol_missing_handler**: the agent implements `Protocol A`
  which declares messages `M1, M2`, but only `on M1` is provided. Error.
- **SD4033 protocol_extra_handler**: `on UnknownMsg(...)` for a name not
  in any implemented protocol. Error (slice 4 was a warning SD2026).

Slice-5 implementation: in `items.rs::check_typed`, after collecting
handlers, walk each implemented protocol's message list and check
arity + coverage. Replace the slice-4 warning with the new SD4032/33
errors.

### 3.10 SD3009 `move *ref` proper modelling

Today `HirExpr::Move(HirExpr::Unary { op: Deref, ... })` flows through
the walker without special handling. Slice 5 adds: if the moved
expression is a deref of a reference, emit `SD3009 move_out_of_ref`.

### 3.11 Tighter SD3002 vs SD3008 distinction

SD3002 ("move out of borrow") fires when the borrowed value itself is
moved while a borrow lives. SD3008 ("cannot move borrowed") is the
same condition — slice 4 used SD3002 universally. Slice 5 distinguishes:
- SD3002: the *original owner* moves while a borrow holds.
- SD3008: a third party tries to move via a non-owner path.
Slice 5 implementation: SD3008 triggers when the move expression is in
function-argument position; SD3002 triggers on direct let/return moves.

### 3.12 Field-level borrows (stretch, may defer)

`&mut s.field_a` vs `&mut s.field_b` should be independent. Slice 5
attempt: extend `LocalState` with `borrowed_fields: HashMap<String, BorrowKind>`.
At each `&[mut] place.field` site, record the field name. Cross-field
mut borrows do not conflict.

If implementation pressure forces a cut, defer to slice 6 — keep slice-5
ship-fast.

## 4. Diagnostics (SD4001..SD4099)

| Code   | Name                          | Severity | Example                                                          |
|--------|-------------------------------|----------|------------------------------------------------------------------|
| SD4001 | effect_undeclared             | Error    | `pub fn f() { net.get(url) }` (no `effect net` declared)         |
| SD4002 | alloc_in_core                 | Error    | strict-profile fn allocates via `arena` or container             |
| SD4010 | capability_too_broad          | Error    | callee wants `Fs (ReadOnly, /data)`, caller provides `Fs (Any)`  |
| SD4020 | method_ambiguous              | Error    | `x.hash()` where two traits in scope provide `hash` for `T`      |
| SD4021 | method_not_found              | Error    | `x.fly()` no impl in inherent / trait tables                     |
| SD4022 | trait_coherence_violation     | Error    | `impl Hash for U` declared twice                                 |
| SD4023 | dyn_requires_object_safe      | Error    | `let h: dyn Hash = x` and `Hash` has `Self`-returning method     |
| SD4030 | protocol_arity_mismatch       | Error    | handler has 1 param, protocol message has 2                      |
| SD4031 | protocol_param_type_mismatch  | Error    | handler param used as Int, protocol declares Str                 |
| SD4032 | protocol_missing_handler      | Error    | implements `Fetch (Page, Head)` but only `on Page`               |
| SD4033 | protocol_extra_handler        | Error    | `on UnknownMsg` not in any implemented protocol                  |
| SD4040 | derive_copy_field_not_copy    | Error    | `#[derive(Copy)] struct S { v: String }`                          |
| SD4041 | derive_unknown                | Error    | `#[derive(Foo)]` where `Foo` is not a known derivable trait      |

## 5. Examples — sweep + expected adjustments

Each canonical example must remain `sdust check` clean. We will:

- **04, 06, 11, 18, 19**: explicitly declare `effect`s on public fns
  where the inference says we need them. Slice 5 emits SD4001 on
  hidden effects — fix by adding the effects clause.
- **09**: `driver(logger: Logger, fetcher: Fetcher, url: Url)` is not
  `pub`, so its inferred effects are fine. No change.
- **13**: already declares `effect net, time, alloc` on `fn fetch(...)`
  per spec text — keep as-is.
- **20**: `mount(dom: Dom)` is `export`. Add `effect dom, spawn` to the
  declaration.

Tour pages get a new chapter `15-traits.md`. Capability and effect
chapters added.

## 6. Spec interpretation calls (will land as A22..A30)

- **A22**: Effect inference algorithm. Bottom-up + fixpoint over recursion;
  every `pub fn` requires effects clause superset of inferred set.
- **A23**: Capability narrowing constraints — slice 5 models
  `Any | ReadOnly | Path | Host | And`. Slice 5 rejects broaden-via-cast.
- **A24**: Trait coherence is name-only (no constraint-overlap).
  Inherent impl shadows trait impl.
- **A25**: `dyn Trait` object safety — conservative ban on Self and
  method generics. No coercion sugar; explicit `let h: dyn Trait = expr`.
- **A26**: Derive set for slice 5 — `Copy`, `Hash`, `Eq`. Other derive
  names emit `SD4041 derive_unknown`.
- **A27**: Top-level `sandbox` items have unit type; their body executes
  under sandbox tolerance.
- **A28**: Protocol handler signature strictness — arity, type, missing,
  extra all hard errors.
- **A29**: `move *ref` is now SD3009 (was permissive in slice 4).
- **A30**: Strict-profile (`profile = "core"`) bans `alloc` effect.

## 7. Implementation order

See `docs/superpowers/plans/2026-05-24-slice5-effects.md`.

Broad order:

1. Add `TyData::Cap` + `TyData::Dyn` + capability constraint enum.
2. Prelude registers the five caps with `Cap::Any` constraints.
3. Capability narrowing methods registered in the built-in table.
4. Capability subsumption check at call sites.
5. Effect inference pass (post-typecheck, pre-borrowck).
6. Public-fn effect superset check + strict-profile alloc check.
7. Trait coherence table + dispatch upgrade (SD4020/21/22).
8. `dyn Trait` parsing + lowering + checking.
9. `derive(Copy/Hash/Eq)` parsing + lowering + Copy table entry.
10. Top-level `sandbox` items.
11. Strict protocol handler checks (SD4030/31/32/33).
12. SD3009 `move *ref` modelling + SD3002 vs SD3008 split.
13. Field-level borrow tracking (stretch).
14. SD4xxx + `sdust explain` entries.
15. Negative-test corpus.
16. Driver/CLI wiring (effect inference runs in pipeline).
17. Source edits to examples 04/06/11/18/19/20 + tour pages.
18. Docs: `docs/internals/effects.md`, `docs/internals/capabilities.md`,
    `docs/internals/traits.md`, `docs/tour/15-traits.md`,
    amendments A22..A30, `SLICE5.md`.
19. Tag `v0.5.0-effects`.
