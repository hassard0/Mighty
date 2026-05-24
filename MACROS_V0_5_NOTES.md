# Macros v0.5 — implementation notes

Working notebook for the v0.5 macro-system slice. Captures interpretation
calls made during the build so future slices can revisit them with context.

## What v0.5 ships

1. **`name!(args)` invocation syntax** (Rust-style trailing `!`). The
   parser recognizes `IDENT BANG L_PAREN` as a `MACRO_CALL` node whose
   args are stored as a single raw `TOKEN_TREE` (no expression parsing
   of the arguments — the macro expander interprets them). v0.4's
   plain-call syntax (`foo(...)` for a known macro `foo`) continues to
   work for backwards-compat.
2. **SD6001 unknown_macro** finally fires. Any `IDENT!(...)` call site
   with no matching `MacroDef` in the registry triggers SD6001.
3. **Extended hygiene mangling**: `let` bindings in tuple, struct, ref,
   and binding-pattern shapes are mangled, not just `let IDENT`. Walks
   the pattern subtree and renames every leaf identifier binding to
   `__mac_<ctx>_<orig>`. Set-of-scopes hygiene is still deferred.
4. **Cross-file `pub macro`**: `MacroDef` carries `is_pub: bool` and
   per-package registries split into `local` and `exported`. When
   `use otherpkg.foo` resolves and `foo` is in `otherpkg`'s exported
   set, the macro is registered in the importing file's local registry.
5. **Procedural macro skeleton**: `proc macro` declarations parse and
   register, but execution is gated behind **SD6006**
   `proc_macro_unsupported_v0_5`. The proc-macro interpreter needs a
   sandboxed SIR sub-context that doesn't exist yet; v0.6 closure.
6. **Standard macros library**: `assert!`, `assert_eq!`, `assert_ne!`,
   `debug!`, `unreachable!` shipped as source fixtures under
   `crates/sdust-macros/lib/`. Loadable by `use sdust_macros.assert`
   etc.

## Interpretation calls

### IC1: `name!(args)` args parsed as opaque tokens

The spec leaves the arg syntax intentionally open. We parse the
argument run as a TOKEN_TREE (a paren-balanced opaque slice). The
macro expander splits on commas at depth 0 to recover individual
arg source slices, preserving v0.4's expansion contract. This gives
maximum flexibility for future variadic macros (`format!(...)`,
`vec![1,2,3]`) without committing to expression-shape arguments.

### IC2: SD6005 vs SD6006

- **SD6005** `proc_macro_impure` is reserved for "your proc-macro
  body contains an effect call". v0.5 detects this statically
  (token-tree scan for `effect.*` patterns) and emits SD6005 at
  declaration time.
- **SD6006** `proc_macro_unsupported_v0_5` fires at *call* sites for
  parsed-but-unexecutable proc macros. This is a soft gate so test
  code can verify parsing + storage now and unblock proc-macro
  rollout in v0.6 by replacing the call site without source churn.

### IC3: Cross-file via fixture-based test, not full package resolution

`PackageMacros::register_use(otherpkg_exported, alias_map)` is the
public API. The HIR lowering loop wires it up when `use otherpkg.foo`
is encountered. For v0.5 the cross-file test uses an in-memory
two-file fixture: package resolution beyond the current file is a
sdust-pkg concern and shouldn't bloat this slice. Real cross-file
flow lights up automatically once sdust-pkg pipes its symbol table
into HIR lowering (work owned by another agent's slice).

### IC4: Extended hygiene walks patterns lexically, not via the parser

The macro body is a flat token stream — we don't have a proper
parsed AST for it. So pattern recognition uses a small lexical
walker: after `let` (possibly `mut`), recognize `(` (tuple),
`{` (struct or block — disambiguated by lookahead), `&` (ref),
or `IDENT` (simple), and harvest every IDENT inside the pattern
extent. False positives (idents inside a struct-type annotation,
say) are tolerated because mangling a non-binding ident has no
behavioral effect at the use site if the same ident isn't also a
binding — the macro body never resolves names against the caller
scope, only its own.

### IC5: Stdlib macros ship as source fixtures, not Rust strings

`crates/sdust-macros/lib/*.sd` are real Stardust source files. The
test harness `include_str!`s them and feeds them through the same
registry as user code. Keeps the macros readable + lets users see
exactly what `assert!` expands to. The standard library "wiring"
(automatic registration when a project imports `use std.macros`)
is left for sdust-pkg integration — v0.5 only ships the source.

### IC6: Proc-macro grammar uses `proc macro` not `#[proc_macro]`

Stardust attributes (v0.5) don't support functional positions
yet (`#[proc_macro]` would need a real attribute system). We
introduce `proc macro` as a two-keyword item form. PROC_MACRO_DECL
is a new SyntaxKind. Body is a single `fn`-shape expression that
takes one `TokenStream` and returns one `TokenStream`.

### IC7: Backwards-compat: `foo(args)` for declared `foo` still expands

v0.4 expansion behavior is preserved for any macro registered in
the local registry. SD6001 only triggers for the **explicit
`name!(...)` shape** when `name` is unresolved. This avoids a
breaking change to every example and selfhost source already in
the tree.

### IC8: `mac!name(...)` was the original proposal — we shipped `name!(...)`

The slice scope mentions `mac!name(...)` as one of two options. We
picked `name!(...)` (Rust-style) because:

- The lexer already produces BANG; no new lexer work.
- Less verbose at call sites — matters because macros are common.
- Familiar to Rust devs; Stardust's audience overlaps heavily.

## Open follow-ups for v0.6

- Proc-macro execution: needs a sandboxed SIR sub-interpreter with
  CPU/memory/wall caps. Owner: sdust-sir + sdust-runtime.
- Set-of-scopes hygiene: replace the lexical mangler. Owner:
  sdust-macros + sdust-hir resolver.
- Real package-aware macro import: sdust-pkg pipes exported symbol
  table into HIR lowering's `PackageMacros::register_use`.
- Variadic macros: `format!("{} {}", a, b)`. Token-tree grammar
  needs `$(...)*` repetition syntax similar to Rust macro_rules.
- `#[proc_macro]` attribute form (once attributes support functional
  application).
