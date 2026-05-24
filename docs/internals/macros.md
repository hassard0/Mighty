# Declarative + procedural macros (v0.5)

This document describes the *implementation* of Stardust's macro system
as it ships in v0.5. The user-facing spec lives in
[`docs/spec/macros-v0.5.md`](../spec/macros-v0.5.md); this page is for
contributors hacking on `sdust-macros` or the HIR-lowering integration
in `sdust-hir`.

## Pipeline

```
   source.sd
      │
      ▼
   parse  ──▶ CST
      │
      ▼
   ┌─────────────────────────────────────────┐
   │  sdust-hir::lower::macros::preprocess   │ ◀── sdust-macros
   │   – collect MacroRegistry from File     │
   │   – check_proc_macros (SD6005)          │
   │   – find every MACRO_CALL OR known      │
   │     plain CALL_EXPR (v0.4 compat)       │
   │   – find every unknown MACRO_CALL        │
   │     (SD6001)                            │
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
   sdust-hir::lower::LoweringCtx::lower_file
      │
      ▼
   HIR Package
```

The macro layer is still a pure source-to-source pre-pass. Downstream
stages (name resolution, type check, borrow check, SIR, codegen)
never see a macro call: they see the expansion as if it had been
written by hand. Procedural-macro call sites *parse* in v0.5 but emit
SD6006 because the sandboxed interpreter that will run them is a v0.6
deliverable.

## Crates

* **`sdust-macros`** — registry + expander, diagnostic-code constants,
  procedural-macro skeleton, bundled standard macros.
* **`sdust-syntax`** — parses the new `MACRO_CALL` / `TOKEN_TREE` /
  `PROC_MACRO_DECL` node kinds.
* **`sdust-hir`** — owns `lower/macros.rs`, which calls
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
selfhost. Only the explicit `name!(...)` shape triggers SD6001 when
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
the exporter's `PackageMacros` from sdust-pkg's symbol table — is a
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

### Purity check (SD6005)

`check_proc_macro_purity` scans the body for:

* `effect.<name>(...)` chains.
* Bare calls to the well-known impure surface: `time`, `env`, `io`,
  `model`, `rand`.

These trigger SD6005 at *declaration time*. The check is purely
syntactic; v0.6's sandbox is the authoritative gate.

### Execution gate (SD6006)

Any call site to a procedural macro emits SD6006 in v0.5 and replaces
the call with the sentinel literal `0`. The macro declaration is
preserved verbatim, so call-site source survives untouched when v0.6
ships actual execution.

### Planned v0.6 sandbox

`crates/sdust-macros/src/proc.rs` exposes the future constants:

```rust
pub const PROC_MACRO_WALL_MS:  u64   = 100;
pub const PROC_MACRO_MEM_BYTES: usize = 16 * 1024 * 1024;
pub const PROC_MACRO_STEPS:    u64   = 100_000;
```

These are the limits the v0.6 sub-interpreter will enforce.

## Standard macro library

Bundled with `sdust-macros` under `lib/`:

| File              | Macros                                  |
|-------------------|------------------------------------------|
| `assert.sd`       | `assert!`, `assert_eq!`, `assert_ne!`    |
| `debug.sd`        | `debug!`                                 |
| `unreachable.sd`  | `unreachable!()`                         |

All five ship as `pub macro`. Projects load them into their
`PackageMacros` via `sdust_macros::stdlib::load_into(&mut pm)`.

Auto-import via `use sdust_macros.assert` lights up once sdust-pkg
pipes its package symbol table into HIR lowering; v0.5 exposes the
sources so projects can opt-in immediately.

## Recursion

The expander itself is non-recursive. The *preprocessing loop* in
`sdust-hir` iterates: each pass expands one wave of macro calls; the
result is re-parsed and the loop runs again until no macro calls
remain or `MAX_EXPANSION_DEPTH = 32` is reached. Hitting the cap
yields SD6004 for every remaining call site.

This caps both direct (`macro r(x) => { r(x) + 1 }`) and transitive
(`A` calls `B` calls `A`) recursion.

## Error catalog

| Code   | Meaning                                                  |
|--------|----------------------------------------------------------|
| SD6001 | `unknown_macro` — `name!(args)` with no matching decl.   |
| SD6002 | `macro_arity_mismatch` — call has wrong number of args.  |
| SD6003 | `macro_body_parse_failed` — expansion doesn't re-parse.  |
| SD6004 | `recursive_macro_too_deep` — depth cap (32) exceeded.    |
| SD6005 | `proc_macro_impure` — proc body references an effect.    |
| SD6006 | `proc_macro_unsupported_v0_5` — exec deferred to v0.6.   |

Codes live in `sdust-macros::diag` as bare `u16` constants; the HIR
integration wraps them in `DiagCode::new(N)` so we don't have to
modify `sdust-diagnostics` for each macro feature. A future cleanup
slice may merge them into the central catalog.

## v0.6 follow-on

* **Procedural-macro execution** — sandboxed SIR sub-interpreter with
  100 ms wall, 16 MB memory, 100 k step caps. Owner: sdust-sir +
  sdust-runtime.
* **Set-of-scopes hygiene** (Racket-style) replacing the lexical
  mangler. Lets macros introduce nested `fn` items, reference
  caller-scope identifiers explicitly, and disambiguate across deeply
  nested expansions without naming collisions.
* **Real package-aware macro import** — sdust-pkg pipes its exported
  symbol table into HIR lowering's `PackageMacros::register_use`.
* **Variadic macros** — `format!("{} {}", a, b)`. Token-tree grammar
  needs a `$(...)*` repetition syntax similar to Rust macro_rules.
* **`#[proc_macro]` attribute form** — once attributes support
  functional application.
