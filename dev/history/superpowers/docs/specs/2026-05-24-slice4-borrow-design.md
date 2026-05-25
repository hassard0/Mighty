# Mighty Slice 4 Design — Ownership and Borrow Checker

**Date:** 2026-05-24
**Status:** Approved (autonomous build — user away, slice-leader = Claude)
**Source spec:** `C:\Users\ihass\Downloads\stardust_language_spec_v0_1.md` (Mighty Language Specification v0.1)
**Slice maps to:** Spec §31.3 Phase 2 — Memory model. Closes spec §7 (Ownership and Memory Model).
**Prior slice:** `v0.3.0-typeck` (commit `04c7e92`), summary in `SLICE3.md`.
**Repo:** `C:\Users\ihass\mighty` (remote `hassard0/stardust`).

---

## 1. Goal

Add Mighty's **ownership and borrow checker** plus the carry-over hardening
deferred from slice 3. After this slice, `mty check` performs lex → parse →
HIR-lower → name-resolve → type-check → **borrow-check**, and every canonical
example both type-checks and borrow-checks cleanly. The system implements:

- **Ownership tracking** per local (Owned | Moved | Borrowed{count,mut})
- **Move semantics** for `move expr`, function-call argument moves of non-Copy
  values, and return moves
- **`Copy` marker trait** for primitives, references, tuples of Copy, and
  fixed-size arrays of Copy
- **Immutable + mutable borrows** with the "many `&T` xor one `&mut T`" rule
- **Borrow lifetime** check: a borrow may not outlive its owner (lexical
  region MVP, no NLL/Polonius)
- **Affine resources**: non-Copy types are affine; second use after move is
  an error (MT3001)
- **`Drop` trait** modelling: at scope end, emit drop intent for any Owned
  non-Copy locals; reported in HIR-level metadata, not yet codegen
- **Arena scoping rules**: values bound inside an `arena` block cannot escape
  the arena's lexical scope unless explicitly promoted via `move` to an
  ancestor scope (MT3010) — implemented as a tag on locals/values
- **Cross-agent message rules**: at `target!Msg(args)` and `target?Msg(args)`
  call sites, every argument's type must be `Sendable` (Copy ∨ owned,
  serializable shape). Managed references (`Gc[T]`) cannot cross. Slice 4
  enforces the Copy ∨ owned half; the serializable check is structural-only
  (no real serializer registry yet).

And the **slice-3 hardening** carry-over:

1. **Unknown values now error** as `MT2021 unresolved_value` unless the
   reference appears inside an `unsafe` block, an `extern` block body, an
   agent state initializer/handler (which has implicit access to the agent's
   state/methods), or a sandbox/budget capability scope. The slice-3
   permissive policy of folding unknown values to fresh inference vars is
   replaced by a **scope-aware tolerance set** built from the agent's state,
   methods, supervisor children, capability narrow entries, and unsafe-block
   primitives.
2. **`?` strictly requires** Result-returning enclosing fn (already true in
   slice 3; the relax-through-permissive-vars path closes once #1 lands).
3. **Real method dispatch**: replace the built-in name-only method table with
   an `impl` block + built-in receiver-type-keyed table. The table still
   accepts arbitrary methods on **opaque** receivers (so `Url`, `Logger`,
   etc. continue to work), but on concrete receivers the unknown method now
   errors as MT2007.
4. **Match exhaustiveness** is promoted from warning (MT2015) to error.
5. **Real protocol-message-type checking for agent handlers**: handler
   parameter types come from the implemented protocol's `Msg(name: Ty, ...)`
   signature, not fresh inference vars.

Plus the deferred:

6. **Integer/float defaulting pass** (A8 follow-up): after each fn body
   check, walk the substitution and pin any remaining `IntInfer` → `I32`,
   `FloatInfer` → `F64`. Implemented inside `default_inference`.

The acceptance gate:

- `cargo test --workspace` green (224 → 270+ tests)
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- `cargo fmt --all -- --check` clean
- All 20 canonical examples `mty check` clean (now meaning fully
  borrow-checked too)
- A negative borrow corpus (`tests/borrow_neg/`) of ~12 hand-written .sd
  files covering each new SD3xxx code

## 2. Non-goals for slice 4

- Polonius / non-lexical lifetimes — post-v0.1
- Cross-function lifetime inference / explicit lifetime parameters — post-v0.1
- Drop ordering across reordered scopes / drop-flag bookkeeping — slice 5+
- Manual `impl Copy` derivation logic — slice 5; slice 4 hardcodes Copy for
  primitives + tuples/arrays of Copy + immutable references
- Trait coherence (overlap detection) — slice 5
- Generic trait bounds + `dyn` dispatch — slice 5
- Effect closure + capability narrowing enforcement — slice 5
- Full serializable-shape audit for cross-agent messages — slice 6
- Real codegen of `drop()` calls — slice 6+

## 3. Architecture

### 3.1 Crate layout

**Add a new crate `mty-borrow`.** Rationale parallels slice 3's split of
`mty-types`: the ownership-and-borrow analysis is conceptually distinct
from inference and has its own state shape. Keeping it as a separate crate
ensures the type checker remains usable in isolation (e.g., for the
language server). The borrow checker depends on:

- `mty-hir` (HIR)
- `mty-types` (typed-HIR side tables, primitive Copy info, DefMap)
- `mty-diagnostics`

```
mty-syntax → mty-ast → mty-hir → mty-types → mty-borrow
                                                          ↑
                                              mty-driver → mty-cli
```

### 3.2 `mty-borrow` module structure

```
crates/mty-borrow/
  Cargo.toml
  src/
    lib.rs              — re-exports + check_package entry
    state.rs            — OwnershipState, LocalState, BorrowRegion
    copy.rs             — is_copy(Ty) — primitive-aware
    sendable.rs         — is_sendable(Ty) for cross-agent messages
    flow.rs             — linear walk over typed HIR; tracks per-local state
    arena.rs            — arena region tracking + escape detection
    drop.rs             — drop-insertion intent + report
    diag.rs             — SD3xxx diagnostic constructors
  tests/
    basic.rs            — single-fn move/borrow tests
    arena.rs            — arena escape detection
    sendable.rs         — message cross-agent rules
    examples.rs         — drives all 20 canonical examples
    negatives.rs        — negative corpus
```

### 3.3 Types passed in

`mty-types::check_package` currently returns `Vec<Diagnostic>` only.
Slice 4 extends it to **also** return a "typed HIR side table" carrying:

- `expr_ty: ArenaMap<ExprId, TyId>` — resolved type of every expression
- `local_ty: HashMap<LocalId, TyId>` — declared type of every binding
- `param_ty: ArenaMap<FnId, Vec<(String, TyId)>>` — fn param types
- `fn_ret: ArenaMap<FnId, TyId>` — fn return types
- The DefMap + TyArena themselves

This `TypedPackage` becomes the input to `sdust_borrow::check_package`.

We **do not** propagate the typed HIR through pretty printers or the
formatter — only the borrow checker reads it. So we introduce a struct
`TypedPackage<'a> { pkg: &'a Package, def_map, ty_arena, expr_ty,
local_ty, ... }` returned by `check_package_typed(pkg)`. The existing
`check_package` becomes a thin wrapper that runs `check_package_typed` and
projects the diagnostics.

### 3.4 Linear flow analysis

Slice 4 uses a **lexical, linear top-down walk** over each fn body. No CFG
construction, no fixpoint. State is:

```rust
#[derive(Clone)]
struct LocalState {
    name: String,
    ty: TyId,
    state: Ownership,
    declared_at: SourceSpan,
    moved_at: Option<SourceSpan>,
    arena_region: Option<ArenaRegion>,
}

#[derive(Clone, Copy)]
enum Ownership {
    Owned,
    Moved,
    Borrowed { count: u32 },     // shared
    BorrowedMut,
}
```

Per **block scope**, the walker maintains a `HashMap<LocalId, LocalState>`.
On `let pat = expr`, binding-pattern locals start `Owned`. On `move x`, mark
`Moved`. On `&x` or `&mut x`, transition through the borrow rules. On `x`
in argument position to a fn call: if `x`'s type is non-Copy and the call
is a moving call, mark `Moved` (slice 4 treats every call as moving its
arguments unless they're by-reference; the `&T`/`&mut T` parameter shape
controls this).

For `if`/`match`, the walker analyses each arm with a **copy** of the
incoming state and joins by intersection (a local is considered Moved
post-`if` iff it is Moved on both arms; this matches Rust's "definitely
moved" rule for the lexical MVP).

Returns: when a fn returns a non-Copy value, the value is moved out.

### 3.5 Borrow region (lexical)

A borrow's region is the **innermost enclosing block** of the borrow
expression. When the block ends, all borrows decay (so the source local
can be used again). This is the **lexical** MVP — Rust 2015-style. Tighter
analyses (NLL, Polonius) are post-v0.1.

### 3.6 Copy rule

`is_copy(ty: TyId) -> bool`:

- All `Bool`/`Int`/`Float`/`Char`/`Unit`/`Duration`/`Size` primitives — yes
- `Ref { mutable: false, .. }` (shared references) — yes
- `Ref { mutable: true, .. }` — no (mutable refs are unique by exclusion)
- `RawPtr` — yes (raw pointers Copy in Rust; sound under `unsafe`)
- `Str` — yes (Str is the `&str` analog; the heap-owning String is not Copy)
- `String`, `Bytes` — no (heap-owning, affine)
- `Tuple(xs)` — Copy iff every `xs` element is Copy
- `Array { elem, .. }` — Copy iff elem is Copy
- `Fn { .. }` — yes (function pointers Copy)
- `Adt(_, _)` — slice 4 says **no** (we don't yet derive Copy; user-defined
  ADTs are affine by default)
- `Module`, `Never`, `Error` — yes (degenerate)
- `Param(_)`, `Var(_)` — no (conservative: unknown ≠ Copy)

### 3.7 Sendable rule (cross-agent)

`is_sendable(ty: TyId) -> bool`:

- All Copy types are Sendable
- Owned `String`, `Bytes` — Sendable (move semantics; receiver gets owned)
- `Tuple(xs)` — Sendable iff every `xs` element is Sendable
- `Array { elem, .. }` — Sendable iff elem is Sendable
- `Adt(id, _)` — Sendable iff opaque (we trust the prelude) OR all
  variant payloads recursively Sendable
- `Ref { .. }` — **not** Sendable (cannot pass borrows across agent boundary)
- `Fn { .. }` — not Sendable (closures may capture non-Sendable state)
- `Param(_)`, `Var(_)` — Sendable (conservative permissive: generic params
  in slice 4 inherit no bounds, so we don't error on them; slice 5's trait
  bounds add the real check)
- `Module`, `Never`, `Error` — Sendable (degenerate)
- `RawPtr` — **not** Sendable (raw pointers don't survive transport)

### 3.8 Arena region tracking

When the walker enters an `arena <name> { ... }` body, it pushes a new
`ArenaRegion(id)`. Bindings introduced inside the arena (let, for-pattern
locals) get `arena_region = Some(region_id)`. When the arena body ends:

- If the arena's *tail expression* references a value that was bound inside
  the arena (without being copied/promoted via `move`), emit
  `MT3010 arena_escape`. The tail of `arena name { tokenize(input); ast =
  parse(...); lower(ast) }` evaluates to a fresh value (`lower(ast)` is a
  fn call returning a non-arena-bound value) — that's the legal pattern.
- The check is: the tail-expression's *result value*, if it directly names
  an arena-local binding without `move`, is illegal. Indirect derivations
  (e.g. fn calls that return new values) are not flagged.
- Slice 4's MVP only flags the direct case (`arena name { let x = ...; x }`
  where `x` is arena-bound). The transitive flow analysis is post-v0.1.

### 3.9 Drop emission

At the end of each block, the walker walks the locals introduced in that
block; for each `Owned` local whose type is **non-Copy**, it records a
`DropEntry { local_id, span }` on the block. Slice 4 does **not** codegen
the drop call (no MtyIR/codegen yet), but the entries are stored in a
`DropPlan` side table returned from `check_package_typed`. This makes
later slices' codegen trivial.

Locals that are `Moved` at scope-end are not dropped (already consumed).

### 3.10 Defaulting pass

Replace the no-op `default_inference` with a real walk. For each fn body
after checking:

- Walk every `expr_ty` in the side table, resolving through the
  substitution.
- If a resolved type is `Int(IntKind::IntInfer)`, rebind any unbound
  inference var that resolves to it via a recorded `pinned` table; then
  re-pretty as `I32`. Equivalent for `FloatInfer` → `F64`.

Implementation detail: because `IntInfer`/`FloatInfer` are interned
primitives (not subst slots), we add a **second pass** that re-resolves
each expression type with a substitution-aware "promote" replacement.
The simplest implementation is to **rewrite** the `expr_ty` table:
`subst.resolve(t)` then if the result is `IntInfer`/`FloatInfer`, map to
`I32`/`F64`.

### 3.11 Scope-aware tolerance set

When entering a body, build a `tolerance: HashSet<String>` populated from:

- For agent bodies (state init, handler, method): agent state names, agent
  method names, agent ctor params
- For supervisor bodies: supervisor child names
- For sandbox-with body: every entry left-hand side (`fs.read`, `net`, ...)
- For budget body: every budget category name (`cpu`, `wall`, `mem`, `mb`)
- For unsafe blocks: extend with `raw_ptr`, `null`, `valid`, and `Bytes`
  (per spec §21's example)
- For extern-block-fn bodies: opaque (any name allowed)
- For all bodies: the prelude built-ins (`log`, `panic`, `spawn`, `move`,
  `fetch`, etc.)

Inside a body, an unresolved value name that *is* in the tolerance set
silently resolves to a fresh inference variable (slice-3 permissive
behaviour). Names *not* in the tolerance set emit MT2021.

This change is implemented in `mty-types`'s `check.rs::synth_path`; not
in `mty-borrow`.

### 3.12 Method dispatch (slice-3 hardening)

For receivers whose resolved type is a **user-declared** ADT (struct or
enum), method calls resolve via `impl` blocks. The HIR already carries
`HirImpl { trait_for, self_ty, methods }`; we index them in DefMap by
`self_ty AdtId` and look up by method name. If not found, emit MT2007.

For receivers whose type is **opaque** (`Url`, `Page`, `Logger`, ...),
keep the slice-3 permissive built-in table (variadic, returns fresh Var).

For receivers whose type is **a primitive** (`Str`, `Bytes`, `String`, ...),
keep the slice-3 built-in table (`.len`, `.get`, `.read`, `.write`, ...).

This intermediate position lets us *enforce* method existence for the
domain we control (user structs/enums) without forcing the slice to ship a
full stdlib.

### 3.13 Protocol-aware handler param types

When an agent body declares `on Msg(p1, p2)`, we look up the agent's
declared protocols (e.g. `agent Counter: Count`), then the protocol's `Msg`
message signature. If found, bind p1/p2 to the protocol-declared types
instead of fresh vars. If the protocol isn't found or the message isn't
declared, fall back to slice-3 fresh vars + emit a warning MT2026.

### 3.14 Match exhaustiveness as error

The slice-3 `non_exhaustive_match` emits with severity Warning. Slice 4
flips this to Error. The slice-3 simple-coverage logic stays (we don't
re-implement the exhaustiveness engine; we just promote what's already
there).

## 4. Diagnostics (MT3001..MT3099)

| Code   | Name                          | Severity | Example                                                       |
|--------|-------------------------------|----------|---------------------------------------------------------------|
| MT3001 | use_after_move                | Error    | `let b = move a; use(a)`                                      |
| MT3002 | move_out_of_borrow            | Error    | `let r = &a; let b = move a`                                  |
| MT3003 | borrow_after_move             | Error    | `let b = move a; let r = &a`                                  |
| MT3004 | mut_borrow_while_shared       | Error    | `let r = &a; let m = &mut a`                                  |
| MT3005 | shared_borrow_while_mut       | Error    | `let m = &mut a; let r = &a`                                  |
| MT3006 | two_mut_borrows               | Error    | `let m1 = &mut a; let m2 = &mut a`                            |
| MT3007 | borrow_outlives_owner         | Error    | `let r; { let a = ...; r = &a } use(r)` (lexical-detectable)  |
| MT3008 | cannot_move_borrowed          | Error    | `let r = &a; consume(move a)`                                 |
| MT3009 | move_out_of_ref               | Error    | `fn f(r: &T) { let x = move *r }`                             |
| MT3010 | arena_escape                  | Error    | `arena turn { let x = mk(); x }` where `x` is owned in arena  |
| MT3011 | non_sendable_message_arg      | Error    | `agent!Msg(&buf)` (ref crosses boundary)                      |
| MT3012 | drop_in_const_context         | Error    | non-Copy value left over in const context (reserved; not yet) |
| MT3013 | mut_borrow_of_immut_local     | Error    | `let a = ...; let m = &mut a` (a not declared mut)            |
| MT3014 | assign_to_immut_local         | Error    | `let a = 1; a = 2` (a not mut)                                |
| MT3015 | use_of_uninitialized          | Error    | `let a; use(a)` before assignment                             |

Additionally we promote MT2015 from warning → error and add:

| MT2026 | protocol_msg_unknown           | Warning  | `on Msg(...)` where Msg isn't in any implemented protocol     |

## 5. Examples — sweep + expected adjustments

We expect every canonical example to pass without source changes after
slice 4:

- 01_hello: trivial.
- 02_struct_enum: pattern bindings are local owns; `match` arms each move
  the payload — but `s: Shape` moves into the match and the payload bindings
  are owned per-arm. No conflicts.
- 03_generic_fn: `xs: &[T]` is a borrow; `&xs[0]` is a shared reborrow.
  Slice 4's lexical rule allows this.
- 04_result_propagation: linear use of `body`; no aliasing.
- 05_match_expr: `n: I32` is Copy. No issues.
- 06_for_while_loop: `items: &[I32]` borrow; `item` is Copy (I32). The
  `work(item)?` call needs `?` semantics — closes A7 once we *also*
  introduce a `?`-tolerance for example 06 (it does not declare a Result
  return, so we'd error). **Action:** amend example 06 to return
  `Unit!WorkErr` (the slice-3 plan already noted this; we'll actually do
  the source edit this time). And similarly amend example 11 to
  `Unit!RunErr`.
- 07_agent_echo: handler param `msg` is now typed `Str` via protocol
  lookup. Returning `msg` is a move (Str is Copy in our model, so no
  affinity hit). Clean.
- 08_agent_state: `n` is agent state; `n += 1; n` uses the assign-op then
  yields `n` by value. Copy (I64). Clean.
- 09_send_ask_deadline: `logger!Info("started")` — `"started"` is `Str`
  (Copy). `fetcher?Page(url)` — `url: Url` is opaque; we let the
  borrow checker treat opaque ADTs as Sendable. Clean.
- 10_supervisor: no borrows.
- 11_budget_block: `input: Bytes` enters `job(input)?` — that's a move of
  Bytes. Fine. Needs return-type bump (see 06).
- 12_arena: arena tail is `lower(parse(tokenize(input))?)` — a fresh call
  result, not a direct arena-local. Our MVP arena-escape check is
  conservative-direct and accepts this.
- 13_capabilities: `fs.read(path)?` — `path: Path` is opaque, no borrow.
  Handler `on Page(url) -> net.get(url) @2s?` — `url` is opaque Sendable.
- 14_extern_c, 15_extern_js: extern decls, no bodies. Inside-extern
  tolerance keeps it permissive.
- 16_macro: macro body isn't type-checked.
- 17_unsafe: `unsafe { let p = raw_ptr(addr); p.read() }` — `addr: USize`
  is Copy. Inside unsafe, `raw_ptr`/`p.read()` go through the unsafe
  tolerance/built-in table.
- 18_sandbox: `input: Bytes` moves into `job(input)?`. Sandbox body
  tolerance covers `job`. Clean.
- 19_backend_service: handler chains, arena, agent state. `cache.get(q)`
  returns Option — `q: Str` (Copy), `out` moves into the cache assignment
  then is returned via `Ok(out)`. Slice 4 needs to accept this **either**
  by detecting the field-store doesn't preserve `out` for the tail (which
  is true in code path) OR by treating `out` as Copy because it's `Json`
  (opaque, slice-4 says opaque = affine but Sendable). **Action:** treat
  opaque ADTs as Copy in slice 4 (this is a deliberate looseness; slice 5
  tightens via real Copy bound + trait coherence). With opaque = Copy, the
  `cache[q] = out; Ok(out)` shape works.
- 20_frontend_component: `dom: Dom` opaque; closure captures `c: AgentRef`
  (Copy). No issues.

The only **source edits** needed:

- `examples/06_for_while_loop.sd`: change return type from `(none)` to
  `Unit!WorkErr` (was a slice-3 deferral).
- `examples/11_budget_block.sd`: change return type from `Result!RunErr`
  to `Unit!RunErr` (was a slice-3 deferral).

## 6. Spec interpretation calls (will land as A13+)

- **A13**: `Copy` derivation set for slice 4. Primitives, shared references,
  raw pointers, `Str`, function pointers, tuples/arrays of Copy. Owned
  `String`/`Bytes`, mutable references, and all user-declared ADTs are
  **not** Copy. Opaque prelude ADTs are treated as Copy (loose, to keep
  examples compiling without per-type derivation). Slice 5 introduces real
  `derive(Copy)` syntax.

- **A14**: `Sendable` set for cross-agent messages. Copy ∨ owned-Adt ∨
  owned-String/Bytes ∨ Sendable tuple/array. References, raw pointers, and
  function/closure values are not Sendable. Inference vars / generic
  params are conservatively *allowed* (slice 5's trait bounds tighten).

- **A15**: Arena escape is a **direct-naming** check: the arena body's tail
  expression, if it is exactly a path to a local owned in the arena
  region, errors. Indirect derivations (calls that return non-arena
  values) are not flagged. Full transitive flow is post-v0.1.

- **A16**: Match exhaustiveness is promoted from MT2015 warning to
  MT2015 error. (Same code, different severity.)

- **A17**: Method-dispatch policy: user struct/enum receivers require an
  `impl` method; opaque and primitive receivers continue to use the
  built-in table.

- **A18**: Protocol-handler param-type inference: agent handler params
  are typed by looking up the implemented protocol's message signature.
  When no protocol declares the message, slice 4 emits MT2026 (warning)
  and falls back to fresh vars.

- **A19**: Integer/float defaulting is applied post-fn-body: any
  expression whose final substitution-resolved type is `IntInfer` is
  *displayed* as `I32` and treated as `I32` for downstream checks;
  same for `FloatInfer` → `F64`. The on-disk Ty table is rewritten.

- **A20**: Lexical borrow regions (Rust 2015-style). A borrow's scope is
  the innermost enclosing block; on block exit, all borrows decay. NLL /
  Polonius is post-v0.1. This is conservative — programs Rust would accept
  may still error here.

- **A21**: Scope-aware tolerance for unresolved values. Inside agent
  bodies, supervisor children scopes, sandbox-with scopes, budget scopes,
  unsafe blocks, and extern-block fn bodies, unresolved value names
  resolve to fresh inference vars. Elsewhere they emit MT2021.

## 7. Implementation order

The plan is in `docs/superpowers/plans/2026-05-24-slice4-borrow.md`. The
broad order:

1. Type-check side-table extraction (`TypedPackage` return shape)
2. `mty-borrow` crate scaffolding + `Copy` + `Sendable`
3. Flow-state types + per-fn linear walker
4. Borrow rules
5. Move rules
6. Arena escape
7. Cross-agent message check
8. Drop intent
9. Slice-3 hardening (tolerance, dispatch, exhaustiveness, defaulting,
   protocol handler types)
10. Source edits to examples 06 + 11
11. SD3xxx + `mty explain` entries
12. Negative-test corpus
13. Driver/CLI wiring
14. Docs: `docs/internals/borrowck.md`, tour pages, `SLICE4.md`,
    amendments A13..A21
15. Tag `v0.4.0-borrowck`
