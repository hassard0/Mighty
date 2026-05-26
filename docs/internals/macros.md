# Declarative + procedural macros (v0.5)

This document describes the *implementation* of Mighty's macro system
as it ships in v0.5. The user-facing spec lives in
[`docs/spec/macros-v0.5.md`](../spec/macros-v0.5.md); this page is for
contributors hacking on `mty-macros` or the HIR-lowering integration
in `mty-hir`.

## Pipeline

```
   source.sd
      │
      ▼
   parse  ──▶ CST
      │
      ▼
   ┌─────────────────────────────────────────┐
   │  mty-hir::lower::macros::preprocess   │ ◀── mty-macros
   │   – collect MacroRegistry from File     │
   │   – check_proc_macros (MT6005)          │
   │   – find every MACRO_CALL OR known      │
   │     plain CALL_EXPR (v0.4 compat)       │
   │   – find every unknown MACRO_CALL        │
   │     (MT6001)                            │
   │   – substitute params + mangle hygiene  │
   │   – splice expansion (or sentinel for   │
   │     errors / proc macros / unknown)     │
   │   – re-parse + iterate to fixed point   │
   └─────────────────────────────────────────┘
      │
      ▼
   parse  ──▶ CST  (with macro calls inlined)
      │
      ▼
   mty-hir::lower::LoweringCtx::lower_file
      │
      ▼
   HIR Package
```

The macro layer is still a pure source-to-source pre-pass. Downstream
stages (name resolution, type check, borrow check, MtyIR, codegen)
never see a macro call: they see the expansion as if it had been
written by hand. Procedural-macro call sites *parse* in v0.5 but emit
MT6006 because the sandboxed interpreter that will run them is a v0.6
deliverable.

## Crates

* **`mty-macros`** — registry + expander, diagnostic-code constants,
  procedural-macro skeleton, bundled standard macros.
* **`mty-syntax`** — parses the new `MACRO_CALL` / `TOKEN_TREE` /
  `PROC_MACRO_DECL` node kinds.
* **`mty-hir`** — owns `lower/macros.rs`, which calls
  `sdust_macros::preprocess` from `LoweringCtx::lower_file`.

## Data model

```rust
enum MacroKind { Declarative, Procedural }

struct MacroDef {
    name: String,
    params: Vec<String>,
    body: Vec<Tok>,
    is_pub: bool,            // v0.5
    kind: MacroKind,         // v0.5
}

struct MacroRegistry { macros: HashMap<String, MacroDef> }

struct PackageMacros {                         // v0.5
    local: MacroRegistry,    // what the file's expander sees
    exported: MacroRegistry, // re-exportable via `use otherpkg.x`
}
```

A `Tok` is `(SyntaxKind, String)` — the lexer tag plus the source slice.
Trivia (whitespace, comments) is preserved so the spliced expansion is
human-readable in dumps.

## Call-site syntax: `name!(args)`

v0.5 adds an explicit invocation marker. The parser recognizes an
`IDENT` (or `PATH_EXPR` of a single segment) followed immediately by
`!` then `(` as a `MACRO_CALL` whose arguments are stored as a single
opaque `TOKEN_TREE`:

```
MACRO_CALL
├── PATH_EXPR ("foo")
└── TOKEN_TREE ("(a, b, c)")
```

The macro expander splits the token tree on commas at depth 0 to
recover individual argument source slices. Nested parens, brackets,
and braces bump the depth and are preserved verbatim inside an arg
slice.

The v0.4 plain-call form (`foo(args)` for a registered `foo`) still
expands, for backwards-compat with the existing examples and
selfhost. Only the explicit `name!(...)` shape triggers MT6001 when
the name isn't in the registry; a plain unresolved call is handled by
normal name resolution.

## Expansion algorithm

```text
expand(def, args, ctx):
    if args.len() != def.params.len():
        Err(ArityMismatch)

    arg_toks = args.map(lex_fragment)         # bail out on lex errors
    bound  = let-bindings in def.body that aren't params
             (collected with v0.5's pattern walker)

    out = []
    for tok in def.body:
        if tok is IDENT named after a parameter:
            out += [ "(" ] + arg_toks[i] + [ ")" ]
        elif tok is IDENT in `bound`:
            out += IDENT(__mac_<ctx>_<tok.text>)
        else:
            out += tok
    return out
```

The wrap-in-parens around substituted arguments is what keeps operator
precedence honest. Without it, `double(1 + 2) => 1 + 2 + 1 + 2 = 6` is
fine arithmetically, but `negate(1 + 2) => -1 + 2 = 1` is wrong; the
wrapped form `(-(1 + 2)) = -3` is right.

## Hygiene: extended mangling (v0.5)

v0.5 still uses expansion-time mangling (no set-of-scopes), but the
pattern walker now covers:

| Pattern shape                      | Mangled? |
|------------------------------------|----------|
| `let IDENT = ...`                  | yes (v0.4) |
| `let mut IDENT = ...`              | yes |
| `let (a, b, ...) = ...`            | yes (tuple) |
| `let User { id, name } = ...`      | yes (shorthand fields) |
| `let User { id: x } = ...`         | yes — binds `x`, not `id` |
| `let &x = ...` / `let &mut x = ...`| yes (ref) |
| `let ref x = ...`                  | yes |

The walker is lexical: after `let` (optionally followed by `mut`), it
scans forward to the first `=` at depth 0, treating the prefix as the
pattern extent. Inside that extent it tracks bracket nesting so a
struct pattern's `{ ... }` is distinguished from a tuple pattern's
`( ... )`. Type annotations after `:` outside a struct pattern are
skipped.

Three rules avoid false positives:

1. IDENT followed by `::` or `.` is a path segment, not a binding.
2. IDENT followed by `{` is a struct-pattern type name, not a binding.
3. IDENT followed by `(` is an enum-pattern variant constructor, not
   a binding. The bindings are inside the parens.

Parameters are still substituted (never mangled); free names (calls
to `panic`, references to type names) are still left untouched and
get resolved against the caller's scope.

### Worked example: `pair(p)`

```text
macro pair(p) => { let (a, b) = p; a + b }
```

Call site: `pair(thing)`, fresh `ctx = 42`.

Expansion:
```text
let (__mac_42_a, __mac_42_b) = (thing); __mac_42_a + __mac_42_b
```

Even if the caller has its own `a` or `b` in scope, the mangled
identifiers cannot collide.

## Cross-file `pub macro`

`MacroDef::is_pub` tracks whether the source carried `pub macro …`.
`PackageMacros::from_file` splits a file's macros into `local` (every
decl) and `exported` (the public ones only).

When the importer's `use otherpkg.foo` resolves, the HIR lowering
layer calls `PackageMacros::register_use(other, alias_map)` to pull
every exported macro into the importer's `local` set. An alias map
maps `(exporter_name, bound_as)` so `use otherpkg.foo as bar` works.

v0.5 wires the end-to-end flow through a two-file in-memory fixture
test (`cross_file_macro.rs`). Real package-aware resolution — pulling
the exporter's `PackageMacros` from mty-pkg's symbol table — is a
follow-on slice owned by another agent. The v0.5 surface area is
ready to receive it.

## Procedural macros — v0.5 parse-and-store

```
proc macro Name(input: TokenStream) -> TokenStream { body }
```

`PROC_MACRO_DECL` is a new top-level item kind. The parser recognizes
the two-token `proc macro` prefix because `proc` is an `IDENT`
(keeping the keyword set frozen). Body is brace-balanced opaque
tokens, mirroring declarative macros.

Stored as `MacroDef { kind: Procedural, body: Vec<Tok> }`.

### Purity check (MT6005)

`check_proc_macro_purity` scans the body for:

* `effect.<name>(...)` chains.
* Bare calls to the well-known impure surface: `time`, `env`, `io`,
  `model`, `rand`.

These trigger MT6005 at *declaration time*. The check is purely
syntactic; v0.6's sandbox is the authoritative gate.

### Execution gate (MT6006)

Any call site to a procedural macro emits MT6006 in v0.5 and replaces
the call with the sentinel literal `0`. The macro declaration is
preserved verbatim, so call-site source survives untouched when v0.6
ships actual execution.

### Planned v0.6 sandbox

`crates/mty-macros/src/proc.rs` exposes the future constants:

```rust
pub const PROC_MACRO_WALL_MS:  u64   = 100;
pub const PROC_MACRO_MEM_BYTES: usize = 16 * 1024 * 1024;
pub const PROC_MACRO_STEPS:    u64   = 100_000;
```

These are the limits the v0.6 sub-interpreter will enforce.

## Standard macro library

Bundled with `mty-macros` under `lib/`:

| File              | Macros                                  |
|-------------------|------------------------------------------|
| `assert.sd`       | `assert!`, `assert_eq!`, `assert_ne!`    |
| `debug.sd`        | `debug!`                                 |
| `unreachable.sd`  | `unreachable!()`                         |

All five ship as `pub macro`. Projects load them into their
`PackageMacros` via `sdust_macros::stdlib::load_into(&mut pm)`.

Auto-import via `use sdust_macros.assert` lights up once mty-pkg
pipes its package symbol table into HIR lowering; v0.5 exposes the
sources so projects can opt-in immediately.

## Recursion

The expander itself is non-recursive. The *preprocessing loop* in
`mty-hir` iterates: each pass expands one wave of macro calls; the
result is re-parsed and the loop runs again until no macro calls
remain or `MAX_EXPANSION_DEPTH = 32` is reached. Hitting the cap
yields MT6004 for every remaining call site.

This caps both direct (`macro r(x) => { r(x) + 1 }`) and transitive
(`A` calls `B` calls `A`) recursion.

## Error catalog

| Code   | Meaning                                                  |
|--------|----------------------------------------------------------|
| MT6001 | `unknown_macro` — `name!(args)` with no matching decl.   |
| MT6002 | `macro_arity_mismatch` — call has wrong number of args.  |
| MT6003 | `macro_body_parse_failed` — expansion doesn't re-parse.  |
| MT6004 | `recursive_macro_too_deep` — depth cap (32) exceeded.    |
| MT6005 | `proc_macro_impure` — proc body references an effect.    |
| MT6006 | `proc_macro_unsupported_v0_5` — exec deferred to v0.6.   |

Codes live in `mty-macros::diag` as bare `u16` constants; the HIR
integration wraps them in `DiagCode::new(N)` so we don't have to
modify `mty-diagnostics` for each macro feature. A future cleanup
slice may merge them into the central catalog.

## v0.6 follow-on

* **Procedural-macro execution** — sandboxed MtyIR sub-interpreter with
  100 ms wall, 16 MB memory, 100 k step caps. Owner: mty-sir +
  mty-runtime.
* **Set-of-scopes hygiene** (Racket-style) replacing the lexical
  mangler. Lets macros introduce nested `fn` items, reference
  caller-scope identifiers explicitly, and disambiguate across deeply
  nested expansions without naming collisions.
* **Real package-aware macro import** — mty-pkg pipes its exported
  symbol table into HIR lowering's `PackageMacros::register_use`.
* **Variadic macros** — `format!("{} {}", a, b)`. Token-tree grammar
  needs a `$(...)*` repetition syntax similar to Rust macro_rules.
* **`#[proc_macro]` attribute form** — once attributes support
  functional application.

## v0.13 set-of-scopes hygiene (RFC-009) — infrastructure

v0.13 added the substrate for "Bindings as Sets of Scopes" (Flatt 2016,
POPL) alongside the existing mangler. Three modules in `mty-macros`:

| Module     | Type                | Purpose                                  |
|------------|---------------------|------------------------------------------|
| `scopes`   | `Scopes`, `ScopeId`, `ScopeGen` | Scope-set data type; allocator |
| `scopes`   | `resolve(name, candidates) -> Result<Option<P>, ResolveAmbiguity>` | Pick the binding whose scope set is the maximal subset of the name's scope set |
| `hygiene`  | `ScopedTok`, `HygieneEnv` | Token + scope-set pair; per-invocation env |
| `expand`   | `expand_scoped(...) -> ScopedExpansion` | Scope-aware expansion |

The Flatt rules embedded in `expand_scoped`:

* Every macro invocation mints a *fresh scope* via `ScopeGen::fresh`.
* Body-introduced tokens carry `def_scopes ∪ {fresh}`.
* Argument tokens (substituted parameters) keep the caller's scope set
  unchanged — they were not introduced by *this* macro.
* Binding occurrences in the body are recorded as `(text, scope_set)`
  in `ScopedExpansion::bindings` for later resolution.

The v0.13 layer is *redundant* with the legacy mangler — both ran on
every expansion, but only the mangler influenced the spliced source.
Twelve integration tests in `crates/mty-macros/tests/sets_of_scopes.rs`
cover swap macros, recursive macros, macro composition, ambiguity, and
parameter scope preservation.

## v0.14 wiring through HIR — scoped path is primary

v0.14 promotes `expand_scoped_to_source` (a thin wrapper over
`expand_scoped` that also returns the textual splice) to the primary
path used by `mty-hir::lower::macros::preprocess`. The legacy
`expand` / `expand_to_source` are now `#[deprecated]` and scheduled for
removal in v0.15.

```
   source.mty
      │
      ▼
   parse  ──▶ CST
      │
      ▼
   ┌──────────────────────────────────────────────────┐
   │  mty-hir::lower::macros::preprocess              │
   │   – per-translation-unit ScopeGen                │
   │   – per call site: expand_scoped_to_source(…)    │
   │       └─▶ (source, ScopedExpansion)              │
   │   – record MacroExpansionRecord into trace       │
   │   – splice source, re-parse, iterate             │
   └──────────────────────────────────────────────────┘
      │
      ▼
   Preprocessed { source, diagnostics, macro_trace }
      │
      ▼
   parse  ──▶ CST  (with macro calls inlined)
      │
      ▼
   mty-hir::lower::LoweringCtx::lower_file
```

### `MacroExpansionRecord` and the trace

```rust
pub struct MacroExpansionRecord {
    pub name: String,                       // macro name (single segment)
    pub intro: ScopeId,                     // fresh scope minted by ScopeGen
    pub bindings: Vec<(String, Scopes)>,    // bindings the body introduced
    pub call_start: usize,                  // pre-rewrite byte offset
    pub call_end: usize,
    pub pass: u32,                          // 0-based preprocess pass
}

pub struct Preprocessed {
    pub source: String,
    pub diagnostics: Vec<Diagnostic>,
    pub macro_trace: Vec<MacroExpansionRecord>,   // v0.14
}
```

The trace is produced unconditionally for every successful declarative
expansion. Procedural-macro expansions are not represented (proc
macros' output is treated as user source — no scope set to attach).

### What still uses the textual mangle

The spliced source still carries the legacy `__mac_<ScopeId>_<name>`
mangle for every binding introduced by an expansion. That's
intentional: mty-hir does not yet have a name resolver, and the
mangled identifiers keep names distinct *at the CST level* without
needing one. When the resolver lands, it should:

1. Walk the trace, building a `BindingId` table keyed by
   `(MacroExpansionRecord, name)` per record.
2. On each reference in the post-expansion CST, derive the active
   scope set from which macro expansions textually surround the
   reference (the `call_start`..`call_end` ranges + the wrapping
   record's `intro`).
3. Call `mty_macros::resolve(name_scopes, candidate_bindings)` to pick
   the right binding, mapping its `Err(ResolveAmbiguity)` arm to
   MT5901.

That work is reserved for the future HIR `resolve` module (currently a
stub at `crates/mty-hir/src/resolve.rs`). The set-of-scopes data is
already in place — only the consumer is missing.

### Why the textual round-trip is OK for v0.14

The fact that `expand_scoped` returns a `Vec<ScopedTok>` but we drop
the scope tags during the source splice (via `strip_scopes`) means we
lose scope information at the source-text boundary. That's tolerable
because:

* The mangled binding names guarantee no two same-named bindings ever
  clash in the post-expansion CST — *something* the borrow checker
  and type checker can already see at the IDENT level.
* The trace preserves the full scope set per binding, indexed by
  `intro` scope ID — a future resolver can rebuild a scope-aware
  binding graph from the trace + CST without re-running expansion.

The v0.15 follow-up (see `dev/history/notes/MACRO_HYGIENE_WIRING_V0_14_NOTES.md`)
will remove the legacy `expand` / `expand_to_source` once the resolver
lands and verifies it doesn't need the textual mangle as a fallback.
