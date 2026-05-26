# expand() / expand_to_source() removal v0.15

Closes the v0.14 deprecation window on `mty-macros`. The legacy
single-mark expander (`pub fn expand`, `pub fn expand_to_source`) is
**deleted**, along with the now-unused `MacroContext` type alias and
every `#[allow(deprecated)]` shim that existed only to keep the v0.13
test bodies compiling while the deprecation window was open.

`expand_scoped_to_source` is the **sole** macro-expansion entry point
in mty-macros as of v0.15.

This agent is one of five parallel swarm agents working v0.15 from
HEAD `055b7a0` (v0.14.0).

Companion docs:
- [`MACRO_HYGIENE_WIRING_V0_14_NOTES.md`](MACRO_HYGIENE_WIRING_V0_14_NOTES.md) — the wiring this follows up on
- [`MACRO_HYGIENE_V0_13_NOTES.md`](MACRO_HYGIENE_V0_13_NOTES.md) — the set-of-scopes substrate
- [`docs/spec/rfcs/RFC-009-set-of-scopes.md`](../../../docs/spec/rfcs/RFC-009-set-of-scopes.md) — the model

## Scope of this slice

Drop the pre-v0.14 mangling-only API. There was exactly one external
consumer (mty-hir) and it migrated in the v0.14 wiring slice — the
deprecation window only had to carry the legacy mty-macros tests
through one release. v0.15 collects on that.

What shipped (SHIPPED-FULL):

* `crates/mty-macros/src/expand.rs` — deleted `pub fn expand` (the
  legacy mangling-only `Vec<Tok>` expander), deleted
  `pub fn expand_to_source` (the `tokens_to_source` shim around it),
  and deleted the `MacroContext = u32` type alias that existed only to
  name the `ctx` parameter those fns took. The module-level doc and
  the `expand_scoped` / `ScopedExpansion` doc comments lost their
  `[`expand`]` cross-references and were updated to describe the
  scope-set path on its own terms (no more "differences from
  `expand`"). The `#[cfg(test)] mod tests` block had its
  `#[allow(deprecated)]` removed; redundant cases (every test whose
  body asserted exactly what a tests/ integration test asserts) were
  deleted, and the three unique cases that remain
  (`arity_mismatch_reported`, `free_name_not_mangled`,
  `parameter_inside_tuple_pattern_is_not_mangled`) were rewritten to
  use `expand_scoped` directly.
* `crates/mty-macros/src/lib.rs` — dropped
  `pub use expand::{expand, expand_to_source};` and the
  `#[allow(deprecated)]` that gated it, and dropped `MacroContext`
  from the `expand_scoped` re-export line.
* All nine `tests/*.rs` integration files that had the
  `#![allow(deprecated)] // exercises legacy expand_to_source` shim
  were migrated to `expand_scoped_to_source` (with a per-file
  `expand_src(def, args)` helper that supplies a fresh `ScopeGen` and
  empty scope sets). Pattern-hygiene tests that asserted on a specific
  mangle string (`__mac_42_a`, etc.) now read the `intro` ScopeId off
  the returned `ScopedExpansion` and format the expected name from
  that — the assertions still pin the same `__mac_<intro>_<orig>`
  shape, just without hard-coding what `<intro>` is.

## Caller inventory (Phase 1)

`grep -rn "fn expand\b\|fn expand_to_source\b\|allow(deprecated)" crates/`
at the start of the slice:

| Caller                                                       | Decision                                              |
|--------------------------------------------------------------|-------------------------------------------------------|
| `crates/mty-macros/src/lib.rs:33` re-export                  | deleted                                               |
| `crates/mty-macros/src/expand.rs:80` `pub fn expand`         | deleted                                               |
| `crates/mty-macros/src/expand.rs:138` `pub fn expand_to_source` | deleted                                            |
| `crates/mty-macros/src/expand.rs:143` inline `#[allow(deprecated)]` | deleted with the body                          |
| `crates/mty-macros/src/expand.rs:527` `#[cfg(test)]` `mod tests` allow + 12 cases | 9 redundant tests deleted, 3 migrated, allow removed |
| `crates/mty-macros/tests/assert_eq_real.rs`                  | migrated                                              |
| `crates/mty-macros/tests/cross_file_macro.rs`                | migrated                                              |
| `crates/mty-macros/tests/hygiene_avoids_capture.rs`          | migrated; deleted obsolete `scoped_expansion_emits_same_source_as_legacy` parity test |
| `crates/mty-macros/tests/mac_marker.rs`                      | migrated                                              |
| `crates/mty-macros/tests/param_substitution.rs`              | migrated                                              |
| `crates/mty-macros/tests/recursive_capped.rs`                | migrated                                              |
| `crates/mty-macros/tests/simple_expansion.rs`                | migrated                                              |
| `crates/mty-macros/tests/stdlib_macros.rs`                   | migrated                                              |
| `crates/mty-macros/tests/tuple_pattern_hygiene.rs`           | migrated                                              |

mty-hir had no `#[allow(deprecated)]` for this path — the v0.14 wiring
slice had already moved it to `expand_scoped_to_source`. No callers
exist in any other crate (`mty-syntax`, `mty-types`, `mty-borrow`,
`mty-driver`, `mty-cli`, `mty-runtime`, codegen, `mty-stdlib`).

## Test count delta

mty-macros: **111 → 101** (−10).

Breakdown:
* `src/expand.rs` inline `mod tests`: 12 → 3 (−9). The baseline
  integration tests already covered most pattern shapes, so the
  unique cases (kept) are `arity_mismatch_reported`,
  `free_name_not_mangled`, and
  `parameter_inside_tuple_pattern_is_not_mangled`. The deleted nine
  (`parameter_substitution_wraps_in_parens`, `let_binding_is_mangled`,
  `distinct_contexts_yield_distinct_mangles`,
  `tuple_pattern_bindings_are_mangled`,
  `struct_pattern_bindings_are_mangled_shorthand`,
  `struct_pattern_bindings_are_mangled_renamed`,
  `ref_pattern_binding_is_mangled`,
  `ref_mut_pattern_binding_is_mangled`,
  `mut_binding_is_mangled`) were verbatim duplicates of integration
  cases that now run against the scoped path. The mty-macros unit
  test binary (`src/lib.rs` deps) went 43 → 34 = −9 accordingly.
* `tests/hygiene_avoids_capture.rs`: 5 → 4 (−1) — deleted the
  `scoped_expansion_emits_same_source_as_legacy` parity test. With no
  legacy implementation left there's nothing to compare against; the
  remaining assertions exercise the scope-aware path directly.

Net behavioural coverage is unchanged — every pattern shape, every
arg-substitution rule, every hygiene case still has at least one test.

mty-hir: **43 → 43** (no change).

## Verification

Per the slice's acceptance gates:

* `cargo build -p mty-macros` — clean.
* `cargo build -p mty-hir` — clean.
* `cargo test -p mty-macros` — 101 / 101 pass.
* `cargo test -p mty-hir` — 43 / 43 pass.
* `cargo clippy -p mty-macros --all-targets -- -D warnings` — clean.
  No deprecation warnings (because there's nothing deprecated left).
* `cargo clippy -p mty-hir --all-targets -- -D warnings` — clean.
* `cargo fmt -p mty-macros` — applied.

`cargo build --workspace` is currently red on `mty-codegen-wasm` due
to a parallel swarm agent's in-flight work in that crate (a
`P2DirectImport: Hash` derive). That regression is owned by another
slice and is not in this slice's scope (codegen crates are explicitly
listed as off-limits in this agent's brief). The mty-macros and
mty-hir gates that this slice owns are all green.

## Confirmation

`expand_scoped_to_source` is the **sole** macro-expansion entry point
in mty-macros as of v0.15. The pre-v0.14 `MacroContext` counter and
the single-mark mangler are gone for good — set-of-scopes hygiene
(RFC-009) is the only path through the expander.
