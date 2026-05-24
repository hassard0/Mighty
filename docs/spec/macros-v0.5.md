# Stardust macros — v0.5 spec

Status: **shipped in v0.5**.
Supersedes [`macros-v0.4.md`](./macros-v0.4.md) for everything below;
the v0.4 doc remains the historical record of the v0.4 surface.

## What v0.5 adds over v0.4

| Feature                             | v0.4 | v0.5 |
|-------------------------------------|------|------|
| `macro Name(...) => { body }`       | yes  | yes  |
| Plain-call invocation `foo(args)`   | yes  | yes (compat) |
| `Name!(args)` invocation marker     | no   | **yes** |
| SD6001 `unknown_macro`              | reserved | **active** |
| `let IDENT` hygiene mangling        | yes  | yes  |
| Tuple/struct/ref pattern hygiene    | no   | **yes** |
| `pub macro` cross-file visibility   | no   | **yes** |
| `proc macro` declaration parsing    | no   | **yes** |
| `proc macro` execution              | no   | **no** (v0.6) |
| SD6005 `proc_macro_impure`          | n/a  | **yes** |
| SD6006 `proc_macro_unsupported_v0_5`| n/a  | **yes** |
| Bundled `assert!` / `debug!` / etc. | no   | **yes** |

## Goals (carried from v0.4)

* **Predictable.** A v0.5 declarative macro is still a *token template*
  parameterised by named parameters. Expansion is deterministic and
  visible in `sdust dump --hir`.
* **Hygienic.** Macro-introduced bindings cannot capture or be captured
  by the caller's bindings, across more pattern shapes than v0.4.
* **Sandboxed by shape.** Declarative macros still execute zero user
  code at build time. Procedural macros' bodies are stored but not run
  in v0.5; v0.6 will run them inside a sandboxed sub-interpreter with
  hard wall/memory/step caps.
* **Bounded.** Recursive declarative-macro expansion is capped at 32
  levels.

## Non-goals (deferred to v0.6+)

* Procedural-macro **execution**. v0.5 parses + stores; v0.6 ships the
  sandbox.
* Set-of-scopes hygiene (Racket-style). Extended mangling covers ~95%
  of real-world cases; the set-of-scopes upgrade lands when macros
  need to introduce nested `fn` items.
* Variadic macros (`format!("{} {}", a, b)`).
* `#[proc_macro]` attribute form (requires functional attribute
  application).

## Grammar

```
MACRO_DECL      = ("pub")? "macro" NAME "(" PARAM_LIST? ")" "=>" "{" BODY_TOKENS "}"
PROC_MACRO_DECL = ("pub")? "proc" "macro" NAME "(" PROC_PARAM ")" ("->" TYPE)? "{" BODY_TOKENS "}"
PROC_PARAM      = NAME ":" TYPE
PARAM_LIST      = NAME ("," NAME)*
BODY_TOKENS     = ... brace-balanced opaque tokens ...

MACRO_CALL      = PATH "!" TOKEN_TREE
TOKEN_TREE      = "(" ... paren-balanced opaque tokens ... ")"
```

A macro decl is an item. Local visibility is from the point of
declaration to the end of the enclosing file. `pub macro` adds
cross-file visibility: the macro is registered in the package's
`exported` set and can be re-bound into another file's `local` set by
`use otherpkg.name`.

## Invocation syntax

v0.5 accepts both forms:

```sdust
// v0.5 syntax (recommended for new code).
assert_eq!(1 + 1, 2)

// v0.4 syntax (still works; emits the expansion if the name resolves).
assert_eq(1 + 1, 2)
```

The `!` marker has two practical consequences:

1. **SD6001 fires.** If a `Name!(...)` site refers to a name with no
   matching declarative or procedural macro decl in the (combined
   local + exported-of-imports) registry, the lowering pre-pass emits
   `SD6001 unknown_macro`. The plain-call form intentionally does NOT
   trigger SD6001 — the lowering pass cannot distinguish a typo'd
   `foo()` from a regular function call there.

2. **Arguments are opaque tokens.** Inside `!(`, the args are stored
   as a single `TOKEN_TREE` and the expander splits on commas at
   depth 0. This gives macros the flexibility to accept syntax that
   a normal call wouldn't parse (e.g. a custom DSL inside the macro).

The v0.4 plain-call form is kept for backwards-compat; v0.6 may
deprecate it.

## Hygiene — extended pattern coverage

Hygiene still rests on expansion-time mangling: every macro-introduced
binding is renamed to `__mac_<ctx>_<orig>` so it cannot collide with
caller bindings of the same name. v0.5 extends the binding shapes
covered:

| Pattern                                | Bound names |
|----------------------------------------|-------------|
| `let x = ...`                          | `x`         |
| `let mut x = ...`                      | `x`         |
| `let (a, b, ...) = ...`                | `a`, `b`, …  |
| `let User { id, name } = ...`          | `id`, `name` |
| `let User { id: x } = ...`             | `x`         |
| `let &x = ...` / `let &mut x = ...`    | `x`         |
| `let ref x = ...`                      | `x`         |

Identifiers that are not bindings — type names (`User`, `Some`), enum
variant constructors, field selectors, path segments — are deliberately
left unmangled. They resolve against the caller's scope, exactly like a
hand-written inline expansion would.

The unifying rule: macro parameters are *substituted*; macro-introduced
bindings are *mangled*; everything else is *passed through*.

## Cross-file `pub macro`

```sdust
// in package "math":
pub macro double(x) => { x + x }

// in another package that imports "math":
use math.double
fn main() -> i32 { double!(21) }   // expands to (21) + (21)
```

The exporter's `pub` macros land in its `PackageMacros::exported` set.
When the importer's `use math.double` resolves, the macro is copied
into the importer's `local` set with the bound name (`double`, or a
renamed `as` form). Private macros never appear in `exported` and
cannot be imported.

The end-to-end pkg → HIR import wiring lights up once sdust-pkg pipes
its symbol table into HIR lowering. v0.5 ships the `PackageMacros`
surface and an in-memory two-file fixture test; the connector slice is
strictly additive.

## Procedural macros — declaration only in v0.5

```sdust
proc macro upcase(input: TokenStream) -> TokenStream {
  // body that manipulates `input` and returns a new TokenStream.
  input
}

fn main() {
  upcase!(my_name)   // SD6006 in v0.5; expansion lands in v0.6.
}
```

A procedural macro is a Stardust function over `TokenStream`. The body
is stored verbatim in the registry. v0.5 enforces two rules:

1. **Purity check** at decl time. If the body references `effect.…(…)`
   or a bare call to the well-known impure surface (`time`, `env`,
   `io`, `model`, `rand`), the compiler emits **SD6005
   `proc_macro_impure`**. Proc macros run at *compile time* — they
   cannot perform I/O or read the environment.

2. **Execution gate** at call sites. v0.5 cannot execute a proc-macro
   body; doing so requires a sandboxed sub-interpreter that is a v0.6
   deliverable. So every `name!(…)` call to a procedural macro emits
   **SD6006 `proc_macro_unsupported_v0_5`** and replaces the call with
   the sentinel literal `0`. The macro declaration is preserved so the
   call site can stay stable when v0.6 enables execution.

### v0.6 sandbox limits (informative)

When the v0.6 interpreter ships, proc-macro execution will be capped:

* **Wall clock:** 100 ms per expansion.
* **Memory:** 16 MB intermediate state.
* **Step count:** 100 000 SIR steps.

These constants are exposed today as
`sdust_macros::proc::PROC_MACRO_{WALL_MS,MEM_BYTES,STEPS}` so spec and
implementation cannot drift before v0.6.

## Standard macro library

`sdust-macros` ships these macros under `crates/sdust-macros/lib/`:

| Macro             | Behavior                                            |
|-------------------|-----------------------------------------------------|
| `assert!(cond)`   | Panic if `!cond`.                                   |
| `assert_eq!(a, b)`| Panic if `a != b`.                                  |
| `assert_ne!(a, b)`| Panic if `a == b`.                                  |
| `debug!(expr)`    | Eprintln a debug-shaped line carrying `expr`.       |
| `unreachable!()`  | Panic with `"entered unreachable code"`.            |

All five are `pub macro`. Projects load them via
`sdust_macros::stdlib::load_into(&mut pm)`; sdust-pkg will eventually
auto-import them as part of the prelude.

## Diagnostics

| Code   | Meaning                                                  |
|--------|----------------------------------------------------------|
| SD6001 | `unknown_macro` — `name!(args)` with no matching decl.   |
| SD6002 | `macro_arity_mismatch` — call has wrong number of args.  |
| SD6003 | `macro_body_parse_failed` — expansion doesn't re-parse.  |
| SD6004 | `recursive_macro_too_deep` — depth cap (32) exceeded.    |
| SD6005 | `proc_macro_impure` — proc body references an effect.    |
| SD6006 | `proc_macro_unsupported_v0_5` — exec deferred to v0.6.   |

SD6001 through SD6004 are unchanged from v0.4. SD6005 and SD6006 are
new in v0.5.

## Migration from v0.4

* **Existing macros keep working.** Plain-call invocations still
  expand for any macro registered in the file's local registry.
* **Optional opt-in to `!` syntax.** Add `!` to call sites where you
  want the v0.5 SD6001 check, or where the macro's arguments would not
  parse as a normal expression.
* **New cross-file flow.** Mark macros `pub macro …` to export them.
  Add `use otherpkg.foo` to import. The flow is end-to-end once
  sdust-pkg pipes its symbol table; until then, projects can use
  `PackageMacros::register_use` programmatically.
