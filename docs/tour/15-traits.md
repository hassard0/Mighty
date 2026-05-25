# Traits

Mighty traits group related behavior behind a shared method set,
analogous to Rust traits or interfaces.

## Declaration

```mty
trait Hash {
  fn hash(self) -> U64
}
```

A trait is declared at the top level. Each method must specify its
signature; bodies (default methods) are reserved for a later slice.

## Implementation

```mty
struct UserId {
  v: U64
}

impl Hash for UserId {
  fn hash(self) -> U64 { v }
}
```

The compiler enforces **coherence**: at most one
`impl Trait for Type` per (trait, type) pair. A duplicate raises
`MT4022 trait_coherence_violation`.

## Dispatch

When you call `x.hash()`:

1. **Inherent first.** If `impl T { fn hash(self) -> U64 }` exists for
   `T`, it wins regardless of trait impls.
2. **Trait fallback.** If exactly one trait in scope provides `hash`
   for `T`, that impl dispatches. Two or more in scope: ambiguous,
   `MT4020 method_ambiguous` — disambiguate by importing fewer traits
   or using an explicit trait-qualified call (slice 5 doesn't yet
   parse that syntax; the diagnostic lists candidates).
3. **None of the above.** `MT4021 method_not_found`.

## `dyn Trait`

A `dyn Trait` value erases the static type and dispatches
dynamically:

```mty
trait Show {
  fn show(self) -> Str
}

impl Show for I32 {
  fn show(self) -> Str { "<int>" }
}

fn print(s: dyn Show) -> Unit {
  log(s.show())
}
```

Slice 5 keeps object-safety conservative: a trait used through `dyn`
may not mention `Self` in any method parameter or return, and may not
have method-level generics. Violation: `MT4023 dyn_requires_object_safe`.

## `#[derive(Copy, Hash, Eq)]`

The derive attribute generates conformance to a trait without writing
the body. Slice 5 supports `Copy`, `Hash`, and `Eq`:

```mty
#[derive(Copy)]
struct Vec2 {
  x: F32
  y: F32
}
```

`Copy` validates every field's type is itself Copy
(`MT4040 derive_copy_field_not_copy` otherwise). `Hash` / `Eq` register
synthetic impls so `dyn Hash = my_t` resolves.

A shorthand `derive Copy struct Vec2 { ... }` is also accepted.

Unknown derive names raise `MT4041 derive_unknown`.

## Try it

There is no single `15_traits.mty` example — trait declarations and
impls appear across most of the larger examples (`19_backend_service`,
`20_frontend_component`). Paste any snippet from this chapter into
`scratch.mty` and run:

```bash
mty check scratch.mty
```

The conformance corpus at
[`tests/conformance/traits/`](https://github.com/hassard0/Mighty/tree/main/tests/conformance)
exercises the full dispatch + coherence machinery.

## End of the tour

You've reached the end of the chapter-by-chapter tour. Next steps:

- Read the normative spec at
  [`docs/spec/v1.0-rc.md`](../spec/v1.0-rc.md).
- Browse the [CLI reference](../reference/cli/mty.md).
- Skim the [FAQ](../faq.md).
- Open an issue on
  [github.com/hassard0/Mighty](https://github.com/hassard0/Mighty) if
  you find anything that surprises you.
