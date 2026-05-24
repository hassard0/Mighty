# 03 — Generics

Generic parameters use square brackets, not angle brackets — this keeps
the syntax unambiguous with comparison operators. In **type** position
no turbofish is needed (`Result[T, E]`, `Option[T]`). In **expression**
position a `::` disambiguator separates generic arguments from index
access — see "Turbofish" below.

## The program

```sd
fn first[T](xs: &[T]) -> Option[&T] {
  if xs.len == 0 { None } else { Some(&xs[0]) }
}
```

## What is interesting

- `fn first[T](...)` declares a single generic type parameter `T`. No
  constraints — `T` may be any type that fits the body.
- `&[T]` is an immutable borrow of a slice of `T`. Stardust borrows look
  like Rust borrows but are written with square brackets for the slice
  shape.
- `Option[&T]` is the built-in optional type from `std.option`. The
  spec defines `Option[T] = Some(T) | None`.
- `if` is an expression. Both arms produce a value of type `Option[&T]`,
  so the whole `if` does.
- `&xs[0]` borrows the first element without copying it.

## Run it

```bash
sdust check examples/03_generic_fn.sd
```

## Turbofish — expression-position generics

In expression position, generic arguments require a `::` separator to
disambiguate from index expressions (where `Map[k]` already means
"index into `Map`"):

```sd
let m = Map::[Str, Json]{}      // struct literal with explicit types
let s = Some::[I32](42)         // constructor call with type arg
let v = Vec::[T].new()          // method call on a generic path
```

This is documented in `docs/spec/v0.1-amendments.md` A2. Type-position
generics remain bracket-only as above.

## Type errors you might see

```sd
// MT2004 wrong generic arity
fn f(x: Option[I32, Str]) -> Unit {}   // Option takes 1 arg

// Type-arg inference flows from arg to return:
fn id[T](x: T) -> T { x }
let n: I32 = id(42)        // T is inferred to I32
let s = id::[Str]("hi")    // T is explicit via turbofish
```

## Next

Continue to [04 — Errors](04-errors.md).
