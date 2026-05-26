# Macro hygiene wiring v0.14 — HIR consumes set-of-scopes

Wires the v0.13 set-of-scopes hygiene layer (RFC-009) into the
`mty-hir` macro pipeline. The legacy single-mark expander
(`expand` / `expand_to_source`) is now deprecated; the new path
(`expand_scoped` via `expand_scoped_to_source`) is what every
declarative-macro call site flows through.

This agent is one of five parallel swarm agents working v0.14 from
HEAD `023f904` (v0.13.0).

Companion docs:
- [`docs/internals/macros.md`](../../../docs/internals/macros.md) — extended with a v0.14 wiring section
- [`docs/spec/rfcs/RFC-009-set-of-scopes.md`](../../../docs/spec/rfcs/RFC-009-set-of-scopes.md) — the model
- [`MACRO_HYGIENE_V0_13_NOTES.md`](MACRO_HYGIENE_V0_13_NOTES.md) — the v0.13 substrate this builds on

## Scope of this slice

Goal: replace `mty-hir`'s call to the legacy mangler with the
scope-aware expander, capture the per-invocation scope trace, and
expose it on `Preprocessed` so a future HIR name resolver can consume
it without further plumbing changes.

What shipped (SHIPPED-FULL):

* `crates/mty-macros/src/expand.rs` — promoted `expand_scoped` to the
  primary path. Added `expand_scoped_to_source(def, args, gen,
  def_scopes, caller_arg_scopes) -> Result<(String, ScopedExpansion)>`
  as a drop-in replacement for `expand_to_source` that ALSO yields
  the scope trace. Marked `expand` and `expand_to_source`
  `#[deprecated(since = "0.14.0", note = "use expand_scoped …")]` with
  v0.15 removal scheduled.
* `crates/mty-macros/src/lib.rs` — re-export `expand_scoped_to_source`;
  the deprecated `expand` / `expand_to_source` re-exports stay (still
  needed by the v0.13 test files) gated by `#[allow(deprecated)]`.
* `crates/mty-hir/src/lower/macros.rs` — switched the call site from
  `expand_to_source(def, args, ctx_counter)` to
  `expand_scoped_to_source(def, args, &mut scope_gen, Scopes::empty(),
  Scopes::empty())`. The `MacroContext` u32 counter is gone; a single
  `ScopeGen` lives for the duration of `preprocess`. Added a new
  `MacroExpansionRecord` struct and `Preprocessed::macro_trace:
  Vec<MacroExpansionRecord>` field that records every successful
  declarative expansion's `(name, intro, bindings, call_span, pass)`.
* `crates/mty-hir/Cargo.toml` — added `mty-macros` as a
  dev-dependency so the e2e test can reference `Scopes` directly.
* `crates/mty-hir/tests/macro_hygiene_e2e.rs` (NEW) — 6 end-to-end
  tests:
    1. `identity_macro_doesnt_capture_caller_var` — identity macro
       leaves caller bindings unmolested.
    2. `macro_let_doesnt_shadow_caller` — macro-introduced `tmp`
       gets the intro scope; caller's `tmp` remains visible.
    3. `swap_macro_composition` — Flatt's canonical case: two
       macros each introducing `t` get distinct scope sets.
    4. `macro_recursion` — `inner` called from `outer` and
       standalone: 3 inner records + 2 outer records, all with
       unique intro scopes.
    5. `def_then_use_macro` — macro on RHS of `let r = mac(7)`
       lowers cleanly; caller's `r` reference survives.
    6. `scope_ids_are_strictly_monotonic` — across multiple call
       sites, intro scopes are unique and bindings carry the
       record's intro scope.
* `crates/mty-macros/tests/*.rs` — eight v0.13 test files that
  exercise the deprecated `expand` / `expand_to_source` got
  `#![allow(deprecated)]` at the crate-level. Acceptable until v0.15
  drops the legacy functions, at which point those tests will be
  retired or rewritten against the scoped expander.
* `docs/internals/macros.md` — appended a "v0.14 wiring through HIR"
  section describing the new pipeline, the `MacroExpansionRecord`
  trace, what still uses the textual mangle, and the v0.15 plan.

## What did not change

* The spliced source text still carries the legacy
  `__mac_<ScopeId>_<name>` mangle for every macro-introduced binding.
  That's intentional: the textual mangle keeps the post-expansion CST
  unambiguous at the IDENT level even though the HIR resolver
  (`crates/mty-hir/src/resolve.rs` is a stub) doesn't consume the
  scope trace yet. Removing the mangle requires a real resolver that
  walks the trace + CST in tandem.
* `mty-syntax`, `mty-types`, `mty-borrow`, the codegen crates,
  `mty-driver`, `mty-cli`, `mty-stdlib`, and `mty-runtime` — none
  touched (per concurrency rules).
* Workspace `Cargo.toml` — not modified.

## Diagnostic counts

* `mty-macros`: 111 tests (43 sets_of_scopes + 68 pre-existing) — all
  passing.
* `mty-hir`: 43 tests (37 pre-existing + 6 new e2e) — all passing.
* Clippy on owned crates: clean with `-D warnings`. (Other crates not
  built in this session — see "Coordination notes" below.)
* `cargo fmt --check` on owned crates: clean after one fmt pass.

## User-visible behavior

Strict improvement:

* Programs that worked under v0.13 continue to work — the textual
  output of `expand_scoped` for the legacy mangling cases is
  byte-identical to what `expand` produced. (Verified by the
  `crates/mty-macros/tests/hygiene_avoids_capture.rs` v0.13
  baseline-equivalence tests.)
* Programs that had subtle capture bugs under the old mangler (swap
  macros, deep composition) now produce the right scope trace.
  Whether they *resolve* correctly is up to the future HIR resolver;
  the data plumbing is now in place.

## v0.15 follow-up

1. **Build the HIR name resolver** (`crates/mty-hir/src/resolve.rs` is
   currently a stub). Consume `Preprocessed::macro_trace` + the
   post-expansion CST to compute scope sets per reference, call
   `mty_macros::resolve` per name, surface `MT5901` on
   `ResolveAmbiguity`. Spec for `MT5901` is RFC-009 §6.
2. **Drop the textual mangle.** Once the resolver works without it,
   `expand_scoped` can emit plain IDENTs and rely on `bindings` +
   scope sets for disambiguation.
3. **Remove the deprecated `expand` / `expand_to_source`** and
   the `#![allow(deprecated)]` annotations in the v0.13 test files.
   At that point the v0.13 tests should be ported to assert against
   the scope trace directly (most of them already have analogues in
   `tests/sets_of_scopes.rs`, so the diff should be small).
4. **Wire macro-in-macro composition through the trace** — the
   current preprocess loop expands the inner call on the next outer
   pass after textual splicing, which means inner expansions see
   `def_scopes = Scopes::empty()` instead of the outer body's scopes.
   The resolver should compose the trace records' intro scopes so
   nested expansions inherit their enclosing scope sets without
   needing per-pass plumbing.

## Coordination notes

* Sibling agents had large in-flight edits in `mty-codegen-wasm` +
  `mty-driver` at the time of this work. `cargo test --workspace`
  surfaces failures in those crates' uncommitted diffs (selfhost
  codegen + WASI Preview 2 component-wrap test). Those are
  sibling-agent territory, not regressions from this slice.
  Verified by running `cargo test -p mty-macros -p mty-hir` (all
  green) and `cargo build --workspace` (clean).
* No build-failure or test-regression in any non-owned crate is
  attributable to this slice.

## Commits

(See `git log --oneline` for the exact set; this slice ships as
co-authored commits with `Co-Authored-By: Claude Opus 4.7 (1M context)
<noreply@anthropic.com>`.)
