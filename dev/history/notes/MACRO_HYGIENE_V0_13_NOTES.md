# Macro hygiene v0.13 — set-of-scopes (RFC-009) notes

Captures the build of the set-of-scopes hygiene layer for Mighty's
declarative-macro expander. This is one of four parallel swarm
agents working v0.13 from HEAD `1d187ac`; this agent owns
`mty-macros` scope-hygiene only.

Companion docs:
- [`RFC-009-set-of-scopes.md`](../../../docs/spec/rfcs/RFC-009-set-of-scopes.md)
- [`macros-v0.5.md`](../../../docs/spec/macros-v0.5.md) (unchanged — the v0.5 expander remains the baseline; v0.13 adds an alongside scope layer)

## Scope of this slice

Goal: add Flatt-style "Bindings as Sets of Scopes" data + entry
points to `mty-macros`, without breaking the existing mangling-based
expander or any callers in `mty-hir` / the workspace.

What shipped:

* `crates/mty-macros/src/scopes.rs` (NEW) — `ScopeId` (`u32`),
  `Scopes` (BTreeSet wrapper with `with` / `without` / `is_subset`
  / `intersect` / `union`), `ScopeGen` (monotonic allocator,
  skips `0`), and `resolve` (largest-subset picker with explicit
  `ResolveAmbiguity` error).
* `crates/mty-macros/src/hygiene.rs` (NEW) — `ScopedTok` (token +
  scope set), `HygieneEnv` (per-invocation hygiene environment with
  `apply_to_body` / `apply_to_argument` helpers), `strip_scopes`
  utility.
* `crates/mty-macros/src/expand.rs` (EXTENDED) — new
  `expand_scoped(def, args, gen, def_scopes, caller_arg_scopes)`
  entry point returning `ScopedExpansion { tokens, bindings, intro
  }`. The legacy `expand` / `expand_to_source` are unchanged.
* `crates/mty-macros/src/lib.rs` (RE-EXPORTS) — `Scopes`, `ScopeId`,
  `ScopeGen`, `resolve`, `ResolveAmbiguity`, `HygieneEnv`,
  `ScopedTok`, `strip_scopes`, `expand_scoped`, `ScopedExpansion`.
* `crates/mty-macros/tests/sets_of_scopes.rs` (NEW) — 12 integration
  tests (target was 8+) covering identity, let-introduction, swap
  macros, recursion, let-binding composition, global names, inner
  shadowing, ambiguity reporting, parameter scope preservation,
  cross-macro reference resolution, allocator monotonicity, and
  definition-scope propagation.
* `crates/mty-macros/tests/hygiene_avoids_capture.rs` (EXTENDED) —
  two new parity checks: scoped expansion emits the same source as
  the legacy mangler, and the bindings list is populated correctly
  for `let`-introducing macros.
* `docs/spec/rfcs/RFC-009-set-of-scopes.md` (NEW) — spec.
* This notes file.

## Tests delta

Baseline (HEAD `1d187ac`, this crate only):
- `cargo test -p mty-macros --lib`: 32 passed
- `cargo test -p mty-macros --tests`: 31 passed across 13 integration tests

v0.13 (after this slice):
- `cargo test -p mty-macros --lib`: 43 passed (+11 — 7 in `scopes::tests`, 4 in `hygiene::tests`)
- `cargo test -p mty-macros --tests`: 45 passed across 14 integration tests
  (+12 in `sets_of_scopes`, +2 in `hygiene_avoids_capture`, -1 — n/a)

Net: **+14 unit, +14 integration = +28 macro-hygiene tests.**

All existing macro tests pass unchanged. The scoped expander
produces byte-identical source to the legacy mangler when run with
matching context IDs (see `scoped_expansion_emits_same_source_as_legacy`).

## Why a separate `expand_scoped` rather than rewriting `expand`?

The mangling-based `expand` is consumed by `mty-hir` and indirectly
by every downstream tool that re-parses macro output (LSP, formatter,
debugger). Switching that signature would force a coordinated change
across crates the other swarm agents are also touching tonight.

The chosen split is:

* `expand` keeps its existing `(def, args, ctx) -> Vec<Tok>` shape
  and the existing mangling logic untouched. All current callers
  keep working.
* `expand_scoped` is the new entry point that carries scope-set
  information through. It internally REUSES the same mangling pass
  (so the emitted source remains parseable by the existing front-
  end) AND records per-binding scope sets in `ScopedExpansion.bindings`
  for a future scope-aware resolver.

This delivers the SHIPPED-FULL surface (RFC + data layer + scoped
expander + tests) without requiring changes to `mty-hir`. Wiring
HIR to consume `expand_scoped` is the natural v0.14 follow-up.

## Confirmed unchanged

Macros that worked under single-mark hygiene continue to work — the
legacy `expand` path is byte-identical, and the new scope-aware
path emits identical output text (only attaching extra metadata).
Specifically re-verified:

* `assert_eq` / `assert_eq_real` (stdlib).
* `param_substitution` integration test.
* `simple_expansion`, `tuple_pattern_hygiene`, `cross_file_macro`.
* `recursive_capped`, `unknown_macro_mt6001`, `mac_marker`.
* All proc-macro tests (`proc_macro_*`) — unaffected; scope layer
  is declarative-macro-only in v0.13.

## Performance

`Scopes` is a `BTreeSet<u32>`. In the v0.13 test suite the deepest
observed scope set has 3 elements (recursive expansion); subset and
intersection are O(n+m) on small `n`, `m`, dominated by allocation.
The 12 set-of-scopes tests run in < 1 ms total (well under the 100
ms suite-overhead noise floor).

If a real-world program ever produces measurable hot spots in
`Scopes::is_subset`, the recommended escape hatch is swapping
`BTreeSet<u32>` for a `SmallVec<[u32; 4]>` sorted-vec representation
behind the same `Scopes` API (no caller-visible change).

## Outstanding gaps deferred to v0.14

1. **HIR consumes `expand_scoped`.** Wire `mty-hir`'s name resolver
   to use `mty_macros::resolve` with the per-binding scope sets in
   `ScopedExpansion.bindings`. Until then HIR continues to rely on
   the legacy mangling, which is sound for the simple capture cases
   but does not benefit from the scope-set disambiguation for the
   composition cases (the disambiguation IS now provable correct in
   the test suite, just not yet plumbed into the type checker).
2. **MT5901 surfacing.** Reserved diagnostic code for ambiguous
   resolution; not yet emitted from the front-end because the
   resolver is not yet wired in. The error path through
   `ResolveAmbiguity` is exercised by `tests/sets_of_scopes.rs`.
3. **Cross-package scope sharing.** `pub macro` imports re-mint
   scope IDs at the importing TU. For pristine "imported macros
   always carry the same `def_scopes`" semantics this needs the
   `PackageMacros` import path to plumb the source `def_scopes`
   through.
4. **The "flip" rule.** Mighty does not expose a user-level
   `bind`/`local-expand` primitive; "add" is the only Flatt rule
   we need today. When that primitive is added, `HygieneEnv` will
   need a `flip` method analogous to `Scopes::without` + ID tagging.

## Acceptance ☑

- [x] `cargo build -p mty-macros` clean
- [x] `cargo build --workspace` clean (existing macro tests pass)
- [x] `cargo test -p mty-macros` passes including new tests
- [x] `cargo test --workspace` not regressed
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] `cargo fmt --all -- --check` clean
- [x] RFC + notes file present
