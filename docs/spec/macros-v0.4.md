# Mighty macros — v0.4 spec

Status: **shipped in v0.4**.
Supersedes the placeholder discussion in spec §20.3 (v0.1).
Normative companions: spec §3.3 (`macro` keyword), spec §30.2 (deferred
feature index).

## Goals

* **Predictable.** A v0.4 macro is a *token template* parameterised by
  named parameters. Expansion is deterministic and visible in
  `mty dump --hir`.
* **Hygienic.** Macro-introduced bindings cannot capture or be captured
  by the caller's bindings.
* **Sandboxed by shape.** A macro body is data; the macro system
  executes no user code at build time. No I/O, no FFI, no environment
  access.
* **Bounded.** Recursive expansion is capped at 32 levels; the cap is
  part of the spec, not an implementation detail.

## Non-goals (deferred to v0.5+)

* Procedural macros (token-tree → token-tree functions).
* Hygienic set-of-scopes name resolution. v0.4 uses
  expansion-time mangling — sound for the v0.4 surface, deliberately
  simpler than a full set-of-scopes implementation.
* `mac!name(...)` syntactic marker. v0.4 reuses ordinary function-call
  syntax; the marker (and the matching MT6001 check) lands in v0.5.
* Compile-time function evaluation.

## Grammar

```
MACRO_DECL = "macro" NAME "(" PARAM_LIST? ")" "=>" "{" BODY_TOKENS "}"
PARAM_LIST = NAME ("," NAME)*
BODY_TOKENS = ... brace-balanced opaque tokens ...
```

A macro decl is an item. It is visible from the point of declaration to
the end of the enclosing file (no forward references across files in
v0.4; cross-file macro export is v0.5 territory).

## Call sites

v0.4 has no syntactic marker for macro calls. A macro is invoked using
ordinary function-call syntax:

```mty
macro assert_eq(a, b) => {
  if a != b { panic("assert_eq failed") }
}

fn main() {
  assert_eq(1 + 1, 2)
}
```

The HIR lowering pre-pass replaces every `CALL_EXPR` whose callee
resolves to a registered macro with the expanded body, then re-parses
the resulting source. After this pre-pass, downstream stages see only
the expansion.

## Expansion semantics

For an expansion of `name(arg_1, ..., arg_n)` against a definition
`macro name(p_1, ..., p_n) => { body }`:

1. **Arity check.** If `n` does not equal the declared parameter count,
   diagnostic **MT6002** is raised and the call is replaced with the
   integer literal `0` to keep the surrounding parse well-formed.
2. **Parameter substitution.** Each `IDENT` token in `body` whose text
   matches `p_i` is replaced by `( arg_i_tokens )`. The surrounding
   parens preserve precedence at the call site.
3. **Hygiene mangling.** Any identifier introduced by a `let` binding
   inside the body — and any subsequent reference to that name within
   the same body — is renamed to `__mac_<ctx>_<orig>`, where `<ctx>`
   is a fresh per-expansion counter and `<orig>` is the source name.
   Parameters and free names are not renamed.
4. **Splice + re-parse.** The expansion is spliced into the source at
   the call site's byte range. The full file is re-parsed.
5. **Iterate.** If the re-parsed source contains further macro calls
   (because the expansion itself called another macro, or because an
   outer macro produced a call after substitution), repeat from step 1
   for that next layer. Stop when no macro calls remain or after 32
   iterations.

`MAX_EXPANSION_DEPTH = 32` is part of this spec. Hitting it raises
**MT6004** for every call still present and aborts further expansion.

## Hygiene scope (v0.4 subset)

Hygiene applies to the simple binding shape `let IDENT [: TYPE] = ...`
and `let mut IDENT [: TYPE] = ...`. Tuple, struct, and reference
patterns in `let` inside macro bodies are explicitly out of scope:
their bindings will *not* be mangled in v0.4, and any collision with a
caller binding is a real shadowing situation that name resolution will
report normally.

This restriction is acceptable because v0.4 declarative macros are
intended for small templates (assertions, guards, simple sugar);
authors needing complex local-binding patterns should hand-write the
code or wait for v0.5's set-of-scopes hygiene.

## Diagnostic codes

* **MT6001** `unknown_macro` — reserved; not raised in v0.4 (see
  Non-goals).
* **MT6002** `macro_arity_mismatch` — wrong arg count.
* **MT6003** `macro_body_parse_failed` — expanded body did not parse.
* **MT6004** `recursive_macro_too_deep` — depth cap exceeded.

## Worked acceptance example

```mty
macro assert_eq(a, b) => {
  if a != b { panic("assert_eq failed") }
}

fn main() {
  assert_eq(1 + 1, 2)
}
```

After expansion, `fn main` lowers to a `HirExpr::If` over
`(1 + 1) != (2)` with a `panic("assert_eq failed")` consequent — the
same HIR as if the user had typed the `if` by hand. Verifiable via:

```
mty check examples/16_macro.sd   # passes
mty dump --hir examples/16_macro.sd
```

## Compatibility

v0.4 declarative macros are forward-compatible with v0.5's planned
additions:

* Adding `mac!name(...)` syntax does not change the meaning of any
  existing v0.4 program (which never uses the marker).
* Switching the hygiene engine from mangling to set-of-scopes
  preserves the v0.4 behaviour for programs whose `let` bindings
  follow the v0.4 supported shapes.
* Introducing procedural macros adds a new MacroDef variant; the
  registry and the HIR-lowering hook stay shape-compatible.
