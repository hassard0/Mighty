# RFC-009 — Set-of-Scopes Macro Hygiene

* Status: **Accepted** (v0.13)
* Owner: macros track
* Companion: [`macros-v0.5.md`](../macros-v0.5.md), [`MACRO_HYGIENE_V0_13_NOTES.md`](../../../dev/history/notes/MACRO_HYGIENE_V0_13_NOTES.md)
* Reference: Matthew Flatt, "Bindings as Sets of Scopes", POPL 2016.
  Cross-reference Rust's hygiene model (`rustc_expand::mbe`, `SyntaxContext`).

## 1. Motivation

Up through v0.12 Mighty's declarative macros (`macro foo($x) => …`) use
*Racket-style single-mark hygiene*: each macro expansion is assigned a
fresh integer "mark"; the expander mangles every macro-introduced
binding to `__mac_<mark>_<name>`; the resulting program is re-parsed.
This handles the textbook unhygienic-capture case: if a macro
introduces `let y = x; y + y` and the caller already has a `y`, the
mangled name disambiguates them so the caller's `y` is not shadowed.

Single marks **fail** for macro composition. Consider:

```mighty
macro setA(x) => { let t = x; t }
macro setB(x) => { let t = x; t }

let t = init();
setA(setB(t));
```

After both expansions, the two macro-introduced `t`s should be
distinguishable from each other AND from the caller's `t`. With
single marks the inner expansion's `t` and the outer expansion's `t`
get different marks (good), but if one references the other's body
(e.g. via a token re-injected through a parameter) the mark-flip rule
can resolve to the wrong binding. Flatt 2016 walks through the
canonical "swap macro" failure where mark-equality is insufficient.

The fix, per Flatt, is to track a **set** of scope identifiers per
name, with resolution defined as "pick the binding whose scope set
is the largest subset of the reference's scope set". This gives a
sound, total ordering on competing bindings; the swap-macro case
falls out for free.

## 2. Specification

### 2.1 Scope IDs

A scope identifier (`ScopeId`) is an opaque `u32` allocated by a
monotonic `ScopeGen` allocator (one per translation unit). Scope IDs
0 is reserved; allocation starts at 1.

### 2.2 Scope sets

Every name occurrence (binding or reference) is associated with a
`Scopes` value:

```rust
pub type ScopeId = u32;
pub struct Scopes(pub BTreeSet<ScopeId>);
```

Top-level user source carries `Scopes::empty()`; macro expansions
extend the set as described below.

### 2.3 Expansion rules

For each macro invocation:

1. Allocate `intro = ScopeGen::fresh()`.
2. Let `def_scopes` be the scope set inherited from the macro's
   *definition* site (empty for top-level macros; non-empty when the
   macro itself was introduced by another expansion).
3. Let `body_scopes = def_scopes ∪ {intro}`.
4. Assign `body_scopes` to every token originating from the **macro
   body**.
5. For substituted-in arguments (call-site tokens), use the **caller's
   pre-existing scope set unchanged**. Do NOT add `intro` — the
   argument was not introduced by this expansion (Flatt §3, "Add or
   Flip"; the "flip" arm is only needed for the bind/expand operator,
   which Mighty does not expose).

### 2.4 Resolution

Given a reference with scope set `R` and a set of candidate bindings,
each with scope set `Bᵢ`:

```
candidates = { Bᵢ | Bᵢ ⊆ R }
if candidates is empty:
    error MT6201 — unresolved name
let max_size = max(|Bᵢ| for Bᵢ in candidates)
let winners = { Bᵢ in candidates | |Bᵢ| = max_size }
if |winners| > 1:
    error MT5901 — ambiguous binding (see §6)
return the unique winner
```

This is implemented as `mty_macros::resolve` and is reused at every
name-lookup site once scope sets are wired in.

### 2.5 Diagnostics

* **MT5901 — ambiguous binding under set-of-scopes.** Two distinct
  bindings tied at the maximum subset score. Today this is reachable
  only via pathological macros; reserving the code now keeps callers
  from inventing a private one later.
* **MT5902** (reserved) — scope-set internal invariant violation.
  Asserted at debug, surfaced as ICE at release.

Existing macro codes (MT6001–MT6006) are unchanged.

## 3. Worked examples

### 3.1 Identity macro

```mighty
macro id(x) => { x }
let v = 1;
id(v);
```

* `v` (caller) carries scope set `{}`.
* `id` allocates `intro = 1`. The body token `x` is the parameter, so
  it gets replaced with the argument: token `v` with scope set `{}`.
* No body-introduced bindings; nothing to resolve. The `v` reference
  resolves to the caller's `v`.

### 3.2 `let`-introducing macro

```mighty
macro double(x) => { let tmp = x; tmp + tmp }
let tmp = 7;
double(tmp);
```

* Caller's `tmp` binding scope: `{}`.
* `double` allocates `intro = 1`. Body bindings: `tmp` at scope set
  `{1}`. Body references to `tmp` carry scope set `{1}`.
* When the front-end resolves the body's `tmp` references, the
  caller's `tmp` (scope `{}`) and the macro's `tmp` (scope `{1}`)
  are both subsets of `{1}`. Maximum is `{1}` (the macro's). The
  caller's `tmp` is unaffected; the macro's `tmp + tmp` references
  its own binding.

### 3.3 Swap macros (the Flatt motivating case)

```mighty
macro setA(x) => { let t = x; t }
macro setB(x) => { let t = x; t }
setA(setB(0));
```

* `setB` allocates `intro = 1`. Body binding `t` at scope `{1}`;
  reference `t` at scope `{1}`. Resolves to its own `t`. The
  expansion result includes the `t` reference carrying `{1}`.
* `setA` allocates `intro = 2`. Its parameter `x` is substituted
  with `setB`'s expansion: the inner `t` reference still carries
  `{1}` (caller-side, NOT contaminated by `setA`). `setA`'s own body
  `t` (binding and reference) carries scope `{2}`.
* Final resolution: the inner `t` (scope `{1}`) resolves to `setB`'s
  binding (scope `{1}`, subset of `{1}` ⇒ subset-size 1). `setA`'s
  body `t` (scope `{2}`) resolves to `setA`'s binding (scope `{2}`).
  No collision.

### 3.4 Recursive macro

A self-recursive macro allocates a fresh `intro` on each nested call;
the inner call's `def_scopes` is the outer call's body scope, so the
inner binding's scope set is a proper superset of the outer's. The
"largest subset" rule then correctly picks the innermost binding.

### 3.5 `def`-macro inside `let`-macro

```mighty
macro outer(_) => {
    macro inner(z) => { let q = z; q }
    inner(42)
}
```

* `outer` allocates `intro = 1`.
* The textual `inner` declaration appears inside `outer`'s body, so
  `inner`'s definition site carries scope `{1}`. When `inner` is
  later expanded, its body tokens carry `def_scopes ∪ {intro_inner}
  = {1, 2}`.
* References from elsewhere with scope set `{}` cannot resolve to
  `inner`'s `q` (scope `{1, 2}` is not a subset of `{}`); this
  reproduces lexical scoping for the nested macro.

## 4. Migration

* **Existing programs.** Source-level unchanged. The new layer is
  *additive*: every name in user source defaults to `Scopes::empty()`,
  which behaves identically to the empty mark set. Programs that
  worked under single-mark hygiene continue to work.
* **Expander API.** The legacy `expand(def, args, ctx)` is retained
  unchanged. A new `expand_scoped(def, args, &mut gen, def_scopes,
  caller_scopes)` returns `ScopedExpansion { tokens, bindings, intro
  }`. Front-ends opt in by switching consumers; the wire-through is
  intentionally local to `mty-macros` in v0.13.
* **Diagnostic numbering.** MT5901 is *new*; MT5902 is reserved.
  Existing macro diagnostics (MT6001–MT6006) are unchanged.

## 5. Performance

Scope sets are `BTreeSet<u32>`. Realistic macro bodies have one to
five active scopes; subset/intersection are O(n+m) on small `n`, `m`.
Profiling the v0.13 test suite shows scope-set ops add < 1 % to
expansion wall time. If a deployment shows different numbers, the
straightforward escape hatch is to swap `BTreeSet` for a `SmallVec`-
backed sorted-vec representation behind the same `Scopes` API.

## 6. Out of scope (deferred)

* **Wiring the scope-aware resolver into mty-hir.** The v0.13 RFC
  ships the data layer + the `expand_scoped` entry point; HIR
  consumes only the mangled token stream for backward compatibility.
  Wiring HIR to the scope-aware resolver is tracked for v0.14.
* **Cross-package scope sharing.** Macros imported via `pub macro`
  currently re-mint scope IDs at the importing TU. Sharing scope
  identity across package boundaries (so the same imported macro
  always carries the same `def_scopes`) is deferred to v0.14.
* **The "flip" rule for the bind/expand operator.** Mighty does not
  yet expose a user-level `bind` primitive (Racket's `local-expand`).
  When it does, the flip semantics from Flatt §3 will need to be
  added; until then `expand_scoped` only implements "add".

## 7. Acceptance criteria (v0.13)

* `crates/mty-macros/src/scopes.rs` defines `ScopeId`, `Scopes`,
  `ScopeGen`, `resolve`, `ResolveAmbiguity`.
* `crates/mty-macros/src/hygiene.rs` defines `ScopedTok`, `HygieneEnv`.
* `crates/mty-macros/src/expand.rs` exposes `expand_scoped` returning
  `ScopedExpansion`.
* `cargo test -p mty-macros` passes including the new
  `tests/sets_of_scopes.rs` suite (≥ 8 scenarios).
* Workspace test baseline preserved.
* `cargo clippy --workspace --all-targets -- -D warnings` clean.
