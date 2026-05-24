# 04_undeclared_alloc

v0.3 (A65) conformance: a pub fn that allocates via `to_string` (or any
heap-growing builtin) must declare `alloc` in its effect set. Without
`effect alloc` the effect inferencer surfaces MT4001 because the inferred
set `{alloc}` is not a subset of the declared `{}`.
