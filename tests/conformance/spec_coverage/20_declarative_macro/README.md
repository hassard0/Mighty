# §20 metaprogramming — declarative macro

Canonical positive shape covering Spec v1.0-RC §20 (compile-time
metaprogramming):

- Declarative macro `inc(x) => { x + 1 }` with a fixed-arity param.
- Call site `inc(41)` expands cleanly before lowering.

Passes type-check clean.
