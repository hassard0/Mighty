# §6 types — struct + enum + generic

Canonical positive shape covering Spec v1.0-RC §6 (type system):

- Generic struct `Pair[K, V]` with named fields.
- Generic enum `Either[A, B]` with payload variants.
- Generic-instantiating function returning a fully-applied
  `Either[I64, Str]`.

Passes type-check clean (exit 0).
