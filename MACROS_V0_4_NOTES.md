# Macros v0.4 — implementation notes

Working notebook for the v0.4 declarative-macro slice. Captures
interpretation calls made during the build so future slices can
revisit them with context.

## Crate layout

* `crates/sdust-macros/` — registry + expander + diag-code constants.
  Pure library, no I/O, no other crate's runtime types.
* `crates/sdust-hir/src/lower/macros.rs` — integration: walks the
  source, calls `sdust_macros::preprocess`, plugs the rewritten
  source back into the parser before the normal lowering walk.
* `crates/sdust-hir/src/lower/mod.rs` — registers the macros module
  *and* invokes `preprocess` from `LoweringCtx::lower_file`. This is
  technically more than the "one-line registration" the slice scope
  describes, but the integration genuinely has nowhere else to go
  (the driver layer is outside scope, and parsing already happened
  before lowering starts). The diff is ~12 lines and additive.

## Interpretation calls

1. **SD6xxx codes live in `sdust_macros::diag` as bare `u16`** rather
   than in `sdust_diagnostics::codes`. Per the slice scope we cannot
   modify `sdust-diagnostics`. The HIR integration wraps the constants
   in `DiagCode::new(N)` at the point of emission. A future cleanup
   slice may absorb them into the central catalog.

2. **Hygiene via mangling, not set-of-scopes.** Set-of-scopes is the
   "right" answer for declarative macros, but it's a substantial
   compiler change (the resolver needs a notion of "scope set"
   attached to every identifier reference). For v0.4's small surface
   area — assertions, simple guards, sugar — expansion-time mangling
   of `let IDENT` bindings is sound and small. The upgrade path is
   documented in both the internals doc and the spec.

3. **No `mac!name(...)` syntactic marker.** This was tempting because
   it would unlock MT6001 ("unknown_macro"), but adding it requires
   parser changes that ripple into syntax, ast, items, and recovery —
   all outside scope. Keep MT6001 reserved; ship it with the marker in
   v0.5.

4. **Hygiene only catches `let IDENT`.** Macros that introduce
   pattern-binding `let`s (`let (a, b) = ...`) do not get those names
   mangled in v0.4. This is documented as a known limitation; it does
   not break any existing example or test. v0.5's set-of-scopes
   rewrite handles all binding shapes uniformly.

5. **Failed expansions get replaced with a sentinel `0`.** When the
   expander returns an error (arity mismatch, bad arg tokens, depth
   blow-up), we still need to remove the call from source so the
   preprocessing loop terminates *and* so downstream lowering can
   continue and find more errors. Replacing the call's byte range with
   the literal `0` keeps the surrounding parse well-formed while
   surfacing the SD6xxx diagnostic on the original span.

6. **Macro call detection rule.** A `CALL_EXPR` whose callee is a
   single-segment `PATH_EXPR` matching a registered macro name is a
   macro call. Calls inside the body of a `MACRO_DECL` are NOT
   detected (the body is a template, expanded once at the actual call
   site, not at definition time). Multi-segment paths (`foo.bar()`,
   `Type::method()`) are never macro calls in v0.4.

7. **Substituted arguments are always paren-wrapped.** This preserves
   operator precedence in the general case. The cost is uglier source
   text (`(1 + 1)` instead of `1 + 1`) but the HIR is identical, so
   nothing downstream notices.

8. **The preprocessor loop iterates a max of `MAX_EXPANSION_DEPTH = 32`
   times.** Each iteration expands one *wave* of calls; the next
   iteration sees what the previous wave produced. Direct recursion
   (`r => r() + 1`) and mutual recursion both terminate via the cap.

## Open follow-ups for v0.5

* `mac!name(...)` syntax + MT6001 activation.
* Set-of-scopes hygiene (replacing the mangling pass in `expand`).
* Proc macros (sandboxed token-tree → token-tree functions).
* Cross-file macro export + visibility (`pub macro foo`).
* `format!`-style variadic macro arguments.
