# Declarative macros (v0.4)

This document describes the *implementation* of Stardust's declarative
macro system as it ships in v0.4. The user-facing spec lives in
[`docs/spec/macros-v0.4.md`](../spec/macros-v0.4.md); this page is for
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
   │   – find every CALL_EXPR to a macro     │
   │   – substitute params + mangle hygiene  │
   │   – splice expansion into source        │
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

The macro layer is a pure source-to-source pre-pass. Downstream stages
(name resolution, type check, borrow check, SIR, codegen) never see a
macro call: they see the expansion as if it had been written by hand.

## Crates

* **`sdust-macros`** — the registry + expander, plus the `SD6xxx`
  diagnostic-code constants.
* **`sdust-hir`** — owns `lower/macros.rs`, which calls
  `sdust_macros::preprocess` from `LoweringCtx::lower_file`. Only the
  registration line in `lower/mod.rs` and the four-line preprocessing
  hook in `LoweringCtx::lower_file` constitute changes to existing hir
  code.

## Data model

```rust
struct MacroDef { name: String, params: Vec<String>, body: Vec<Tok> }
struct MacroRegistry { macros: HashMap<String, MacroDef> }
```

A `Tok` is `(SyntaxKind, String)` — the lexer tag plus the source slice.
Trivia (whitespace, comments) is preserved so the spliced expansion is
human-readable in dumps.

## Expansion algorithm

```text
expand(def, args, ctx):
    if args.len() != def.params.len():
        Err(ArityMismatch)

    arg_toks = args.map(lex_fragment)         # bail out on lex errors
    bound  = let-bindings in def.body that aren't params

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

## Hygiene strategy: per-context mangling

v0.4 implements "expansion-time mangling" hygiene. Each macro
expansion gets a fresh `MacroContext` (a monotonic `u32`); every
*macro-introduced let-binding* in the body is renamed to
`__mac_<ctx>_<orig>`, and every reference to that binding inside the
same body is renamed to match.

* Parameters: substituted, never renamed.
* Free names (calls to `panic`, references to `prelude`-imported
  functions, type names, etc.): never renamed. They will be resolved
  by name resolution against the caller's scope, exactly like a
  hand-written inline expansion would be.
* `let` bindings introduced inside the body: renamed.

This works for v0.4's small surface: macros do not declare local
`fn`s, structs, or pattern-binding `let`s. Tuple patterns (`let (a, b)
= ...`) and struct patterns in macro bodies are explicitly out of
scope and reach name resolution unrenamed; v0.5's set-of-scopes
rewrite will handle them properly.

### Worked example: `twice(x)`

```text
macro twice(x) => { let y = x; y + y }
```

Call site: `twice(3)`, fresh `ctx = 11`.

Expansion:
```text
let __mac_11_y = (3); __mac_11_y + __mac_11_y
```

Even if the caller has its own `y`, the mangled identifier cannot
collide.

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
| SD6001 | `unknown_macro` — reserved for the planned `mac!name()` syntax (v0.5). |
| SD6002 | `macro_arity_mismatch` — call has wrong number of args.  |
| SD6003 | `macro_body_parse_failed` — expansion doesn't re-parse.  |
| SD6004 | `recursive_macro_too_deep` — depth cap (32) exceeded.    |

SD6001 is reserved but not produced by v0.4: with no syntactic marker
distinguishing a macro call from a regular function call, the
preprocessor cannot tell whether an unresolved `foo(...)` was *meant*
to be a macro. v0.5's `mac!foo(...)` proposal (or a similar marker)
will activate the check.

Codes live in `sdust-macros::diag` as bare `u16` constants; the HIR
integration wraps them in `DiagCode::new(N)` so we don't have to
modify `sdust-diagnostics` for each macro feature. A future cleanup
slice may merge them into the central catalog.

## v0.5 follow-on

* **`mac!name(...)` syntax** for macro call sites so SD6001 can become
  a real check and so authors can opt out of macro semantics on a
  per-call basis.
* **Set-of-scopes hygiene** (Racket-style) replacing the mangling
  scheme. This will support pattern-binding `let`s in macro bodies,
  macros that introduce nested `fn` items, and macros that need to
  reference identifiers from both the definition and the caller scope.
* **Procedural macros** (token-tree -> token-tree functions written in
  Stardust) — gated behind a `proc` capability and run in a sandboxed
  interpreter. The `MacroRegistry` already abstracts over `MacroDef`
  shape; an additional `ProcMacroDef` variant will slot in here.
* **Build-time compile-time function evaluation** (CTFE) so macros can
  consult constants, generate boilerplate from data files, etc. —
  again sandboxed, no I/O.
